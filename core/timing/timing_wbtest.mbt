// Auto-generated golden parity test from testdata/sentence.json
// (Kokoro bm_george, sentence verified against the Python pipeline).

///|
fn approx(a : Double, b : Double) -> Bool {
  let d = a - b
  (if d < 0.0 { -d } else { d }) < 0.002
}

///|
test "join_timestamps golden parity" {
  let tokens : Array[@ir.Token] = [
    @ir.Token::{ text: "The", phonemes: "ðə", whitespace: true },
    @ir.Token::{ text: "basic", phonemes: "bˈAsɪk", whitespace: true },
    @ir.Token::{ text: "concept", phonemes: "kˈɒnsɛpt", whitespace: true },
    @ir.Token::{ text: "of", phonemes: "ɒv", whitespace: true },
    @ir.Token::{ text: "exit", phonemes: "ˈɛksɪt", whitespace: false },
    @ir.Token::{ text: ",", phonemes: ",", whitespace: true },
    @ir.Token::{ text: "voice", phonemes: "vˈYs", whitespace: false },
    @ir.Token::{ text: ",", phonemes: ",", whitespace: true },
    @ir.Token::{ text: "and", phonemes: "and", whitespace: true },
    @ir.Token::{ text: "loyalty", phonemes: "lˈYəlti", whitespace: true },
    @ir.Token::{ text: "was", phonemes: "wɒz", whitespace: true },
    @ir.Token::{
      text: "developed",
      phonemes: "dɪvˈɛləpt",
      whitespace: true,
    },
    @ir.Token::{ text: "by", phonemes: "bI", whitespace: true },
    @ir.Token::{
      text: "Professor",
      phonemes: "pɹəfˈɛsə",
      whitespace: true,
    },
    @ir.Token::{
      text: "Hirschman",
      phonemes: "hˈɜːʃmən",
      whitespace: true,
    },
    @ir.Token::{ text: "in", phonemes: "ɪn", whitespace: true },
    @ir.Token::{
      text: "1970",
      phonemes: "nˌIntˈiːn sˈɛvnti",
      whitespace: false,
    },
    @ir.Token::{ text: ".", phonemes: ".", whitespace: false },
  ]
  let pred_dur = [
    12, 1, 2, 2, 2, 3, 3, 3, 2, 2, 2, 2, 2, 2, 4, 4, 3, 2, 2, 2, 2, 2, 3, 2, 2, 3,
    4, 4, 8, 4, 3, 2, 5, 5, 6, 2, 2, 1, 1, 1, 2, 2, 4, 3, 2, 3, 4, 17, 4, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 2, 3, 2, 2, 2, 1, 3, 2, 2, 2, 3, 2, 2, 1, 2, 2,
    3, 3, 3, 3, 7, 3, 2, 1, 2, 1, 2, 2, 2, 2, 2, 1, 2, 2, 3, 2, 2, 2, 2, 3, 4, 17,
    7, 1,
  ]
  let timed = join_timestamps(tokens, pred_dur)
  assert_eq(timed.length(), 18)
  let exp_text = [
    "The", "basic", "concept", "of", "exit", ",", "voice", ",", "and", "loyalty",
    "was", "developed", "by", "Professor", "Hirschman", "in", "1970", ".",
  ]
  let exp_start = [
    0.225, 0.325, 0.75, 1.325, 1.488, 2.1, 2.237, 2.725, 2.8, 2.925, 3.875, 4.1,
    4.575, 4.75, 5.225, 5.888, 6.025, 7.325,
  ]
  let exp_end = [
    0.325, 0.75, 1.325, 1.488, 2.1, 2.237, 2.725, 2.8, 2.925, 3.875, 4.1, 4.575,
    4.75, 5.225, 5.888, 6.025, 7.325, 7.5,
  ]
  for k in 0..<timed.length() {
    assert_eq(timed[k].text, exp_text[k])
    assert_true(approx(timed[k].start_s, exp_start[k]))
    assert_true(approx(timed[k].end_s, exp_end[k]))
  }
}

///|
test "align_words: clean 1:1 is complete, monotonic, anchored" {
  let words = ["the", "cat", "sat"]
  let wph = ["ðə", "kat", "sat"]
  let phonemes = "ðə kat sat"
  // BOS + (ð ə _ k a t _ s a t) + EOS
  let pred = [3, 5, 5, 4, 6, 6, 6, 4, 7, 7, 7, 3]
  let out = align_words(words, wph, phonemes, pred)
  assert_eq(out.length(), 3)
  assert_eq(out[0].text, "the")
  assert_eq(out[2].text, "sat")
  // first word anchored at 0, last word ends at the group timeline end (~1.425)
  assert_true(approx(out[0].start_s, 0.0))
  assert_true(out[2].end_s > 1.4 && out[2].end_s < 1.45)
  // monotonic, every word has nonneg duration
  for k in 0..<out.length() {
    assert_true(out[k].start_s <= out[k].end_s)
  }
  assert_true(out[0].end_s <= out[1].start_s + 0.001)
  assert_true(out[1].end_s <= out[2].start_s + 0.001)
}

///|
test "align_words: function-word merge keeps every word (regression)" {
  // espeak merges "of a" -> one group "əvə"; the old positional zip padded the
  // trailing words with empty phonemes and join_timestamps then dropped them.
  let words = ["of", "a", "cat"]
  let wph = ["ˈʌv", "ˈeɪ", "kˈæt"] // per-word phonemes (isolated, stressed)
  let phonemes = "əvə kˈæt" // in-context: of+a fused, no internal space
  // BOS + (ə v ə _ k ˈ æ t) + EOS
  let pred = [3, 5, 5, 5, 4, 6, 1, 6, 6, 3]
  let out = align_words(words, wph, phonemes, pred)
  assert_eq(out.length(), 3) // nothing dropped
  assert_eq(out[0].text, "of")
  assert_eq(out[1].text, "a")
  assert_eq(out[2].text, "cat")
  // monotonic and anchored
  assert_true(out[0].start_s <= out[1].start_s)
  assert_true(out[1].start_s <= out[2].start_s)
  assert_true(out[2].end_s > out[0].start_s)
}
