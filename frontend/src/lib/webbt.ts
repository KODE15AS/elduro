import type { CaptureCallbacks, Sample } from './types'

/**
 * Captures the standard BLE Heart Rate Profile directly in the browser
 * via the Web Bluetooth API (Chrome/Edge, secure context required).
 */
export class WebBTCapture {
  private device: any = null
  private t0 = 0
  private cb: CaptureCallbacks | null = null
  private active = false

  async start(cb: CaptureCallbacks): Promise<void> {
    this.cb = cb
    const bt = (navigator as any).bluetooth
    if (!bt) {
      cb.onstatus(
        'error',
        'Web Bluetooth is not available - use Chrome/Edge over HTTPS',
      )
      return
    }
    try {
      cb.onstatus('scanning', 'pick your H10 in the browser dialog')
      this.device = await bt.requestDevice({
        filters: [{ services: ['heart_rate'] }],
        optionalServices: ['battery_service'],
      })
      cb.onstatus('connecting', '', { device: this.device.name })
      const gatt = await this.device.gatt.connect()
      this.device.addEventListener('gattserverdisconnected', () => {
        if (this.active) this.finish('disconnected')
      })

      let battery: number | null = null
      try {
        const bs = await gatt.getPrimaryService('battery_service')
        const bc = await bs.getCharacteristic('battery_level')
        battery = (await bc.readValue()).getUint8(0)
      } catch {
        // battery service is optional
      }

      const hs = await gatt.getPrimaryService('heart_rate')
      const hrm = await hs.getCharacteristic('heart_rate_measurement')
      this.t0 = performance.now()
      hrm.addEventListener('characteristicvaluechanged', (e: any) => {
        const sample = parseHr(e.target.value, performance.now() - this.t0)
        this.cb?.onsample(sample)
      })
      await hrm.startNotifications()
      this.active = true
      cb.onstatus('streaming', '', { device: this.device.name, battery })
    } catch (err: any) {
      cb.onstatus('error', err?.message ?? String(err))
      this.device = null
    }
  }

  stop(reason: string): void {
    this.finish(reason)
  }

  private finish(reason: string): void {
    if (!this.active && !this.device) return
    this.active = false
    try {
      if (this.device?.gatt?.connected) this.device.gatt.disconnect()
    } catch {
      // already gone
    }
    this.device = null
    this.cb?.onstatus('stopped', reason)
  }
}

function parseHr(dv: DataView, t: number): Sample {
  const flags = dv.getUint8(0)
  let i = 1
  let bpm: number
  if (flags & 0x01) {
    bpm = dv.getUint16(i, true)
    i += 2
  } else {
    bpm = dv.getUint8(i)
    i += 1
  }
  if (flags & 0x08) i += 2 // skip energy expended
  const rr: number[] = []
  if (flags & 0x10) {
    while (i + 2 <= dv.byteLength) {
      rr.push(Math.round((dv.getUint16(i, true) * 1000) / 1024))
      i += 2
    }
  }
  return { t, bpm, rr }
}

