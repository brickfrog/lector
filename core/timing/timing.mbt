// Per-phoneme model durations -> per-word timestamps, and ASS time formatting.

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
