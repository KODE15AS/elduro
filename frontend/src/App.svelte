<script lang="ts">
  import { onMount } from 'svelte'
  import Lane from './lib/Lane.svelte'
  import EcgView from './lib/EcgView.svelte'
  import HrvView from './lib/HrvView.svelte'
  import Landing from './lib/Landing.svelte'
  import { computeMetrics, emptyLane } from './lib/metrics'
  import type { LaneData, EcgStreamMsg } from './lib/types'

  // Path-based routing. The app stays a single persistent-mounted SPA; the
  // route only selects which pane is visible, so live captures never tear down
  // when navigating between tools.
  type View = 'home' | 'hr' | 'ecg' | 'hrv'
  const PATH_TO_VIEW: Record<string, View> = {
    '/': 'home',
    '/hr-compare': 'hr',
    '/raw-ecg': 'ecg',
    '/rhythm-hrv': 'hrv',
  }
  const VIEW_TO_PATH: Record<View, string> = {
    home: '/',
    hr: '/hr-compare',
    ecg: '/raw-ecg',
    hrv: '/rhythm-hrv',
  }
  function viewFromPath(): View {
    return PATH_TO_VIEW[location.pathname] ?? 'home'
  }
  function go(v: View) {
    view = v
    const p = VIEW_TO_PATH[v]
    if (location.pathname !== p) history.pushState({}, '', p)
  }

  const LANE_DEFS = [
    { key: 'ravenUsb', num: 1, title: 'RAVEN - ASUS BT-600 USB' },
    { key: 'lenovo', num: 2, title: 'LENOVO - NATIVE AGENT' },
  ]

  let sources: Record<string, string> = $state({})
  let lanes: Record<string, LaneData> = $state({
    ravenUsb: emptyLane(),
    lenovo: emptyLane(),
  })
  let activeLane: string | null = $state(null)
  let durationS = $state(60)
  let wsUp = $state(false)
  let view: View = $state(viewFromPath())

  // Phase 2 ECG plumbing: status per source and live-frame subscribers.
  let ecgStatus: Record<string, { state: string; detail: string; device: string }> = $state({})
  const ecgSubs: ((m: EcgStreamMsg) => void)[] = []
  function registerEcg(fn: (m: EcgStreamMsg) => void) {
    ecgSubs.push(fn)
  }
  function sendCmd(cmd: object) {
    ws?.send(JSON.stringify(cmd))
  }
  function ecgStatusFor(source: string) {
    return ecgStatus[source] ?? null
  }

  let ws: WebSocket | null = null
  let closed = false

  const RECORDING_STATES = ['scanning', 'connecting', 'streaming']

  function laneSource(key: string): string | null {
    const ids = Object.keys(sources)
    switch (key) {
      case 'lenovo':
        // Any non-raven agent (e.g. the laptop, whatever its hostname is).
        return ids.find((s) => !s.startsWith('raven:')) ?? null
      case 'ravenUsb':
        return ids.find((s) => s.startsWith('raven:') && s !== 'raven:hci0') ?? null
    }
    return null
  }

  function sourceLane(source: string): string | null {
    for (const d of LANE_DEFS) {
      if (laneSource(d.key) === source) return d.key
    }
    return null
  }

  onMount(() => {
    connectWs()
    const onPop = () => {
      view = viewFromPath()
    }
    window.addEventListener('popstate', onPop)
    return () => {
      closed = true
      ws?.close()
      window.removeEventListener('popstate', onPop)
    }
  })

  function connectWs() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const s = new WebSocket(proto + '://' + location.host + '/ws/ui')
    s.onopen = () => {
      wsUp = true
    }
    s.onclose = () => {
      wsUp = false
      if (!closed) setTimeout(connectWs, 3000)
    }
    s.onmessage = (ev) => {
      try {
        handleMsg(JSON.parse(ev.data))
      } catch {
        // ignore malformed messages
      }
    }
    ws = s
  }

  function handleMsg(m: any) {
    if (m.t === 'sources') {
      const next: Record<string, string> = {}
      for (const s of m.sources) next[s.id] = s.label
      sources = next
    } else if (m.t === 'status') {
      ecgStatus[m.source] = {
        state: m.state ?? '',
        detail: m.detail ?? '',
        device: m.device ?? '',
      }
      const key = sourceLane(m.source)
      if (key) applyStatus(key, m)
    } else if (m.t === 'ecg' || m.t === 'acc') {
      for (const fn of ecgSubs) fn(m)
    } else if (m.t === 'hr') {
      const key = sourceLane(m.source)
      if (key) {
        lanes[key].samples.push({ t: m.ts, bpm: m.bpm, rr: m.rr ?? [] })
      }
      for (const fn of ecgSubs) (fn as (x: any) => void)(m)
    }
  }

  function applyStatus(key: string, m: any) {
    const lane = lanes[key]
    if (m.battery != null) lane.battery = m.battery
    if (m.device) lane.device = m.device
    if (m.state === 'stopped') {
      if (RECORDING_STATES.includes(lane.status)) {
        lane.detail = m.detail ?? ''
        finishLane(key)
      }
    } else if (m.state === 'error') {
      lane.detail = m.detail ?? ''
      lane.status = 'error'
      if (activeLane === key) activeLane = null
    } else if (RECORDING_STATES.includes(m.state)) {
      lane.detail = m.detail ?? ''
      lane.status = m.state
    }
  }

  function finishLane(key: string) {
    const lane = lanes[key]
    lane.metrics = computeMetrics(lane.samples)
    lane.status = lane.samples.length ? 'done' : 'idle'
    if (activeLane === key) activeLane = null
  }

  function record(key: string) {
    if (activeLane) return
    const src = laneSource(key)
    if (!src) return
    const lane = lanes[key]
    lane.samples = []
    lane.metrics = null
    lane.battery = null
    lane.device = ''
    lane.detail = ''
    lane.status = 'scanning'
    activeLane = key
    ws?.send(JSON.stringify({ t: 'start', source: src, duration_s: durationS }))
  }

  function stop(key: string) {
    const src = laneSource(key)
    if (src) ws?.send(JSON.stringify({ t: 'stop', source: src }))
  }
</script>

<header>
  <div
    class="brand"
    role="button"
    tabindex="0"
    onclick={() => go('home')}
    onkeydown={(e) => {
      if (e.key === 'Enter' || e.key === ' ') go('home')
    }}
  >
    ELDURO <span class="heart">&hearts;</span>
  </div>
  <div class="subtitle">POLAR H10 SIGNAL LAB</div>
  <div class="tabs">
    <button class:active={view === 'hr'} onclick={() => go('hr')}>
      HR COMPARE
    </button>
    <button class:active={view === 'ecg'} onclick={() => go('ecg')}>
      RAW ECG
    </button>
    <button class:active={view === 'hrv'} onclick={() => go('hrv')}>
      RHYTHM / HRV
    </button>
  </div>
  <div class="controls">
    {#if view === 'hr'}
      <label>
        Window
        <select bind:value={durationS}>
          <option value={30}>30 s</option>
          <option value={60}>60 s</option>
          <option value={120}>120 s</option>
          <option value={0}>manual</option>
        </select>
      </label>
    {/if}
    <span class="conn" class:up={wsUp}>
      {wsUp ? 'backend connected' : 'backend offline'}
    </span>
  </div>
</header>

<div class="pane" style:display={view === 'home' ? 'contents' : 'none'}>
  <Landing onopen={go} />
</div>
<div class="pane" style:display={view === 'hr' ? 'contents' : 'none'}>
  <main>
    {#each LANE_DEFS as d (d.key)}
      <Lane
        num={d.num}
        title={d.title}
        lane={lanes[d.key]}
        available={laneSource(d.key) !== null}
        locked={activeLane !== null && activeLane !== d.key}
        onrecord={() => record(d.key)}
        onstop={() => stop(d.key)}
      />
    {/each}
  </main>
</div>
<div class="pane" style:display={view === 'ecg' ? 'contents' : 'none'}>
  <EcgView
    sources={sources}
    send={sendCmd}
    register={registerEcg}
    onstatus={ecgStatusFor}
  />
</div>
<div class="pane" style:display={view === 'hrv' ? 'contents' : 'none'}>
  <HrvView
    src="/sample-hrv.json"
    sources={sources}
    send={sendCmd}
    register={registerEcg}
    onstatus={ecgStatusFor}
  />
</div>

<style>
  header {
    height: 58px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 0 18px;
    border-bottom: 1px solid var(--color-line);
    background: var(--color-card);
  }
  .brand {
    font-family: var(--font-display);
    font-size: 30px;
    letter-spacing: 2px;
    cursor: pointer;
  }
  .brand .heart {
    color: var(--color-heart);
  }
  .subtitle {
    font-family: var(--font-display);
    font-size: 15px;
    letter-spacing: 2px;
    color: var(--color-slate);
  }
  .tabs {
    display: flex;
    gap: 6px;
    flex: 1;
    margin-left: 10px;
  }
  .tabs button {
    font-family: var(--font-display);
    font-size: 14px;
    letter-spacing: 1.5px;
    padding: 5px 14px;
    border: 1px solid var(--color-line);
    border-radius: 8px;
    background: var(--color-bg);
    color: var(--color-slate);
    cursor: pointer;
  }
  .tabs button.active {
    background: var(--color-accent);
    color: #fff;
    border-color: var(--color-accent);
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 14px;
    font-size: 13px;
  }
  .controls select {
    margin-left: 6px;
    font-family: var(--font-body);
    padding: 3px 6px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-bg);
  }
  .conn {
    color: var(--color-error);
    font-weight: 600;
  }
  .conn.up {
    color: var(--color-slate);
    font-weight: 400;
  }
  .conn.up::before,
  .conn::before {
    content: '';
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-right: 6px;
    background: currentColor;
  }
  main {
    height: calc(100vh - 58px);
    display: grid;
    grid-auto-rows: 1fr;
    gap: 10px;
    padding: 10px 14px 14px;
    box-sizing: border-box;
  }
</style>
