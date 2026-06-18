// Precise hand-written types for the MoonBit core's JS export.
// The generated lector.d.ts types compound args as `any`; verified from node,
// the runtime crosses the boundary as plain JS arrays/objects, so these shapes
// are accurate.

export interface Token {
  text : string
  phonemes : string
  whitespace : boolean
}

export interface TimedToken {
  text : string
  whitespace : boolean
  start_s : number
  end_s : number
}

export function pack(paras : string[], max_chars : number) : string[]

export function join_timestamps(
  tokens : Token[],
  pred_dur : number[],
) : TimedToken[]

export function fmt_time(t : number) : string

export function split_lines(words : string[], max_words : number) : string[][]

export function to_ass(timed : TimedToken[], max_words : number) : string

export function cumulative_offsets(durations : number[]) : number[]

export function shift(timed : TimedToken[], offset : number) : TimedToken[]

export function merge_timed(
  chapters : TimedToken[][],
  durations : number[],
) : TimedToken[]
