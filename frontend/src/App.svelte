<script lang="ts">
  import { onMount } from 'svelte'
  import ConnectionView from './lib/ConnectionView.svelte'
  import EcgView from './lib/EcgView.svelte'
  import HrvView from './lib/HrvView.svelte'
  import Landing from './lib/Landing.svelte'
  import type { EcgStreamMsg } from './lib/types'

  // Path-based routing. The app stays a single persistent-mounted SPA; the
  // route only selects which pane is visible, so live captures never tear down
  // when navigating between tools. (HR Compare ble fjernet 20.09.2026 - den
  // var et chat-1-verktoey for radiosammenligning og hadde utspilt rollen.
  // All øktstyring bor på TILKOBLING-fanen fra 20.09.2026.)
  type View = 'home' | 'conn' | 'ecg' | 'hrv'
  const PATH_TO_VIEW: Record<string, View> = {
    '/': 'home',
    '/tilkobling': 'conn',
    '/raw-ecg': 'ecg',
    '/rhythm-hrv': 'hrv',
  }
  const VIEW_TO_PATH: Record<View, string> = {
    home: '/',
    conn: '/tilkobling',
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

  let sources: Record<string, string> = $state({})
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
    } else if (m.t === 'ecg' || m.t === 'acc' || m.t === 'hr' || m.t === 'telemetry') {
      for (const fn of ecgSubs) (fn as (x: any) => void)(m)
    }
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
    <button class:active={view === 'conn'} onclick={() => go('conn')}>
      TILKOBLING
    </button>
    <button class:active={view === 'ecg'} onclick={() => go('ecg')}>
      RAW ECG
    </button>
    <button class:active={view === 'hrv'} onclick={() => go('hrv')}>
      RHYTHM / HRV
    </button>
  </div>
  <div class="controls">
    <span class="conn" class:up={wsUp}>
      {wsUp ? 'backend connected' : 'backend offline'}
    </span>
  </div>
</header>

<div class="pane" style:display={view === 'home' ? 'contents' : 'none'}>
  <Landing onopen={go} />
</div>
<div class="pane" style:display={view === 'conn' ? 'contents' : 'none'}>
  <ConnectionView
    sources={sources}
    send={sendCmd}
    register={registerEcg}
    onstatus={ecgStatusFor}
  />
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
</style>
