# lector monorepo — top-level tasks. `just <task>` (run `just` to list).

default:
    @just --list

# core (MoonBit): run unit + golden tests
test-core:
    cd core && moon test

# core: format + typecheck
check-core:
    cd core && moon fmt && moon check

# core: build for the JS backend (browser-host readiness)
build-core-js:
    cd core && moon build --target js

# core: run the JSON -> ASS demo harness
demo:
    cd core && moon run cmd/main

# everything currently wired
ci: check-core test-core build-core-js
