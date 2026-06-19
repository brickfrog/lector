///|
test "pack packs whole paragraphs and never splits" {
  // "aaaa\n\nbbbb" = 10 chars fits; adding cccc (10+2+4=16) overflows -> new chunk
  let out = pack(["aaaa", "bbbb", "cccc"], 10)
  assert_eq(out.length(), 2)
  assert_eq(out[0], "aaaa\n\nbbbb")
  assert_eq(out[1], "cccc")
}

///|
test "pack: a single oversized paragraph is kept intact" {
  let out = pack(["a-very-long-paragraph-exceeding-max"], 8)
  assert_eq(out.length(), 1)
  assert_eq(out[0], "a-very-long-paragraph-exceeding-max")
}

///|
test "pack: empty input yields empty" {
  let out = pack([], 8000)
  assert_eq(out.length(), 0)
}

///|
test "sentence_pack splits paragraphs into sentences and packs to budget" {
  // two sentences; budget 40 forces a split between them
  let out = sentence_pack(["First sentence here is longer. Second one."], 40)
  assert_eq(out.length(), 2)
  assert_eq(out[0], "First sentence here is longer.")
  assert_eq(out[1], "Second one.")
}

///|
test "sentence_pack: short sentences pack together, across paragraphs" {
  let out = sentence_pack(["One. Two.", "Three!"], 8000)
  assert_eq(out.length(), 1)
  assert_eq(out[0], "One. Two. Three!")
}

///|
test "sentence_pack: handles closing quote and collapses whitespace" {
  let out = sentence_pack(["\"Stop!\" she cried.  Then\nsilence."], 8000)
  assert_eq(out.length(), 1)
  assert_eq(out[0], "\"Stop!\" she cried. Then silence.")
}

///|
test "sentence_pack: an oversized single sentence is kept intact" {
  let out = sentence_pack(["this one sentence has no terminator and is long"], 10)
  assert_eq(out.length(), 1)
  assert_eq(out[0], "this one sentence has no terminator and is long")
}
