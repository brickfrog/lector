// Step A: prove our duration-exposing Kokoro model runs in onnxruntime-web,
// in the browser, and yields per-token durations alongside the waveform.
import * as lector from "./core/lector.js"
import { init, synth } from "./bridge.js"

const out = document.getElementById("out")!
const status = document.getElementById("status")!
const log = (s: string) => (out.textContent += s + "\n")

const TEXT = "The basic concept of exit, voice, and loyalty was developed by Professor Hirschman in 1970."

async function run() {
  try {
    // core sanity (unchanged from slice 1)
    log("core.fmt_time(3661.5) = " + lector.fmt_time(3661.5))

    await init((s) => (status.textContent = s))
    status.textContent = "synthesizing…"
    const r = await synth(TEXT, "bm_george", "b")

    log("")
    log("phonemes: " + r.phonemes)
    log("input_ids: " + r.inputIds.length + " tokens")
    log("duration: " + JSON.stringify(r.duration))
    log("sum(duration) = " + r.duration.reduce((a, b) => a + b, 0) + " frames")
    log("waveform: " + r.waveform.length + " samples (" + (r.waveform.length / r.sampleRate).toFixed(2) + "s)")
    log("samples/frame = " + (r.waveform.length / r.duration.reduce((a, b) => a + b, 0)).toFixed(1))

    status.innerHTML = '<span class="ok">our model ran in onnxruntime-web, duration exposed ✓</span>'
  } catch (e) {
    status.innerHTML = '<span class="bad">bridge failed: ' + (e as Error).message + "</span>"
    log("ERROR: " + (e as Error).stack)
  }
}

run()
