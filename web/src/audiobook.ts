// Pre-rendered audiobook player: load an audio file + its karaoke .ass (produced
// by the native pipeline) and play with a real seekbar and word-synced read-along.
// No synth, no model — playback is the browser's native <audio>, so seeking and
// scrubbing are free and there are no stalls.
import { parseAss, type PlayLine } from "./ass.js"

const $ = (id: string) => document.getElementById(id)!
const audio = $("ab-audio") as HTMLAudioElement
const reader = $("ab-reader")
const status = $("ab-status")
const list = $("ab-books")
const audioInput = $("ab-audio-file") as HTMLInputElement
const assInput = $("ab-ass-file") as HTMLInputElement

let lines: PlayLine[] = []
let haveAudio = false

interface Book {
  slug: string
  title: string
  audio: string
  seconds: number
  words: number
}

// list books rendered into public/books/ (by render.mjs) so you just click one
async function loadLibrary() {
  let books: Book[] = []
  try {
    books = await (await fetch("/books/index.json")).json()
  } catch {
    /* no library yet */
  }
  list.replaceChildren()
  if (!books.length) {
    list.innerHTML = '<span class="hint">no rendered books yet — run <code>bun render.mjs your.pdf</code></span>'
    return
  }
  for (const b of books) {
    const btn = document.createElement("button")
    const mins = Math.round(b.seconds / 60)
    btn.textContent = `▶ ${b.title} (${mins} min)`
    btn.onclick = () => openBook(b)
    list.append(btn, document.createTextNode(" "))
  }
}

async function openBook(b: Book) {
  status.textContent = `loading ${b.title}…`
  try {
    const [assText] = await Promise.all([fetch(`/books/${b.slug}/book.ass`).then((r) => r.text())])
    lines = parseAss(assText)
    audio.src = `/books/${b.slug}/${b.audio}`
    haveAudio = true
    curLine = -1
    status.innerHTML = `<span class="ok">${b.title}</span> — ${lines.length} lines, press play`
  } catch (e) {
    status.innerHTML = '<span class="bad">load failed: ' + (e as Error).message + "</span>"
  }
}

// rendered window state
let curLine = -1
let wordSpans: HTMLSpanElement[] = []

function ready() {
  if (haveAudio && lines.length) {
    curLine = -1
    status.innerHTML = `<span class="ok">loaded ✓</span> ${lines.length} lines — press play`
  }
}

// last line whose start is <= t (binary search)
function lineAt(t: number): number {
  let lo = 0
  let hi = lines.length - 1
  let ans = 0
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (lines[mid].start_s <= t) {
      ans = mid
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  return ans
}

// render a window of lines centred on `center`; current line gets the spans we highlight
function renderWindow(center: number) {
  reader.replaceChildren()
  wordSpans = []
  const from = Math.max(0, center - 2)
  const to = Math.min(lines.length - 1, center + 8)
  for (let i = from; i <= to; i++) {
    const p = document.createElement("p")
    p.className = i === center ? "ab-line ab-cur" : "ab-line"
    if (i === center) {
      for (const w of lines[i].words) {
        const s = document.createElement("span")
        s.className = "w"
        s.textContent = w.text
        p.append(s, document.createTextNode(" "))
        wordSpans.push(s)
      }
    } else {
      p.textContent = lines[i].words.map((w) => w.text).join(" ")
    }
    reader.append(p)
  }
  const cur = reader.querySelector(".ab-cur")
  cur?.scrollIntoView({ block: "center", behavior: "smooth" })
}

let raf = 0
function tick() {
  const t = audio.currentTime
  const li = lineAt(t)
  if (li !== curLine) {
    curLine = li
    renderWindow(li)
  }
  const words = lines[li].words
  let active = -1
  for (let i = 0; i < words.length; i++) {
    if (t >= words[i].start_s) active = i
    else break
  }
  for (let i = 0; i < wordSpans.length; i++) {
    wordSpans[i].classList.toggle("active", i === active)
    wordSpans[i].classList.toggle("spoken", i < active)
  }
  raf = requestAnimationFrame(tick)
}
audio.onplay = () => {
  cancelAnimationFrame(raf)
  raf = requestAnimationFrame(tick)
}
audio.onpause = () => cancelAnimationFrame(raf)
audio.onseeked = () => {
  curLine = -1
} // force re-render at new position

audioInput.onchange = () => {
  const f = audioInput.files?.[0]
  if (!f) return
  audio.src = URL.createObjectURL(f)
  haveAudio = true
  ready()
}
assInput.onchange = async () => {
  const f = assInput.files?.[0]
  if (!f) return
  try {
    lines = parseAss(await f.text())
    ready()
  } catch (e) {
    status.innerHTML = '<span class="bad">.ass parse failed: ' + (e as Error).message + "</span>"
  }
}

loadLibrary()
