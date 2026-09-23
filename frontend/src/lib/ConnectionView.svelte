<script lang="ts">
  import { onMount } from 'svelte'

  // TILKOBLING: all øktstyring og tilkoblingsdiagnostikk samlet (20.09.2026).
  // Ett kort per kilde - designet for N kilder (dual-H10 kommer).
  type Status = { state: string; detail: string; device: string } | null
  interface Props {
    sources: Record<string, string>
    belts?: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: any) => void) => void
    onstatus: (source: string) => Status
  }
  let { sources, belts = {}, send, register, onstatus }: Props = $props()

  // Konsistent rekkefølge med RAW-fanene: belte A øverst, så B, C...;
  // kilder uten kjent belte etter transport (ESP32 først).
  function beltOf(id: string): string {
    void tick
    const dev = telemetry[id]?.device || onstatus(id)?.device || ''
    for (const [beltId, letter] of Object.entries(belts)) {
      if (dev.includes(beltId)) return letter
    }
    return ''
  }
  function orderKey(id: string): string {
    const b = beltOf(id)
    if (b) return '0' + b
    if (id.startsWith('esp32-')) return '1' + id
    if (id === 'raven:hci1') return '2' + id
    return '3' + id
  }
  const sourceIds = $derived(
    Object.keys(sources).sort((a, b) => orderKey(a).localeCompare(orderKey(b))),
  )
  const PLACEMENT: Record<string, string> = {
    A: 'øvre belte - senter ~5 cm under høyre brystvorte',
    B: 'nedre belte, rotert - senter ~8 cm under venstre brystvorte',
  }

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

  // Tilkoblingstrinn ("lamper"): utleder hvor i kjeden en kilde står, så det
  // er synlig hvor det evt. stopper. Trinn: bro online -> søker belte ->
  // kobler -> armerer -> strømmer. Hvert trinn er 'done' | 'active' | 'wait' | 'err'.
  const STEPS = [
    { key: 'bridge', label: 'ESP32-bro online' },
    { key: 'search', label: 'Søker belte' },
    { key: 'connect', label: 'Kobler til H10' },
    { key: 'warmup', label: 'Oppvarming' },
    { key: 'stream', label: 'Strømmer' },
  ]
  // Trinn-tilstand når ingen bro er tilkoblet ennå (tom-visning): vi leter
  // aktivt etter ESP32-broen, resten venter.
  const SEARCHING_BRIDGE: Record<string, string> = {
    bridge: 'active', search: 'wait', connect: 'wait', warmup: 'wait', stream: 'wait',
  }
  function steps(id: string, st: Status): Record<string, string> {
    void tick
    const tm = tele(id)
    const streaming = isStreaming(id) || st?.state === 'streaming'
    const anyFrame = streaming
    const s: Record<string, string> = {}
    // Bro online: telemetri mottatt (ESP32) eller kilden finnes (raven-agent).
    const bridgeUp = !!tm || sourceIds.includes(id)
    s.bridge = bridgeUp ? 'done' : 'wait'
    // BLE-tilstand fra telemetri (ESP32) eller status.
    const bleConn = tm?.ble_connected || st?.state === 'streaming'
    const scanning = st?.state === 'scanning' || (busy(id, st) && !bleConn && !streaming)
    const connecting = st?.state === 'connecting'
    const err = st?.state === 'error'

    if (streaming) {
      s.search = 'done'; s.connect = 'done'
      // Oppvarming: BLE oppe, men EKG/HR-rammer ikke begynt (H10 ~5-35 s).
      s.warmup = anyFrame ? 'done' : 'active'
      s.stream = anyFrame ? 'active' : 'wait'
    } else if (bleConn) {
      s.search = 'done'; s.connect = 'done'; s.warmup = 'active'; s.stream = 'wait'
    } else if (connecting) {
      s.search = 'done'; s.connect = err ? 'err' : 'active'; s.warmup = 'wait'; s.stream = 'wait'
    } else if (scanning) {
      s.search = err ? 'err' : 'active'; s.connect = 'wait'; s.warmup = 'wait'; s.stream = 'wait'
    } else {
      s.search = 'wait'; s.connect = 'wait'; s.warmup = 'wait'; s.stream = 'wait'
    }
    if (err && !connecting) s.search = 'err'
    return s
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
      <h2>Søker etter ESP32-broen …</h2>
      <ol class="steps">
        {#each STEPS as step (step.key)}
          <li class={SEARCHING_BRIDGE[step.key]}>
            <span class="lamp"></span>
            <span class="lbl">{step.label}</span>
          </li>
        {/each}
      </ol>
      <p>
        Broen kobler seg til backend via iPhone-hotspoten (WiFi må være på, og
        internettdeling-skjermen bør stå åpen). Så snart den melder seg, dukker
        kortet opp her. Raven-agenten kommer via USB-adapteren.
      </p>
    </section>
  {/if}

  {#each sourceIds as id (id)}
    {@const st = onstatus(id)}
    {@const tm = tele(id)}
    <section class="card">
      <header>
        {#if beltOf(id)}
          <span class="belt" title={PLACEMENT[beltOf(id)] ?? ''}>BELTE {beltOf(id)}</span>
        {/if}
        <h2>{friendlyLabel(id)}</h2>
        <code>{id}</code>
      </header>

      <div class="grid">
        <div class="block">
          <h3>Økt</h3>
          <div class="controls">
            <select bind:value={modes[id]}>
              <option value="hrv">hrv (anbefalt)</option>
              <option value="ecg">ecg</option>
              <option value="hr">hr</option>
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
          {#each [steps(id, st)] as sp}
            <ol class="steps">
              {#each STEPS as step (step.key)}
                <li class={sp[step.key]}>
                  <span class="lamp"></span>
                  <span class="lbl">{step.key === 'bridge' && !id.startsWith('esp32-') ? 'Agent online' : step.label}</span>
                </li>
              {/each}
            </ol>
            {#if sp.search === 'active'}
              <p class="hint">Søker etter beltet - ta på H10 med fuktede elektroder hvis det ikke dukker opp.</p>
            {:else if sp.warmup === 'active'}
              <p class="hint">Tilkoblet - H10 varmer opp (~5-35 s før første EKG).</p>
            {:else}
              <p class="hint">Arbitrering per belte: kilder låst til hvert sitt belte (dual-H10) strømmer samtidig; ellers vinner nyeste start (H10 = én sentral).</p>
            {/if}
          {/each}
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
              <tr><td title="DIS-firmwarestreng fra beltet; kan avvike fra versjonen Polar Flow viser">Firmware (DIS)</td><td>{tm?.fw || '-'}</td></tr>
              <tr><td>Batteri</td><td>{tm && tm.battery >= 0 ? tm.battery + ' %' : '-'}</td></tr>
              <tr><td>Hudkontakt</td><td>{tm?.contact === 'yes' ? 'ja' : tm?.contact === 'no' ? 'nei (tas av?)' : '-'}</td></tr>
              <tr><td>BLE-link</td><td>{tm ? (tm.ble_connected ? 'tilkoblet' : 'frakoblet') : '-'}</td></tr>
              <tr><td>BLE-RSSI</td><td>{tm?.ble_connected && tm.ble_rssi ? tm.ble_rssi + ' dBm' : '-'}</td></tr>
            </tbody>
          </table>
        </div>

        <div class="block">
          <h3>Bro / agent</h3>
          {#if tm && tm.uptime_s !== undefined}
            <table>
              <tbody>
                <tr><td>WiFi-RSSI</td><td>{tm.wifi_rssi} dBm</td></tr>
                <tr><td>Chip-temp</td><td class:warn={tm.chip_c > 75}>{tm.chip_c > 0 ? tm.chip_c.toFixed(0) + ' °C' : '-'}</td></tr>
                <tr><td>Fritt minne</td><td>{tm.heap_kb} kB</td></tr>
                <tr><td>microSD</td><td>{tm.sd ? 'montert (spill aktiv)' : 'ikke montert'}</td></tr>
                <tr><td>Klokke</td><td>{tm.clock === 'ntp-synced' ? 'NTP-synket (ekte tid)' : tm.clock === 'unsynced' ? 'usynket (oppetid)' : '-'}</td></tr>
                <tr><td>Oppetid</td><td>{Math.floor(tm.uptime_s / 60)} min</td></tr>
              </tbody>
            </table>
          {:else if tm}
            <p class="dim">Raven-agent (USB-adapter) - sender belte-telemetri (hudkontakt/batteri), ingen bro-telemetri.</p>
          {:else}
            <p class="dim">Ingen telemetri mottatt ennå (kommer når en økt strømmer; ESP32-broen sender hvert 5. sekund).</p>
          {/if}
        </div>
      </div>
    </section>
  {/each}

  <section class="card foot">
    <p>
      Visningene <b>RAW ECG</b>, <b>RAW ACC</b> og <b>SYNTETISK EKG</b> følger
      automatisk øktene som startes her (to strimler ved dual-H10, belte A
      alltid øverst). Bruk <b>hrv</b>-modus for å mate alle (EKG + ACC + nativ
      HR/RR). H10 trenger ~5–35 s oppvarming før første EKG-ramme.
    </p>
    <p>
      <b>Beltepark og plassering:</b> Belte A = 0B052A39, <i>øvre belte, senter
      ~5 cm under høyre brystvorte</i>. Belte B = 1DA2053E, <i>nedre belte,
      rotert, senter ~8 cm under venstre brystvorte</i>.
      <b>Oppstart fra scratch: start belte B (BT-600) først, deretter
      ESP32</b> - broen mangler foreløpig beltefilter i firmware og tar første
      belte den ser.
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
  .belt {
    font-family: var(--font-display);
    font-size: 11px;
    letter-spacing: 1.5px;
    color: #fff;
    background: var(--color-slate, #555);
    padding: 2px 8px;
    border-radius: 5px;
    cursor: help;
  }
  .grid {
    display: grid;
    /* auto-fill + romslig minimum: blokker bryter til ny rad i stedet for å
       klemmes/overlappe (sett 20.09: knapp under strømmer-tabellen). */
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 18px 26px;
  }
  .block { min-width: 0; }
  .controls { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  select {
    padding: 5px 8px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-bg);
    font-family: var(--font-body);
    min-width: 0;
    max-width: 100%;
  }
  button.start, button.stop {
    border: 0;
    border-radius: 8px;
    padding: 6px 20px;
    cursor: pointer;
    color: #fff;
    font-family: var(--font-display);
    letter-spacing: 1.5px;
    font-size: 13px;
  }
  /* Eksplisitt fokus-stil: unngå nettleserens grå fokusprikk over teksten. */
  button.start:focus-visible, button.stop:focus-visible {
    outline: 2px solid var(--color-ink, #111);
    outline-offset: 2px;
  }
  button.start:focus, button.stop:focus { outline: none; }
  button.start { background: #0a9a4a; }
  button.stop { background: var(--color-heart, #cb333b); }

  .steps {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 12px 0 6px;
    padding: 0;
  }
  .steps li {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--color-slate);
  }
  .steps .lamp {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--color-disabled, #cfcfcf);
    flex: none;
    transition: background 0.2s;
  }
  .steps li.done .lamp { background: #0a9a4a; }
  .steps li.done .lbl { color: var(--color-ink, #222); }
  .steps li.active .lamp { background: #e8a41c; animation: pulse 1s ease-in-out infinite; }
  .steps li.active .lbl { color: var(--color-ink, #222); font-weight: 600; }
  .steps li.err .lamp { background: var(--color-heart, #cb333b); }
  .steps li.err .lbl { color: var(--color-heart, #cb333b); }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.35; } }
  .status { margin: 8px 0 2px; font-size: 13px; }
  .status.err { color: var(--color-error); font-weight: 600; }
  .hint { margin: 2px 0 0; font-size: 11px; color: var(--color-slate); }
  table { border-collapse: collapse; font-size: 13px; width: 100%; }
  td { padding: 2px 10px 2px 0; white-space: nowrap; }
  td:last-child { white-space: normal; }
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
