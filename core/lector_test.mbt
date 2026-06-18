// Blackbox tests of the public API via @lector.

///|
test "pack joins paragraphs under the limit" {
  let chunks = @lector.pack(["hello world", "second para"], 100)
  assert_eq(chunks.length(), 1)
  assert_eq(chunks[0], "hello world\n\nsecond para")
}

///|
test "split_lines breaks on a sentence ender past 4 words" {
  let lines = @lector.split_lines(["a", "b", "c", "d."], 12)
  assert_eq(lines.length(), 1)
}

///|
test "fmt_time formats H:MM:SS.cc" {
  assert_eq(@lector.fmt_time(3661.5), "1:01:01.50")
  assert_eq(@lector.fmt_time(0.225), "0:00:00.23")
}
