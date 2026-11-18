#!/bin/sh
# Write VERSION into [package] version of Cargo.toml (release builds only).
set -eu
awk -v v="$1" '/^\[package\]/ { p = 1 } /^\[/ && !/^\[package\]/ { p = 0 } p && /^version *=/ { $0 = "version = \"" v "\"" } { print }' Cargo.toml > Cargo.toml.stamped
mv Cargo.toml.stamped Cargo.toml
