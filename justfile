# lector — tasks. `just <task>` (run `just` to list).

default:
    @just --list

# build the release binary
build:
    cargo build --release

# render a PDF to an audiobook (mp3 + .ass). usage: just render book.pdf [outdir]
render pdf outdir=".":
    cargo run --release -- {{pdf}} {{outdir}}

# fetch voice + tokenizer into assets/ (model: see scripts/export_dur.py)
vendor:
    bash scripts/vendor.sh

# format + lint
check:
    cargo fmt && cargo clippy
