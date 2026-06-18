// Heuristic word aligner: map a flat espeak phoneme string back to display words.
//
// kokoro-js/espeak gives space-separated phoneme groups, not clean per-word
// tokens (misaki, the Python path, did). Most words are 1:1 with a group; the
// mismatch is expanded numbers ("1970" -> two groups) and the like. We build one
// Token per DISPLAY word, letting a token's `phonemes` span multiple groups
// (internal spaces preserved) when a word expanded — which is exactly how the
// Python MToken for "1970" looked. join_timestamps consumes the char indices.
//
// Known imperfection (the documented footgun): with several expanded tokens the
// surplus is attributed naively (to the first digit-bearing word, then the last
// word as fallback). Fine for prose, can slip on number-dense text.

import type { Token } from "./core/lector.js"

export function buildTokens(text: string, phonemes: string): Token[] {
  const words = text.trim().split(/\s+/)
  const groups = phonemes.trim().split(/\s+/)
  const tokens: Token[] = []

  if (groups.length <= words.length) {
    // 1:1 (or espeak merged some words): zip, pad last with remaining groups
    for (let i = 0; i < words.length; i++) {
      const ph = i < groups.length ? groups[i] : ""
      tokens.push({ text: words[i], phonemes: ph, whitespace: i < words.length - 1 })
    }
    return tokens
  }

  // more groups than words: surplus came from expansions (numbers). Absorb it.
  let surplus = groups.length - words.length
  let gi = 0
  for (let wi = 0; wi < words.length; wi++) {
    let take = 1
    const last = wi === words.length - 1
    if (surplus > 0 && /\d/.test(words[wi])) {
      take += surplus
      surplus = 0
    } else if (last && surplus > 0) {
      take += surplus
      surplus = 0
    }
    tokens.push({
      text: words[wi],
      phonemes: groups.slice(gi, gi + take).join(" "),
      whitespace: !last,
    })
    gi += take
  }
  return tokens
}
