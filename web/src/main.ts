// Slice 1: prove the MoonBit core runs in the browser, plain-data in and out.
import * as lector from "./core/lector.js"
import type { Token } from "./core/lector.js"

const out = document.getElementById("out")!
const status = document.getElementById("status")!
const log = (s: string) => (out.textContent += s + "\n")

try {
  log("fmt_time(3661.5)         = " + lector.fmt_time(3661.5))
  log('pack(["a","b"], 100)     = ' + JSON.stringify(lector.pack(["a", "b"], 100)))

  // a tiny join_timestamps + to_ass round-trip
  const tokens: Token[] = [
    { text: "The", phonemes: "ðə", whitespace: true },
    { text: "cat", phonemes: "kat", whitespace: false },
  ]
  const timed = lector.join_timestamps(tokens, [12, 1, 2, 2, 2, 3, 3, 1])
  log("join_timestamps          = " + JSON.stringify(timed))
  log("")
  log("to_ass (first lines):")
  log(lector.to_ass(timed, 12).split("\n").filter((l) => l.startsWith("Dialogue")).join("\n"))

  status.innerHTML = '<span class="ok">core loaded and ran in the browser ✓</span>'
} catch (e) {
  status.innerHTML = '<span class="bad">core failed: ' + (e as Error).message + "</span>"
}
