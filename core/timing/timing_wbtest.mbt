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
