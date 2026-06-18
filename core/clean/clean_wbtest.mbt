///|
test "page_paragraphs: drops sub-body spans, reflows lines, splits on indent" {
  let spans = [
    Span::{ text: "Hello", size: 16.0, x: 10.0, y: 100.0 },
    Span::{ text: "world", size: 16.0, x: 60.0, y: 100.0 }, // same line (y=100)
    Span::{ text: "continued.", size: 16.0, x: 10.0, y: 120.0 }, // same para (at margin)
    Span::{ text: "a footnote", size: 12.0, x: 10.0, y: 140.0 }, // dropped (< body)
    Span::{ text: "New para.", size: 16.0, x: 25.0, y: 160.0 }, // indented -> new para
  ]
  let paras = page_paragraphs(spans, 15.0)
  assert_eq(paras.length(), 2)
  assert_eq(paras[0], "Hello world continued.")
  assert_eq(paras[1], "New para.")
}

///|
test "page_paragraphs: skips standalone page numbers" {
  let spans = [
    Span::{ text: "Real line.", size: 16.0, x: 10.0, y: 50.0 },
    Span::{ text: "47", size: 16.0, x: 10.0, y: 300.0 }, // page number -> skipped
  ]
  let paras = page_paragraphs(spans, 15.0)
  assert_eq(paras.length(), 1)
  assert_eq(paras[0], "Real line.")
}

///|
test "dehyphenate rejoins wrapped words, spares dashes" {
  assert_eq(dehyphenate("infor- mation"), "information") // line-wrap rejoined
  assert_eq(dehyphenate("a-b"), "a-b") // hyphen, no space -> spared (real hyphen)
  assert_eq(dehyphenate("clean text"), "clean text")
}

///|
test "squeeze collapses and trims whitespace" {
  assert_eq(squeeze("  a   b \n c "), "a b c")
}

///|
test "detect_body_size returns the dominant size" {
  let spans = [
    Span::{ text: "aaaaaaaaaaaaaaa", size: 16.0, x: 0.0, y: 0.0 }, // most chars
    Span::{ text: "bb", size: 12.0, x: 0.0, y: 0.0 },
  ]
  assert_eq(detect_body_size(spans), 16.0)
}
