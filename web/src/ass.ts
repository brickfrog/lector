// Parse a karaoke .ass (the format core.to_ass / reference align.py emit) back
// into timed lines for the browser read-along player. Inverse of core to_ass:
// each Dialogue line starts at its Start time; per-word `\kf<cs>` durations are
// centiseconds (gap folded into the preceding word), accumulated across the line.

export interface PlayWord {
  text: string
  start_s: number
  end_s: number
}
export interface PlayLine {
  start_s: number
  end_s: number
  words: PlayWord[]
}

// "H:MM:SS.cc" -> seconds
function parseTime(s: string): number {
  const [h, m, rest] = s.trim().split(":")
  const [ss, cc = "0"] = rest.split(".")
  return +h * 3600 + +m * 60 + +ss + +cc / 100
}

const RUN = /\{\\[kK][fF]?(\d+)\}([^{]*)/g

export function parseAss(text: string): PlayLine[] {
  const lines: PlayLine[] = []
  for (const raw of text.split(/\r?\n/)) {
    if (!raw.startsWith("Dialogue:")) continue
    // fields: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
    // Text (field 9) may contain commas, so only split off the first 9.
    const parts = raw.slice("Dialogue:".length).split(",")
    if (parts.length < 10) continue
    const start = parseTime(parts[1])
    const txt = parts.slice(9).join(",")
    const words: PlayWord[] = []
    let t = start
    let m: RegExpExecArray | null
    RUN.lastIndex = 0
    while ((m = RUN.exec(txt)) !== null) {
      const dur = +m[1] / 100
      const display = m[2].trim()
      const s = t
      t += dur
      if (display) words.push({ text: display, start_s: s, end_s: t })
    }
    if (words.length) lines.push({ start_s: start, end_s: t, words })
  }
  return lines
}
