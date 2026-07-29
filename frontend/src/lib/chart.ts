import type { Sample } from './types'

const BPM_MIN = 40
const BPM_MAX = 180
const GRID_LINES = [60, 100, 140]

export interface ChartOpts {
  frozen: boolean
  windowMs?: number
}

export function drawChart(
  canvas: HTMLCanvasElement,
  samples: Sample[],
  opts: ChartOpts,
): void {
  const dpr = window.devicePixelRatio || 1
  const w = canvas.clientWidth
  const h = canvas.clientHeight
  if (!w || !h) return
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr)
    canvas.height = Math.round(h * dpr)
  }
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, w, h)

  const yFor = (bpm: number) => {
    const clamped = Math.min(BPM_MAX, Math.max(BPM_MIN, bpm))
    return h - ((clamped - BPM_MIN) / (BPM_MAX - BPM_MIN)) * h
  }

  ctx.strokeStyle = '#e3e0d8'
  ctx.fillStyle = '#959593'
  ctx.font = '10px "Open Sans", sans-serif'
  ctx.lineWidth = 1
  for (const g of GRID_LINES) {
    const y = yFor(g)
    ctx.beginPath()
    ctx.moveTo(0, y)
    ctx.lineTo(w, y)
    ctx.stroke()
    ctx.fillText(String(g), 4, y - 3)
  }

  if (!samples.length) return

  const windowMs = opts.windowMs ?? 30000
  const lastT = samples[samples.length - 1].t
  let t0 = 0
  let t1: number
  if (opts.frozen) {
    t1 = Math.max(lastT, 1000)
  } else if (lastT < windowMs) {
    t1 = windowMs
  } else {
    t0 = lastT - windowMs
    t1 = lastT
  }
  const xFor = (t: number) => ((t - t0) / (t1 - t0)) * w

  ctx.strokeStyle = '#CB333B'
  ctx.lineWidth = 2
  ctx.lineJoin = 'round'
  ctx.beginPath()
  let started = false
  for (const s of samples) {
    if (s.t < t0) continue
    const x = xFor(s.t)
    const y = yFor(s.bpm)
    if (!started) {
      ctx.moveTo(x, y)
      started = true
    } else {
      ctx.lineTo(x, y)
    }
  }
  ctx.stroke()

  const last = samples[samples.length - 1]
  ctx.fillStyle = '#CB333B'
  ctx.beginPath()
  ctx.arc(xFor(last.t), yFor(last.bpm), 3.5, 0, Math.PI * 2)
  ctx.fill()
}

