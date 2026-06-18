# lector

Turns a book (PDF or EPUB) into a karaoke audiobook: synthesized narration with
the words highlighted as they're spoken. Runs locally, no per-use cost.

Monorepo with components in a few languages. They share one data format: a list
of `{text, phonemes, whitespace}` tokens plus the TTS model's per-phoneme frame
durations (`pred_dur`). Word timings come straight from those durations, so there
is no separate forced-alignment step. Kokoro already computes the durations while
it synthesizes.

## Layout

| dir | what | language | status |
|-----|------|----------|--------|
| `core/` | `brickfrog/lector`. The logic library: paragraph chunking, `pred_dur` to word timestamps, ASS karaoke output, book-timeline assembly. Pure functions, no I/O or model code. Builds for JS and native. | MoonBit | tested, 9/9 |
| `reference/` | The Python pipeline the core was ported from, and where the test fixtures come from. Extract, glyph-repair via LLM, Kokoro TTS, alignment, ASS, chaptered M4B. Sandboxed with bubblewrap and memory caps. | Python | working |
| `web/` | Browser app: core compiled to JS, plus kokoro-js and JASSUB. | TS | not started |
| `bridge/` | Pulls `(tokens, pred_dur)` out of Kokoro to feed the core. | TS or Py | not started |

## Running core

```sh
cd core
moon test               # unit tests + the golden fixture
moon run cmd/main       # decode a JSON fixture, print the ASS
moon build --target js  # JS backend build
```

## Notes

`core` takes data and returns strings. It does not read files, run models, or mux
audio. That work belongs to the host: `reference/` does it today, `web/` and
`bridge/` will later.

The timestamp math in `core` is checked against a Kokoro-generated fixture
(`core/testdata/sentence.json`) and matches the Python output to within 2 ms.

Source books and generated audio are not in the repo. They're copyrighted and
large, and `.gitignore` blocks them. Bring your own input.

Not on mooncakes yet.
