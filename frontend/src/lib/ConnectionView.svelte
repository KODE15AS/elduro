<script lang="ts">
  import { onMount } from 'svelte'

  // TILKOBLING: all øktstyring og tilkoblingsdiagnostikk samlet (20.09.2026).
  // Ett kort per kilde - designet for N kilder (dual-H10 kommer).
  type Status = { state: string; detail: string; device: string } | null
  interface Props {
    sources: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: any) => void) => void
    onstatus: (source: string) => Status
  }
  let { sources, send, register, onstatus }: Props = $props()

  const sourceIds = $derived(Object.keys(sources))

  // Per kilde/strøm: rullerende 5 s-vindu for ratemåling.
  type StreamStat = { win: { t: number; n: number }[]; lastMs: number; total: number; seq: number }
  let stats: Record<string, Record<string, StreamStat>> = $state({})
  let telemetry: Record<string, any> = $state({})
  let modes: Record<string, string> = $state({})
  let pending: Record<string, boolean> = $state({}) // optimistisk: satt ved klikk
  let tick = $state(0) // driver re-render av ratene

  function onFrame(m: any) {
    if (m.t === 'telemetry') {
      telemetry[m.source] = { ...m, rxMs: performance.now() }
      return
    }
    if (!['ecg', 'acc', 'hr'].includes(m.t)) return
    const s = (stats[m.source] ??= {})
    const st = (s[m.t] ??= { win: [], lastMs: 0, total: 0, seq: 0 })
    const now = performance.now()
    const n = m.t === 'hr' ? 1 : (m.samples?.length ?? 0)
    st.win.push({ t: now, n })
    st.lastMs = now
    st.total += n
    if (m.seq) st.seq = m.seq
    while (st.win.length && now - st.win[0].t > 5000) st.win.shift()
  }

  onMount(() => {
    register(onFrame)
    const iv = setInterval(() => {
      tick++
      // Rydd optimistisk pending når en definitiv status har kommet: strømmer
      // (rammer flyter) eller økten tok ikke / ble avsluttet.
      for (const id of Object.keys(pending)) {
        if (!pending[id]) continue
        const st = onstatus(id)
        if (isStreaming(id) || st?.state === 'error' || st?.state === 'stopped') {
          pending[id] = false
        }
      }
    }, 1000)
    return () => clearInterval(iv)
  })

  const NOMINAL: Record<string, number> = { ecg: 130, acc: 200, hr: 1 }

  function rate(src: string, t: string): number {
    void tick
    const st = stats[src]?.[t]
    if (!st || !st.win.length) return 0
    const now = performance.now()
    if (now - st.lastMs > 3000) return 0
    const span = Math.max(now - st.win[0].t, 1000)
    return st.win.reduce((a, b) => a + b.n, 0) / (span / 1000)
  }
  function streamOk(src: string, t: string): boolean {
    const r = rate(src, t)
    return t === 'hr' ? r >= 0.5 : r >= NOMINAL[t] * 0.8
  }
  function tele(src: string): any | null {
    void tick
    const t = telemetry[src]
    if (!t || performance.now() - t.rxMs > 15000) return null
    return t
  }

  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (svak, kun nød)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB (benk)'
    if (id.startsWith('esp32-')) return 'ESP32 feltbro - Polar H10'
    return `agent (${id.split(':')[0]})`
  }

  function start(src: string) {
    pending[src] = true // umiddelbar respons; status/rammer overtar straks
    send({ t: 'start', source: src, mode: modes[src] ?? 'hrv', duration_s: 0 })
  }
  function stop(src: string) {
    pending[src] = false
    send({ t: 'stop', source: src })
  }
  function isStreaming(src: string): boolean {
    void tick
    const s = stats[src]
    if (!s) return false
    return Object.values(s).some((st) => performance.now() - st.lastMs < 3000)
  }
  // «Busy» = en økt er ønsket/i gang (ren funksjon - ingen mutasjon i render).
  // Optimistisk pending gir umiddelbar knapperespons; status og rammer holder
  // den korrekt videre. pending ryddes i tick-intervallet.
  function busy(src: string, st: Status): boolean {
    void tick
    return (
      pending[src] ||
      isStreaming(src) ||
      st?.state === 'streaming' ||
      st?.state === 'scanning' ||
      st?.state === 'connecting'
    )
  }
</script>

<main class="conn">
  {#if !sourceIds.length}
    <section class="card empty">
      <h2>Ingen agenter tilkoblet</h2>
      <p>
        ESP32-broen kobler seg til via hotspoten (WiFi må være på), og
        raven-agenten via USB-adapteren. Ingen av dem er online nå.
      </p>
    </section>
  {/if}

  {#each sourceIds as id (id)}
    {@const st = onstatus(id)}
    {@const tm = tele(id)}
    <section class="card">
      <header>
        <h2>{friendlyLabel(id)}</h2>
        <code>{id}</code>
      </header>

      <div class="grid">
        <div class="block">
          <h3>Økt</h3>
          <div class="controls">
            <select bind:value={modes[id]}>
              <option value="hrv">hrv - EKG + ACC + HR/RR (anbefalt)</option>
              <option value="ecg">ecg - EKG + ACC</option>
              <option value="hr">hr - kun HR/RR</option>
            </select>
            {#if busy(id, st)}
              <button class="stop" onclick={() => stop(id)}>STOPP</button>
            {:else}
              <button class="start" onclick={() => start(id)}>START</button>
            {/if}
          </div>
          <p class="status" class:err={st?.state === 'error'}>
            {st ? `${st.state}${st.detail ? ' - ' + st.detail : ''}` : 'idle'}
          </p>
          <p class="hint">Nyeste start vinner: en start her stopper andre kilder (H10 = én sentral).</p>
        </div>

        <div class="block">
          <h3>Strømmer (bekreftelse)</h3>
          <table>
            <tbody>
              {#each ['ecg', 'acc', 'hr'] as t (t)}
                <tr>
                  <td><span class="dot" class:ok={streamOk(id, t)} class:off={rate(id, t) === 0}></span> {t.toUpperCase()}</td>
                  <td class="num">{rate(id, t) ? rate(id, t).toFixed(1) : '-'} {t === 'hr' ? 'rammer/s' : 'Hz'}</td>
                  <td class="dim">nominelt {NOMINAL[t]} {t === 'hr' ? '/s' : 'Hz'}</td>
                  <td class="dim">seq {stats[id]?.[t]?.seq || '-'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>

        <div class="block">
          <h3>Polar H10</h3>
          <table>
            <tbody>
              <tr><td>Enhet</td><td>{tm?.device || st?.device || '-'}</td></tr>
              <tr><td>Batteri</td><td>{tm && tm.battery >= 0 ? tm.battery + ' %' : '-'}</td></tr>
              <tr><td>BLE-link</td><td>{tm ? (tm.ble_connected ? 'tilkoblet' : 'frakoblet') : '-'}</td></tr>
              <tr><td>BLE-RSSI</td><td>{tm?.ble_connected && tm.ble_rssi ? tm.ble_rssi + ' dBm' : '-'}</td></tr>
            </tbody>
          </table>
        </div>

        <div class="block">
          <h3>Bro / agent</h3>
          {#if tm}
            <table>
              <tbody>
                <tr><td>WiFi-RSSI</td><td>{tm.wifi_rssi} dBm</td></tr>
                <tr><td>Chip-temp</td><td class:warn={tm.chip_c > 75}>{tm.chip_c > 0 ? tm.chip_c.toFixed(0) + ' °C' : '-'}</td></tr>
                <tr><td>Fritt minne</td><td>{tm.heap_kb} kB</td></tr>
                <tr><td>microSD</td><td>{tm.sd ? 'montert (spill aktiv)' : 'ikke montert'}</td></tr>
                <tr><td>Oppetid</td><td>{Math.floor(tm.uptime_s / 60)} min</td></tr>
              </tbody>
            </table>
          {:else}
            <p class="dim">Ingen telemetri (raven-agenten sender ikke telemetri; ESP32-broen sender hvert 5. sekund).</p>
          {/if}
        </div>
      </div>
    </section>
  {/each}

  <section class="card foot">
    <p>
      Visningene <b>RAW ECG</b> og <b>RHYTHM/HRV</b> følger automatisk økten som
      startes her. Bruk <b>hrv</b>-modus for å mate begge (EKG + ACC + nativ
      HR/RR). H10 trenger ~5–35 s oppvarming før første EKG-ramme.
    </p>
  </section>
</main>

<style>
  .conn {
    height: calc(100vh - 58px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px 18px;
    box-sizing: border-box;
  }
  .card {
    border: 1px solid var(--color-line);
    border-radius: 12px;
    background: var(--color-card);
    padding: 14px 18px;
  }
  .card.empty, .card.foot { color: var(--color-slate); }
  .card header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 10px;
  }
  h2 {
    font-family: var(--font-display);
    font-size: 17px;
    letter-spacing: 1.5px;
    margin: 0;
  }
  h3 {
    font-family: var(--font-display);
    font-size: 12px;
    letter-spacing: 1.2px;
    color: var(--color-slate);
    margin: 0 0 8px;
  }
  code { font-size: 12px; color: var(--color-slate); }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
    gap: 18px;
  }
  .controls { display: flex; gap: 8px; align-items: center; }
  select {
    padding: 5px 8px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-bg);
    font-family: var(--font-body);
  }
  button.start, button.stop {
    border: 0;
    border-radius: 8px;
    padding: 6px 18px;
    cursor: pointer;
    color: #fff;
    font-family: var(--font-display);
    letter-spacing: 1.5px;
    font-size: 13px;
  }
  button.start { background: var(--color-accent); }
  button.stop { background: var(--color-heart); }
  .status { margin: 8px 0 2px; font-size: 13px; }
  .status.err { color: var(--color-error); font-weight: 600; }
  .hint { margin: 2px 0 0; font-size: 11px; color: var(--color-slate); }
  table { border-collapse: collapse; font-size: 13px; width: 100%; }
  td { padding: 2px 10px 2px 0; }
  td.num { font-variant-numeric: tabular-nums; font-weight: 600; }
  td.warn { color: var(--color-error); font-weight: 700; }
  .dim { color: var(--color-slate); font-size: 12px; }
  .dot {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--color-error);
    margin-right: 4px;
  }
  .dot.ok { background: #0a9a4a; }
  .dot.off { background: var(--color-disabled, #bbb); }
  .foot p { margin: 0; font-size: 13px; }
</style>
