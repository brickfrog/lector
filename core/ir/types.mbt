// Shared types for lector. Pure data; no I/O, no ML, no backend-specific code.

///|
/// A single input token from the TTS G2P: display text, its phoneme string, and
/// whether it is followed by whitespace. Fed to `join_timestamps` alongside the
/// model's per-phoneme `pred_dur`.
pub(all) struct Token {
  text : String
  phonemes : String
  whitespace : Bool
} derive(FromJson, ToJson)

///|
/// A token that received a timestamp from `join_timestamps`, in seconds.
pub(all) struct TimedToken {
  text : String
  whitespace : Bool
  start_s : Double
  end_s : Double
} derive(ToJson)

///|
/// A chapter of narratable text: a title plus reading-ordered paragraphs.
pub(all) struct Chapter {
  title : String
  paragraphs : Array[String]
}

///|
/// Max characters per TTS chunk (host API limits). Bounds alignment memory too.
pub let tts_max_chars : Int = 8000

///|
/// Max characters per glyph-repair LLM call. Documents the boundary; unused in core.
pub let repair_max_chars : Int = 16000
