///|
test "cumulative_offsets are prefix sums starting at 0" {
  let off = cumulative_offsets([10.0, 5.0, 3.0])
  assert_eq(off.length(), 3)
  assert_eq(off[0], 0.0)
  assert_eq(off[1], 10.0)
  assert_eq(off[2], 15.0)
}

///|
test "merge_timed places each chapter after prior durations" {
  let c0 = [
    @ir.TimedToken::{ text: "a", whitespace: true, start_s: 0.0, end_s: 1.0 },
  ]
  let c1 = [
    @ir.TimedToken::{ text: "b", whitespace: false, start_s: 0.0, end_s: 2.0 },
  ]
  let merged = merge_timed([c0, c1], [10.0, 5.0])
  assert_eq(merged.length(), 2)
  assert_eq(merged[0].start_s, 0.0)
  assert_eq(merged[1].start_s, 10.0)
  assert_eq(merged[1].end_s, 12.0)
}
