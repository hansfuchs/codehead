#!/bin/sh
# Package the release build of TARGET as codehead-<VERSION>-<TARGET>.tar.gz + its .sha256.
# Usage: scripts/package.sh <version> <target>
set -eu
version=$1
target=$2
bin="target/$target/release/codehead"
[ -f "$bin" ] || { echo "package: $bin missing" >&2; exit 1; }
# the binary must be the one asked for: its arch and version (macOS: 1 runner builds both
# arches, 1 of them cross-compiled: lipo + the version string, 0 run), else a run of it
case "$target" in
  aarch64-apple-darwin) lipo -archs "$bin" | grep -qx arm64 && grep -aqF "$version" "$bin" ;;
  x86_64-apple-darwin) lipo -archs "$bin" | grep -qx x86_64 && grep -aqF "$version" "$bin" ;;
  *) "$bin" --version | grep -q "$version" ;;
esac
out="codehead-$version-$target.tar.gz"
tar -czf "$out" -C "$(dirname "$bin")" codehead
if command -v sha256sum >/dev/null 2>&1; then sha256sum "$out"; else shasum -a 256 "$out"; fi > "$out.sha256"
echo "$out"
