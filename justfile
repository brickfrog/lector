# lector — tasks. `just <task>` (run `just` to list).

default:
    @just --list

# build the release binary
build:
    cargo build --release --locked

# render a book to an audiobook (.m4b + .ass). usage: just render book.epub [outdir]
render book outdir=".":
    cargo run --release --locked -- {{quote(book)}} -o {{quote(outdir)}}

# fetch voice + tokenizer into assets/ (model: see scripts/export_dur.py)
vendor:
    bash scripts/vendor.sh

# format, lint, and regression tests
check:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked
    python3 -m unittest discover -s scripts -p 'test_*.py'

# tests requiring installed external tools
check-tools:
    cargo test --locked -- --ignored

# current dependency advisories
audit:
    python3 scripts/audit.py
