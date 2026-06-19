// Per-phoneme model durations -> per-word timestamps, and ASS time formatting.

///|
fn char_len(s : String) -> Int {
  let mut n = 0
  for _c in s {
    n = n + 1
  }
  n
}

///|
/// Count phonemes ignoring spaces and the two stress marks (ˈ U+02C8, ˌ U+02CC).
/// Stress marks move between isolated and in-context phonemization, so excluding
/// them makes per-word and in-context lengths comparable for proportional mapping.
fn stripped_len(s : String) -> Int {
  let mut n = 0
  for c in s {
    let cp = c.to_int()
    if cp != 0x20 && cp != 0x02C8 && cp != 0x02CC {
      n = n + 1
    }
  }
  n
}

///|
/// Time at a cumulative-phoneme position `p` along the per-group timeline.
fn time_at(
  p : Double,
  gcum : Array[Double],
  gw : Array[Double],
  gstart : Array[Double],
  gend : Array[Double],
) -> Double {
  let g = gcum.length()
  if p <= 0.0 {
    return gstart[0]
  }
  for gi in 0..<g {
    if p <= gcum[gi] || gi == g - 1 {
      let group_start_phon = gcum[gi] - gw[gi]
      let frac = (p - group_start_phon) / gw[gi]
      let f = if frac < 0.0 {
        0.0
      } else if frac > 1.0 {
        1.0
      } else {
        frac
      }
      return gstart[gi] + f * (gend[gi] - gstart[gi])
    }
  }
  gend[g - 1]
}

///|
/// Robust word alignment for the espeak host path. espeak merges function words
/// ("of a" -> one phoneme group) and expands numbers, so a positional word<->group
/// zip drifts and drops trailing words. Instead: compute the exact per-group
/// timeline from `pred_dur`, then place each display word's boundary onto it by
/// cumulative phoneme-length proportion (per-word phonemes give the weights).
/// Anchored at both ends, so it can neither drop a word nor desync; exact in the
/// clean 1:1 case, graceful under merges/expansions.
pub fn align_words(
  words : Array[String],
  word_phonemes : Array[String],
  phonemes : String,
  pred_dur : Array[Int],
) -> Array[@ir.TimedToken] {
  let result : Array[@ir.TimedToken] = []
  let w = words.length()
  if w == 0 || pred_dur.length() < 3 {
    return result
  }

  // split the whole phoneme string into space-delimited groups
  let groups : Array[String] = []
  let buf = StringBuilder::new()
  for c in phonemes {
    if c == ' ' {
      let s = buf.to_string()
      if s != "" {
        groups.push(s)
        buf.reset()
      }
    } else {
      buf.write_char(c)
    }
  }
  let tail = buf.to_string()
  if tail != "" {
    groups.push(tail)
  }
  let g = groups.length()
  if g == 0 {
    return result
  }

  // exact per-group [start,end] via the kokoro join magic (half-frames, -3 BOS)
  let magic = 80.0
  let base = pred_dur[0] - 3
  let mut left = 2 * (if base > 0 { base } else { 0 })
  let mut right = left
  let mut i = 1
  let gstart : Array[Double] = Array::make(g, 0.0)
  let gend : Array[Double] = Array::make(g, 0.0)
  let mut timed = 0
  for gi in 0..<g {
    if i >= pred_dur.length() - 1 {
      break
    }
    let glen = char_len(groups[gi])
    let j = i + glen
    if j >= pred_dur.length() {
      break
    }
    let s = left.to_double() / magic
    let mut dur = 0
    for k in i..<j {
      dur = dur + pred_dur[k]
    }
    let has_space = gi < g - 1
    let space = if has_space { pred_dur[j] } else { 0 }
    left = right + 2 * dur + space
    let e = left.to_double() / magic
    right = left + space
    i = j + (if has_space { 1 } else { 0 })
    gstart[gi] = s
    gend[gi] = e
    timed = timed + 1
  }
  if timed == 0 {
    return result
  }
  for k in timed..<g {
    gstart[k] = gend[timed - 1]
    gend[k] = gend[timed - 1]
  }

  // cumulative stripped-phoneme length per group (the time-axis weights)
  let gcum : Array[Double] = Array::make(g, 0.0)
  let gw : Array[Double] = Array::make(g, 0.0)
  let mut gtot = 0.0
  for gi in 0..<g {
    let len = stripped_len(groups[gi]).to_double()
    let lw = if len > 0.0 { len } else { 1.0 }
    gw[gi] = lw
    gtot = gtot + lw
    gcum[gi] = gtot
  }

  // cumulative stripped-phoneme length per display word
  let wcum : Array[Double] = Array::make(w + 1, 0.0)
  let mut wtot = 0.0
  for wi in 0..<w {
    let ph = if wi < word_phonemes.length() { word_phonemes[wi] } else { "" }
    let len = stripped_len(ph).to_double()
    wtot = wtot + (if len > 0.0 { len } else { 1.0 })
    wcum[wi + 1] = wtot
  }
  if wtot == 0.0 {
    return result
  }

  // map each word's [start,end] onto the group timeline by phoneme proportion
  for wi in 0..<w {
    let p0 = wcum[wi] / wtot * gtot
    let p1 = wcum[wi + 1] / wtot * gtot
    result.push(@ir.TimedToken::{
      text: words[wi],
      whitespace: wi < w - 1,
      start_s: time_at(p0, gcum, gw, gstart, gend),
      end_s: time_at(p1, gcum, gw, gstart, gend),
    })
  }
  result
}

///|
fn pad2(n : Int) -> String {
  if n < 10 {
    "0" + n.to_string()
  } else {
    n.to_string()
  }
}

///|
/// Format seconds as an ASS timestamp "H:MM:SS.cc" (centisecond precision).
/// Hand-rolled because MoonBit has no printf-style padding; rounds to centiseconds.
pub fn fmt_time(t : Double) -> String {
  let total_cs = (t * 100.0 + 0.5).to_int()
  let h = total_cs / 360000
  let rem = total_cs % 360000
  let m = rem / 6000
  let rem2 = rem % 6000
  let s = rem2 / 100
  let cs = rem2 % 100
  h.to_string() + ":" + pad2(m) + ":" + pad2(s) + "." + pad2(cs)
}

///|
/// Faithful port of kokoro pipeline.join_timestamps (pipeline.py:295-330).
/// Tracks (left,right) in HALF-frames with MAGIC_DIVISOR=80 (==frames/40 seconds),
/// a -3 BOS offset, and folds inter-word space duration. Returns only tokens that
/// received a timestamp (whitespace-only / overflow tokens are dropped).
pub fn join_timestamps(
  tokens : Array[@ir.Token],
  pred_dur : Array[Int],
) -> Array[@ir.TimedToken] {
  let result : Array[@ir.TimedToken] = []
  if tokens.length() == 0 || pred_dur.length() < 3 {
    return result
  }
  let magic = 80.0
  let base = pred_dur[0] - 3
  let mut left = 2 * (if base > 0 { base } else { 0 })
  let mut right = left
  let mut i = 1
  for t in tokens {
    if i >= pred_dur.length() - 1 {
      break
    }
    if t.phonemes == "" {
      if t.whitespace {
        i = i + 1
        left = right + pred_dur[i]
        right = left + pred_dur[i]
        i = i + 1
      }
      continue
    }
    let j = i + t.phonemes.length()
    if j >= pred_dur.length() {
      break
    }
    let start_s = left.to_double() / magic
    let mut token_dur = 0
    for k in i..<j {
      token_dur = token_dur + pred_dur[k]
    }
    let space = if t.whitespace { pred_dur[j] } else { 0 }
    left = right + 2 * token_dur + space
    let end_s = left.to_double() / magic
    right = left + space
    i = j + (if t.whitespace { 1 } else { 0 })
    result.push(@ir.TimedToken::{
      text: t.text,
      whitespace: t.whitespace,
      start_s,
      end_s,
    })
  }
  result
}
