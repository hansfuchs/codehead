# List recipes.
default:
    @just --list

# rustfmt, clippy (warnings fail), tests + tidy (tests/tidy), like CI.
check:
    cargo fmt --check
    cargo clippy --all-targets --locked -- -D warnings
    cargo test --locked

# Git hooks of .githooks: pre-push runs the CI checks on each pushed commit.
hooks:
    git config core.hooksPath .githooks

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

# Demo videos of the real TUI (scripts/demo): fake harnesses, scripted keys → site/public/demo/.
demo *flows:
    cargo build --release --locked
    ~/.cache/codehead-demo/venv/bin/python scripts/demo/demo.py {{flows}}

# Perf suite (src/perf/mod.rs): every case under 20 ms, cold call included. Release build, 1 thread.
# The release binary first: the thread start case starts real hosts with it.
perf *filter:
    cargo build --release --locked
    cargo test --release --locked --bin codehead perf::cases::{{filter}} -- --ignored --test-threads=1 --nocapture
