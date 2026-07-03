# lector — tasks. `just <task>` (run `just` to list).

default:
    @just --list

# build the release binary
build:
    cargo build --release

# render a book to an audiobook (.m4b + .ass). usage: just render book.epub [outdir]
render book outdir=".":
    cargo run --release -- {{book}} -o {{outdir}}

# fetch voice + tokenizer into assets/ (model: see scripts/export_dur.py)
vendor:
    bash scripts/vendor.sh

# format + lint
check:
    cargo fmt && cargo clippy
