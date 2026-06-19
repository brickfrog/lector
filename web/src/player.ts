// Gapless continuous read-along via the Web Audio API.
//
// Instead of swapping an <audio>.src per chunk (each swap costs a load/seam), we
// decode each chunk's PCM into an AudioBuffer and schedule them back-to-back on
// the AudioContext clock, so short chunks stitch sample-accurately. Synth is
// serialized (single ort session) and we render a few chunks ahead so playback
// absorbs synth jitter; if synth can't keep up the timeline simply stalls and
// resumes when the next chunk lands. The word highlight is driven off the audio
// clock, not wall-clock, so it stays locked to what you hear.

import * as lector from "./core/lector.js"
import { init, synth, type Progress } from "./bridge.js"

export interface Rendered {
  buffer: AudioBuffer
  timed: { text: string; start_s: number; end_s: number }[]
}

export interface ReaderHooks {
  onChunk?: (idx: number, total: number, r: Rendered) => void
  onWord?: (activeIdx: number) => void
  onStatus?: (msg: string) => void
  onProgress?: (p: Progress) => void
  onState?: (playing: boolean) => void
  onEnd?: () => void
  onError?: (e: Error) => void
}

const LOOKAHEAD = 4 // render this many chunks beyond the one scheduled

export class Reader {
  idx = 0
  private ctx = new AudioContext()
  private rendered = new Map<number, Rendered>()
  private pending = new Map<number, Promise<Rendered>>()
  private lock: Promise<unknown> = Promise.resolve()
  private started = false // model initialized
  private playing = false // user intends playback
  private scheduledIdx = -1
  private nextStart = 0
  private timeline: { idx: number; start: number; end: number; src: AudioBufferSourceNode }[] = []
  private displayedIdx = -1
  private rafId = 0

  constructor(
    private chunks: string[],
    private voice: string,
    private lang: "a" | "b",
    private hooks: ReaderHooks = {},
  ) {}

  get total() {
    return this.chunks.length
  }
  get paused() {
    return this.ctx.state === "suspended" || !this.playing
  }

  // ---- synth + cache (serialized through a lock, deduped) ----
  private async doSynth(i: number, withProgress: boolean): Promise<Rendered> {
    const r = await synth(
      this.chunks[i],
      this.voice,
      this.lang,
      withProgress ? (p) => this.hooks.onProgress?.(p) : undefined,
    )
    const timed = lector.align_words(r.words, r.wordPhonemes, r.phonemes, r.duration)
    const buffer = this.ctx.createBuffer(1, r.waveform.length, r.sampleRate)
    buffer.getChannelData(0).set(r.waveform)
    return { buffer, timed }
  }

  private render(i: number, withProgress = false): Promise<Rendered> {
    const hit = this.rendered.get(i)
    if (hit) return Promise.resolve(hit)
    const inflight = this.pending.get(i)
    if (inflight) return inflight
    const run = this.lock.then(() => this.doSynth(i, withProgress))
    this.lock = run.catch(() => {})
    this.pending.set(i, run)
    run.then(
      (r) => {
        this.rendered.set(i, r)
        this.pending.delete(i)
        this.pump() // a freshly-ready chunk may unblock the timeline
      },
      (e) => {
        this.pending.delete(i)
        this.hooks.onError?.(e as Error)
      },
    )
    return run
  }

  private ensureRenders() {
    const front = this.scheduledIdx + 1
    for (let k = front; k <= front + LOOKAHEAD && k < this.chunks.length; k++) {
      if (!this.rendered.has(k) && !this.pending.has(k)) {
        // progress UI only for the chunk we're actually waiting on to start
        this.render(k, this.timeline.length === 0 && k === front)
      }
    }
  }

  private evict() {
    for (const k of this.rendered.keys()) {
      if (k < this.displayedIdx - 1) this.rendered.delete(k)
    }
    this.timeline = this.timeline.filter((e) => e.end >= this.ctx.currentTime - 1)
  }

  // ---- scheduling: append every ready, not-yet-scheduled chunk to the clock ----
  private pump() {
    if (!this.playing) return
    while (this.scheduledIdx + 1 < this.chunks.length) {
      const next = this.scheduledIdx + 1
      const r = this.rendered.get(next)
      if (!r) {
        if (this.timeline.length) this.hooks.onStatus?.("buffering…")
        this.ensureRenders()
        return
      }
      const now = this.ctx.currentTime
      const startAt = this.nextStart > now ? this.nextStart : now + 0.03
      const src = this.ctx.createBufferSource()
      src.buffer = r.buffer
      src.connect(this.ctx.destination)
      src.start(startAt)
      const end = startAt + r.buffer.duration
      this.timeline.push({ idx: next, start: startAt, end, src })
      this.nextStart = end
      this.scheduledIdx = next
      const isLast = next === this.chunks.length - 1
      src.onended = () => {
        if (isLast && this.scheduledIdx === this.chunks.length - 1) this.finish()
      }
    }
    this.ensureRenders()
  }

  private loop = () => {
    const t = this.ctx.currentTime
    const cur = this.timeline.find((e) => t >= e.start && t < e.end)
    if (cur) {
      if (cur.idx !== this.displayedIdx) {
        this.displayedIdx = cur.idx
        this.idx = cur.idx
        const r = this.rendered.get(cur.idx)
        if (r) this.hooks.onChunk?.(cur.idx, this.chunks.length, r)
        this.hooks.onStatus?.("▶ reading…")
        this.evict()
        this.ensureRenders()
      }
      const r = this.rendered.get(cur.idx)
      if (r) {
        const local = t - cur.start
        let active = -1
        for (let i = 0; i < r.timed.length; i++) {
          if (local >= r.timed[i].start_s) active = i
          else break
        }
        this.hooks.onWord?.(active)
      }
    }
    this.rafId = requestAnimationFrame(this.loop)
  }

  private stopScheduled() {
    for (const e of this.timeline) {
      try {
        e.src.onended = null
        e.src.stop()
      } catch {
        /* already stopped */
      }
    }
    this.timeline = []
    this.scheduledIdx = this.idx - 1
    this.nextStart = 0
  }

  private finish() {
    this.playing = false
    this.hooks.onState?.(false)
    this.hooks.onEnd?.()
  }

  // ---- public controls ----
  async playFrom(i: number) {
    if (i < 0 || i >= this.chunks.length) return
    try {
      // resume on the user gesture FIRST: a long model download before resume()
      // would let the activation lapse and the browser would block audio.
      await this.ctx.resume()
      if (!this.started) {
        this.hooks.onStatus?.("loading model…")
        await init((p) => this.hooks.onProgress?.(p))
        this.started = true
      }
      this.stopScheduled()
      this.idx = i
      this.displayedIdx = -1
      this.scheduledIdx = i - 1
      this.nextStart = 0
      this.playing = true
      this.hooks.onState?.(true)
      if (!this.rafId) this.rafId = requestAnimationFrame(this.loop)
      this.ensureRenders()
      this.pump()
    } catch (e) {
      this.hooks.onError?.(e as Error)
    }
  }

  async pause() {
    await this.ctx.suspend()
    this.hooks.onState?.(false)
  }

  async resume() {
    if (!this.started) return this.playFrom(this.idx)
    this.playing = true
    await this.ctx.resume()
    this.hooks.onState?.(true)
    this.pump()
  }

  next() {
    if (this.idx + 1 < this.chunks.length) this.playFrom(this.idx + 1)
    else this.finish()
  }

  prev() {
    this.playFrom(Math.max(0, this.idx - 1))
  }

  dispose() {
    this.playing = false
    cancelAnimationFrame(this.rafId)
    this.stopScheduled()
    this.ctx.close().catch(() => {})
  }
}
