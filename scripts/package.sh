#!/bin/sh
# Package the release build of TARGET as codehead-<VERSION>-<TARGET>.tar.gz + its .sha256.
# Usage: scripts/package.sh <version> <target>
set -eu
version=$1
target=$2
bin="target/$target/release/codehead"
[ -f "$bin" ] || { echo "package: $bin missing" >&2; exit 1; }
# the binary must be the one asked for: its arch, or (native builds) its version
if [ "$target" = aarch64-apple-darwin ]; then
  lipo -archs "$bin" | grep -qx arm64
else
  "$bin" --version | grep -q "$version"
fi
out="codehead-$version-$target.tar.gz"
tar -czf "$out" -C "$(dirname "$bin")" codehead
if command -v sha256sum >/dev/null 2>&1; then sha256sum "$out"; else shasum -a 256 "$out"; fi > "$out.sha256"
echo "$out"
