// brickfrog/lector — public API.
//
// Pure, backend-agnostic core of the karaoke-audiobook pipeline: paragraph
// chunking, per-phoneme -> per-word timing (from a TTS model's pred_dur), ASS
// karaoke rendering, and book-level timeline assembly. No I/O, no ML, no FFI;
// compiles to both JS (browser host) and native (CLI/tests).
//
// Types live in `brickfrog/lector/ir` (@ir.Token, @ir.TimedToken, @ir.Chapter);
// the host/tests import that package for the data shapes. This package re-exports
// the operations as one call surface.

///|
/// PDF spans -> cleaned, reflowed paragraphs (body text only). Spans come from
/// the host (pdf.js / PyMuPDF) as { text, size, x, y }.
pub fn page_paragraphs(
  spans : Array[@clean.Span],
  body_min_size : Double,
) -> Array[String] {
  @clean.page_paragraphs(spans, body_min_size)
}

///|
/// Auto-detect the dominant (body) font size from spans.
pub fn detect_body_size(spans : Array[@clean.Span]) -> Double {
  @clean.detect_body_size(spans)
}

///|
/// Greedy-pack paragraphs into <= max_chars chunks (whole paragraphs only).
pub fn pack(paras : Array[String], max_chars : Int) -> Array[String] {
  @chunk.pack(paras, max_chars)
}

///|
/// Sentence-level packing for short-input TTS (Kokoro caps at ~510 phonemes):
/// splits paragraphs into sentences, packs to <= max_chars per chunk.
pub fn sentence_pack(paras : Array[String], max_chars : Int) -> Array[String] {
  @chunk.sentence_pack(paras, max_chars)
}

///|
/// Per-phoneme pred_dur frames -> per-word timestamps (seconds).
pub fn join_timestamps(
  tokens : Array[@ir.Token],
  pred_dur : Array[Int],
) -> Array[@ir.TimedToken] {
  @timing.join_timestamps(tokens, pred_dur)
}

///|
/// Robust display-word timing for the espeak host path: maps words onto the exact
/// per-group pred_dur timeline by phoneme-length proportion. Tolerant of espeak's
/// function-word merges and number expansions; never drops or desyncs words.
pub fn align_words(
  words : Array[String],
  word_phonemes : Array[String],
  phonemes : String,
  pred_dur : Array[Int],
) -> Array[@ir.TimedToken] {
  @timing.align_words(words, word_phonemes, phonemes, pred_dur)
}

///|
/// Format seconds as an ASS "H:MM:SS.cc" timestamp.
pub fn fmt_time(t : Double) -> String {
  @timing.fmt_time(t)
}

///|
/// Group words into screen lines on sentence boundaries / width.
pub fn split_lines(
  words : Array[String],
  max_words : Int,
) -> Array[Array[String]] {
  @ass.split_lines(words, max_words)
}

///|
/// Render timed tokens to a full ASS karaoke document.
pub fn to_ass(timed : Array[@ir.TimedToken], max_words : Int) -> String {
  @ass.to_ass(timed, max_words)
}

///|
/// Chapter start times (prefix sums of durations).
pub fn cumulative_offsets(durations : Array[Double]) -> Array[Double] {
  @assemble.cumulative_offsets(durations)
}

///|
/// Shift a chapter's timestamps by an offset.
pub fn shift(
  timed : Array[@ir.TimedToken],
  offset : Double,
) -> Array[@ir.TimedToken] {
  @assemble.shift(timed, offset)
}

///|
/// Concatenate per-chapter timings onto one book timeline.
pub fn merge_timed(
  chapters : Array[Array[@ir.TimedToken]],
  durations : Array[Double],
) -> Array[@ir.TimedToken] {
  @assemble.merge_timed(chapters, durations)
}
