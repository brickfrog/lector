///|
/// JSON->ASS harness. Decodes a fixture via the stdlib `@json` (derive(FromJson)),
/// runs the pure core, and prints ASS karaoke. The JSON is embedded as a raw
/// string here; a host would read it from a file/stdin/postMessage instead.
struct Fixture {
  tokens : Array[@ir.Token]
  pred_dur : Array[Int]
} derive(FromJson)

///|
fn main {
  let raw =
    #|{"tokens":[{"text":"The","phonemes":"ðə","whitespace":true},{"text":"basic","phonemes":"bˈAsɪk","whitespace":true},{"text":"concept","phonemes":"kˈɒnsɛpt","whitespace":true},{"text":"of","phonemes":"ɒv","whitespace":true},{"text":"exit","phonemes":"ˈɛksɪt","whitespace":false},{"text":",","phonemes":",","whitespace":true},{"text":"voice","phonemes":"vˈYs","whitespace":false},{"text":",","phonemes":",","whitespace":true},{"text":"and","phonemes":"and","whitespace":true},{"text":"loyalty","phonemes":"lˈYəlti","whitespace":true},{"text":"was","phonemes":"wɒz","whitespace":true},{"text":"developed","phonemes":"dɪvˈɛləpt","whitespace":true},{"text":"by","phonemes":"bI","whitespace":true},{"text":"Professor","phonemes":"pɹəfˈɛsə","whitespace":true},{"text":"Hirschman","phonemes":"hˈɜːʃmən","whitespace":true},{"text":"in","phonemes":"ɪn","whitespace":true},{"text":"1970","phonemes":"nˌIntˈiːn sˈɛvnti","whitespace":false},{"text":".","phonemes":".","whitespace":false}],"pred_dur":[12,1,2,2,2,3,3,3,2,2,2,2,2,2,4,4,3,2,2,2,2,2,3,2,2,3,4,4,8,4,3,2,5,5,6,2,2,1,1,1,2,2,4,3,2,3,4,17,4,2,2,2,2,2,2,2,2,2,2,2,2,1,2,3,2,2,2,1,3,2,2,2,3,2,2,1,2,2,3,3,3,3,7,3,2,1,2,1,2,2,2,2,2,1,2,2,3,2,2,2,2,3,4,17,7,1]}
  let fx : Fixture = @json.from_json(@json.parse(raw)) catch {
    e => {
      println("error: \{e}")
      return
    }
  }
  let timed = @lector.join_timestamps(fx.tokens, fx.pred_dur)
  println(@lector.to_ass(timed, 12))
}
