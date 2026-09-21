<script lang="ts">
  import { onMount } from 'svelte'
  import type { EcgStreamMsg } from './types'
  import { EcgScope } from './ecgScope'

  // RAW ECG (finpuss 21.09.2026): ingen kildevelger - fanen viser automatisk
  // strømmen(e) som faktisk kjører. Ved dual-H10 stables to like kliniske
  // strimler: belte A (ESP32) øverst, belte B (BT-600) under (fast rekkefølge,
  // avtalt med Jørn 21.09). Øktstyring bor på TILKOBLING-fanen.

  interface Props {
    sources: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: EcgStreamMsg) => void) => void
    onstatus: (source: string) => { state: string; detail: string; device: string } | null
  }
  let { sources, send, register, onstatus }: Props = $props()
  void send // øktstyring skjer på TILKOBLING-fanen

  const ECG_FS = 130
  const SPEEDS = [25, 50] // mm/s selectable
  const MAX_STRIPS = 2

  // Ett skop per kilde (ikke-reaktivt; svelte-state speiles per frame).
  const scopes: Record<string, EcgScope> = {}
  function scopeFor(src: string): EcgScope {
    return (scopes[src] ??= new EcgScope())
  }

  let paused = $state(false)
  let speed = $state(25)
  let active: string[] = $state([])
  let canvases: (HTMLCanvasElement | undefined)[] = $state([undefined, undefined])
  // Speilede visningsverdier per aktiv strimmel (indeks følger `active`).
  let mirror: { bpm: number | null; total: number; gaps: number; fresh: boolean }[] = $state([])

  const lastSeen: Record<string, number> = {}
  const streamingSince: Record<string, number> = {}

  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (weak)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB'
    if (id.startsWith('esp32-')) return 'ESP32 - Polar H10'
    return `native agent (${id.split(':')[0]})`
  }
  // Fast rekkefølge: belte A (ESP32) øverst, deretter BT-600, så andre.
  function orderKey(id: string): string {
    if (id.startsWith('esp32-')) return '0' + id
    if (id === 'raven:hci1') return '1' + id
    return '2' + id
  }

  function computeActive(perf: number): string[] {
    const ids = Object.keys(sources).filter((id) => id !== 'synth')
    const act = ids.filter((id) => {
      const st = onstatus(id)
      const fresh = perf - (lastSeen[id] ?? 0) < 3000
      return fresh || st?.state === 'streaming'
    })
    act.sort((a, b) => orderKey(a).localeCompare(orderKey(b)))
    return act.slice(0, MAX_STRIPS)
  }

  onMount(() => {
    register((m: EcgStreamMsg) => {
      if (m.t !== 'ecg' || (m as any).source === 'synth') return
      const src = (m as any).source as string
      scopeFor(src).ingestEcg(m as any)
      lastSeen[src] = performance.now()
    })
    let raf = 0
    const loop = () => {
      const perf = performance.now()
      const act = computeActive(perf)
      // Oppdater reaktiv liste bare ved reell endring (unngå re-render-storm).
      if (act.join('|') !== active.join('|')) active = act
      const mir: typeof mirror = []
      act.forEach((src, i) => {
        const scope = scopeFor(src)
        const fresh = perf - scope.lastEcgMs < 1500
        const st = onstatus(src)
        const live = st?.state === 'streaming'
        if (live && !streamingSince[src]) streamingSince[src] = perf
        if (!live) streamingSince[src] = 0
        scope.tick(perf, fresh && !paused)
        mir.push({ bpm: scope.hrBpm, total: scope.ecgTotal, gaps: scope.gaps, fresh })
        const canvas = canvases[i]
        if (canvas) drawStrip(canvas, scope, src, live, perf)
      })
      mirror = mir
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  })

  function togglePause() {
    if (paused) {
      for (const s of Object.values(scopes)) s.requestAnchor()
      paused = false
    } else paused = true
  }

  function prepare(canvas: HTMLCanvasElement): [CanvasRenderingContext2D, number, number] | null {
    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (!w || !h) return null
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr)
      canvas.height = Math.round(h * dpr)
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return null
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)
    return [ctx, w, h]
  }

  function drawStrip(canvas: HTMLCanvasElement, scope: EcgScope, src: string, live: boolean, perf: number) {
    const p = prepare(canvas)
    if (!p) return
    const [ctx, w, h] = p
    const drew = scope.drawScope(ctx, w, h, speed)

    if (drew < 2) {
      // Oppvarmings-overlay: H10 holder PMD-strømmen tilbake til "measuring"
      // (~5-35 s med tørre elektroder).
      const cx = w / 2, cy = h / 2
      const tsec = live && streamingSince[src] ? (perf - streamingSince[src]) / 1000 : 0
      const pulse = 0.5 + 0.5 * Math.sin(perf / 350)
      ctx.save()
      ctx.textAlign = 'center'
      if (live) {
        ctx.fillStyle = `rgba(203,51,59,${0.2 + 0.55 * pulse})`
        ctx.beginPath(); ctx.arc(cx, cy - 16, 8 + 4 * pulse, 0, Math.PI * 2); ctx.fill()
      }
      ctx.fillStyle = '#444'
      ctx.font = 'bold 17px sans-serif'
      ctx.fillText(live ? 'Polar H10 preparing signal...' : 'venter på strøm', cx, cy + 16)
      if (live) {
        ctx.fillStyle = '#888'
        ctx.font = '13px sans-serif'
        ctx.fillText(
          `waiting for first ECG frame - ${tsec.toFixed(0)} s  (can take ~30 s with dry electrodes)`,
          cx, cy + 38,
        )
      }
      ctx.textAlign = 'left'
      ctx.restore()
    }

    // NO SIGNAL når signalet er for støyete eller ingen slag er sett nylig
    if (drew >= 2 && (scope.poorState || scope.nowT - scope.lastPeakT > 2)) {
      ctx.fillStyle = '#b06a00'
      ctx.font = 'bold 13px sans-serif'
      ctx.fillText('NO SIGNAL', w - 96, 22)
    }
  }
</script>

<div class="ecg">
  <div class="bar">
    <button class="pause" disabled={!active.length && !paused} onclick={togglePause}>{paused ? 'RESUME' : 'PAUSE'}</button>
    <div class="speed">
      {#each SPEEDS as s (s)}
        <button class="sp" class:on={speed === s} onclick={() => (speed = s)}>{s}</button>
      {/each}
      <span class="unit">mm/s</span>
    </div>
    {#if !active.length}
      <span class="state">ingen aktiv strøm - start økten fra TILKOBLING-fanen</span>
    {/if}
  </div>

  {#if active.length}
    {#each active as src, i (src)}
      {@const st = onstatus(src)}
      {@const m = mirror[i]}
      <div class="strip">
        <div class="striphead">
          <b>{friendlyLabel(src)}</b>
          {#if st?.device}<span class="dev">{st.device}</span>{/if}
          <span class="bpm">&hearts; <b>{m?.bpm ?? '--'}</b> bpm</span>
          <span class="stats">
            <span class="state" class:err={st?.state === 'error'}>{st ? st.state + (st.detail ? ' - ' + st.detail : '') : ''}</span>
            <span>{(m?.total ?? 0).toLocaleString()} samples</span>
            <span>{((m?.total ?? 0) / ECG_FS).toFixed(1)} s</span>
            <span class:warn={(m?.gaps ?? 0) > 0}>{m?.gaps ?? 0} dropped</span>
          </span>
        </div>
        <div class="scope ecgscope">
          <canvas bind:this={canvases[i]}></canvas>
          <div class="scale">ECG - 130 Hz - {speed} mm/s - 10 mm/mV</div>
        </div>
      </div>
    {/each}
  {:else}
    <div class="scope ecgscope idle">
      <div class="idlemsg">
        Ingen aktiv strøm. Start økten fra <b>TILKOBLING</b>-fanen - strimlene
        dukker opp her av seg selv (én per belte ved dual-H10).
      </div>
    </div>
  {/if}
</div>

<style>
  .ecg {
    height: calc(100vh - 58px);
    display: flex;
    flex-direction: column;
    padding: 14px;
    box-sizing: border-box;
    gap: 12px;
    overflow-y: auto;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }
  button {
    font-family: var(--font-display);
    font-size: 17px;
    letter-spacing: 2px;
    padding: 7px 26px;
    border: none;
    border-radius: 10px;
    cursor: pointer;
    color: #fff;
  }
  button.pause {
    background: var(--color-slate);
  }
  button.pause:disabled {
    background: var(--color-disabled);
    cursor: not-allowed;
  }
  .bpm {
    font-size: 14px;
    color: var(--color-slate);
  }
  .bpm b {
    color: var(--color-heart);
    font-size: 18px;
  }
  .speed {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .speed .sp {
    font-family: var(--font-body);
    font-size: 12px;
    letter-spacing: 0;
    padding: 4px 10px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-card);
    color: var(--color-slate);
  }
  .speed .sp.on {
    background: var(--color-slate);
    color: #fff;
    border-color: var(--color-slate);
  }
  .speed .unit {
    font-size: 12px;
    color: var(--color-slate);
  }
  .strip { display: flex; flex-direction: column; gap: 6px; }
  .striphead {
    display: flex;
    align-items: baseline;
    gap: 14px;
    font-size: 13.5px;
    color: var(--color-slate);
    flex-wrap: wrap;
  }
  .striphead > b {
    font-family: var(--font-display);
    letter-spacing: 1px;
    color: var(--color-ink, #222);
    font-size: 13px;
  }
  .striphead .dev { font-size: 12px; }
  .stats { display: flex; gap: 14px; flex-wrap: wrap; }
  .stats .warn { color: var(--color-warning); font-weight: 600; }
  .state { text-transform: lowercase; }
  .state.err { color: var(--color-error); }
  .scope {
    position: relative;
    background: var(--color-card);
    border: 1px solid var(--color-line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .ecgscope {
    /* Klinisk strimmelhøyde (~+/-2 mV), fast med vilje - to strimler stables. */
    flex: 0 0 220px;
  }
  .ecgscope.idle { display: flex; align-items: center; justify-content: center; }
  .idlemsg { color: var(--color-slate); font-size: 14px; max-width: 480px; text-align: center; }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .scale {
    position: absolute;
    right: 12px;
    bottom: 10px;
    font-size: 13px;
    color: var(--color-slate);
    background: rgba(243, 241, 236, 0.7);
    padding: 2px 8px;
    border-radius: 6px;
  }
</style>
