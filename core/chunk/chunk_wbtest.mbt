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
