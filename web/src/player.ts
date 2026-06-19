// Continuous read-along player with render-ahead.
//
// The worker holds a single ort session, so synth must be serialized (concurrent
// runs throw "Session already started"). We therefore queue renders through a
// lock, cache a small window around the current chunk, and prefetch exactly one
// chunk ahead: synth (~16s) overlaps the current chunk's playback (~similar), so
// after the first chunk playback stays gapless without unbounded memory use.

import * as lector from "./core/lector.js"
import { init, synth, type Progress } from "./bridge.js"
import { encodeWav } from "./wav.js"

export interface Rendered {
  url: string
  timed: { text: string; start_s: number; end_s: number }[]
}

export interface ReaderHooks {
  onChunk?: (idx: number, total: number, r: Rendered) => void
  onStatus?: (msg: string) => void
  onProgress?: (p: Progress) => void
  onEnd?: () => void
  onError?: (e: Error) => void
}

export class Reader {
  idx = 0
  private cache = new Map<number, Rendered>()
  private pending = new Map<number, Promise<Rendered>>()
  private lock: Promise<unknown> = Promise.resolve()
  private started = false

  constructor(
    private chunks: string[],
    private audio: HTMLAudioElement,
    private voice: string,
    private lang: "a" | "b",
    private hooks: ReaderHooks = {},
  ) {
    // NB: the "ended" -> next() wiring lives in main.ts against the *current*
    // player, so a replaced Reader (new document) can't keep driving the audio.
  }

  get total() {
    return this.chunks.length
  }

  private async doSynth(i: number, withProgress: boolean): Promise<Rendered> {
    const r = await synth(
      this.chunks[i],
      this.voice,
      this.lang,
      withProgress ? (p) => this.hooks.onProgress?.(p) : undefined,
    )
    const timed = lector.align_words(r.words, r.wordPhonemes, r.phonemes, r.duration)
    const url = URL.createObjectURL(encodeWav(r.waveform, r.sampleRate))
    return { url, timed }
  }

  // serialized + deduped render with caching
  private render(i: number, withProgress = false): Promise<Rendered> {
    const hit = this.cache.get(i)
    if (hit) return Promise.resolve(hit)
    const inflight = this.pending.get(i)
    if (inflight) return inflight
    const run = this.lock.then(() => this.doSynth(i, withProgress))
    this.lock = run.catch(() => {})
    this.pending.set(i, run)
    run.then(
      (r) => {
        this.cache.set(i, r)
        this.pending.delete(i)
        this.evict()
      },
      () => this.pending.delete(i),
    )
    return run
  }

  // keep only [idx-1 .. idx+2]; revoke evicted blob URLs
  private evict() {
    for (const [k, r] of this.cache) {
      if (k < this.idx - 1 || k > this.idx + 2) {
        URL.revokeObjectURL(r.url)
        this.cache.delete(k)
      }
    }
  }

  private prefetch(i: number) {
    if (i >= 0 && i < this.chunks.length) this.render(i).catch(() => {})
  }

  async playFrom(i: number) {
    if (i < 0 || i >= this.chunks.length) {
      this.hooks.onEnd?.()
      return
    }
    this.idx = i
    try {
      if (!this.started) {
        await init((p) => this.hooks.onProgress?.(p))
        this.started = true
      }
      if (!this.cache.has(i)) this.hooks.onStatus?.("synthesizing…")
      const r = await this.render(i, true)
      if (this.idx !== i) return // user navigated away while synthesizing
      this.hooks.onChunk?.(i, this.chunks.length, r)
      this.audio.src = r.url
      await this.audio.play().catch(() => {})
      this.prefetch(i + 1) // render-ahead: one chunk
    } catch (e) {
      this.hooks.onError?.(e as Error)
    }
  }

  next() {
    if (this.idx + 1 < this.chunks.length) this.playFrom(this.idx + 1)
    else this.hooks.onEnd?.()
  }

  prev() {
    this.playFrom(this.idx - 1)
  }

  resume() {
    if (this.audio.src) this.audio.play().catch(() => {})
    else this.playFrom(this.idx)
  }
}
