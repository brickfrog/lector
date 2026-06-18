// Book-level assembly: shift per-chapter timings onto one timeline.
// Cleaner than the Python merge_ass (which re-parsed formatted ASS); we keep
// TimedTokens in memory and shift before rendering, so no string round-trip.

///|
/// Prefix sums: result[i] = sum of durations[0..<i]; result[0] = 0.
/// Also the chapter start times for an M4B chapter table.
pub fn cumulative_offsets(durations : Array[Double]) -> Array[Double] {
  let out : Array[Double] = []
  let mut acc = 0.0
  for d in durations {
    out.push(acc)
    acc = acc + d
  }
  out
}

///|
/// Shift every token's timestamps by `offset` seconds (returns new tokens).
pub fn shift(
  timed : Array[@ir.TimedToken],
  offset : Double,
) -> Array[@ir.TimedToken] {
  let out : Array[@ir.TimedToken] = []
  for t in timed {
    out.push(@ir.TimedToken::{
      text: t.text,
      whitespace: t.whitespace,
      start_s: t.start_s + offset,
      end_s: t.end_s + offset,
    })
  }
  out
}

///|
/// Concatenate per-chapter timed tokens onto one book timeline: each chapter is
/// shifted by the cumulative duration of all prior chapters. Feed the result to
/// `to_ass` for a single book-level karaoke track.
pub fn merge_timed(
  chapters : Array[Array[@ir.TimedToken]],
  durations : Array[Double],
) -> Array[@ir.TimedToken] {
  let offsets = cumulative_offsets(durations)
  let out : Array[@ir.TimedToken] = []
  for ci in 0..<chapters.length() {
    let off = if ci < offsets.length() { offsets[ci] } else { 0.0 }
    for t in shift(chapters[ci], off) {
      out.push(t)
    }
  }
  out
}
