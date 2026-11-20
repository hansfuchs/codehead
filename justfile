# List recipes.
default:
    @just --list

# Clippy (warnings fail) and tests, like CI.
check:
    cargo clippy --all-targets --locked -- -D warnings
    cargo test --locked

# Build a release binary and link it into ~/.local/bin. Later release builds take effect at once.
install:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release --locked
    target="{{justfile_directory()}}/target/release/codehead"
    mkdir -p ~/.local/bin
    ln -sfn "$target" ~/.local/bin/codehead
    echo "installed: $(command -v codehead) -> $(readlink "$(command -v codehead)")"
    codehead --version

# Website dev server (codehead.sh), http://localhost:4321.
site:
    cd site && bun install && bun run dev

# Website production build into site/dist.
site-build:
    cd site && bun install --frozen-lockfile && bun run build

# Agent sprite pools (examples/sprites.rs): --force regenerates pcb + chip, --sheet prints them.
sprites *args:
    cargo run --quiet --example sprites -- {{args}}

# Demo videos of the real TUI (scripts/demo): fake harnesses, scripted keys → site/public/demo/.
demo *flows:
    cargo build --release --locked
    ~/.cache/codehead-demo/venv/bin/python scripts/demo/demo.py {{flows}}
