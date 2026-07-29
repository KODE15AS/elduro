<script lang="ts">
  import { drawChart } from './chart'
  import type { LaneData } from './types'

  interface Props {
    num: number
    title: string
    lane: LaneData
    available: boolean
    locked: boolean
    onrecord: () => void
    onstop: () => void
  }
  let { num, title, lane, available, locked, onrecord, onstop }: Props = $props()

  let canvas: HTMLCanvasElement | undefined = $state()

  const recording = $derived(
    ['scanning', 'connecting', 'streaming'].includes(lane.status),
  )
  const lastSample = $derived(
    lane.samples.length ? lane.samples[lane.samples.length - 1] : null,
  )
  const lastRr = $derived(
    lastSample && lastSample.rr.length
      ? lastSample.rr[lastSample.rr.length - 1]
      : null,
  )
  const statusText = $derived(
    !available
      ? 'not connected'
      : lane.status + (lane.detail ? ' - ' + lane.detail : ''),
  )

  $effect(() => {
    // Track sample count and status so the chart redraws on every packet.
    void lane.samples.length
    void lane.status
    if (canvas) drawChart(canvas, lane.samples, { frozen: !recording })
  })

  $effect(() => {
    if (!canvas) return
    const ro = new ResizeObserver(() => {
      if (canvas) drawChart(canvas, lane.samples, { frozen: !recording })
    })
    ro.observe(canvas)
    return () => ro.disconnect()
  })
</script>

<section class="lane" class:recording class:unavailable={!available}>
  <div class="info">
    <div class="top">
      <span class="num">{num}</span>
      <h2>{title}</h2>
    </div>
    <div class="status" class:error={lane.status === 'error'}>
      {statusText}{lane.device ? ' - ' + lane.device : ''}
    </div>
    {#if !available && num === 2}
      <div class="setup">
        <a href="/dl/elduro-capture.exe" download>Download the Windows agent</a>
        and run it:
        <code>elduro-capture.exe --backend ws://100.65.19.39:8094/ws/agent --agent lenovo</code>
      </div>
    {/if}
    <div class="readout">
      <span class="heart" class:beat={lane.status === 'streaming'}>&hearts;</span>
      <span class="bpm">{lastSample ? lastSample.bpm : '--'}</span>
      <span class="unit">BPM</span>
    </div>
    <div class="small">
      R-R {lastRr ?? '--'} ms &middot; Battery {lane.battery ?? '--'}%
    </div>
    {#if lane.metrics}
      <div class="metrics">
        {lane.metrics.packets} pkt / {lane.metrics.durationS}s
        &middot; {lane.metrics.meanBpm} bpm avg ({lane.metrics.minBpm}-{lane.metrics.maxBpm})
        &middot; interval {lane.metrics.meanIntervalMs} ms
        &middot; jitter {lane.metrics.jitterMs} ms
        &middot; max gap {lane.metrics.maxGapMs} ms
        &middot; drops {lane.metrics.drops}
        &middot; {lane.metrics.rrCount} R-R
      </div>
    {/if}
    {#if recording}
      <button class="stop" onclick={onstop}>STOP</button>
    {:else}
      <button class="rec" disabled={!available || locked} onclick={onrecord}>
        RECORD
      </button>
    {/if}
  </div>
  <div class="chart">
    <canvas bind:this={canvas}></canvas>
  </div>
</section>

<style>
  .lane {
    display: flex;
    gap: 14px;
    background: var(--color-card);
    border: 1px solid var(--color-line);
    border-radius: var(--radius);
    padding: 12px 16px;
    min-height: 0;
    overflow: hidden;
  }
  .lane.recording {
    border-color: var(--color-heart);
  }
  .lane.unavailable {
    opacity: 0.55;
  }
  .info {
    width: 300px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-height: 0;
  }
  .top {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .num {
    font-family: var(--font-display);
    font-size: 20px;
    color: var(--color-accent);
  }
  h2 {
    font-family: var(--font-display);
    font-size: 17px;
    letter-spacing: 1.5px;
    margin: 0;
    font-weight: 400;
  }
  .status {
    font-size: 11px;
    color: var(--color-slate);
    min-height: 14px;
    text-transform: lowercase;
  }
  .status.error {
    color: var(--color-error);
  }
  .setup {
    font-size: 11px;
    color: var(--color-slate);
    line-height: 1.5;
  }
  .setup a {
    color: var(--color-accent);
    font-weight: 600;
  }
  .setup code {
    display: block;
    font-size: 10px;
    background: var(--color-bg);
    border-radius: 6px;
    padding: 3px 6px;
    user-select: all;
  }
  .readout {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .heart {
    color: var(--color-heart);
    font-size: 22px;
  }
  .heart.beat {
    animation: beat 1s ease-in-out infinite;
    display: inline-block;
  }
  @keyframes beat {
    0%, 100% { transform: scale(1); }
    15% { transform: scale(1.35); }
    30% { transform: scale(1); }
  }
  .bpm {
    font-family: var(--font-display);
    font-size: 42px;
    line-height: 1;
    color: var(--color-heart);
  }
  .unit {
    font-size: 12px;
    color: var(--color-slate);
  }
  .small {
    font-size: 12px;
    color: var(--color-slate);
  }
  .metrics {
    font-size: 10.5px;
    color: var(--color-accent-muted);
    line-height: 1.5;
  }
  button {
    margin-top: auto;
    align-self: flex-start;
    font-family: var(--font-display);
    font-size: 16px;
    letter-spacing: 2px;
    padding: 6px 22px;
    border: none;
    border-radius: 10px;
    cursor: pointer;
    color: #fff;
  }
  button.rec {
    background: var(--color-accent);
  }
  button.rec:hover:not(:disabled) {
    background: var(--color-slate);
  }
  button.rec:disabled {
    background: var(--color-disabled);
    cursor: not-allowed;
  }
  button.stop {
    background: var(--color-heart);
  }
  .chart {
    flex: 1;
    min-width: 0;
    position: relative;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>

