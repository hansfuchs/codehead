#!/bin/sh
# Release version: <major>.<minor> of Cargo.toml, then the number of commits on HEAD.
# 0.2 + 31 commits → 0.2.31. Grows with every commit, never repeats.
set -eu
mm=$(awk -F'"' '/^\[package\]/ { p = 1; next } /^\[/ { p = 0 } p && /^version *=/ { split($2, v, "."); print v[1] "." v[2]; exit }' Cargo.toml)
echo "$mm.$(git rev-list --count HEAD)"
