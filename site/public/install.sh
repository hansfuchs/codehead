#!/bin/sh
# codehead installer.
#
#   curl -fsSL https://codehead.sh/install | sh              latest release
#   curl -fsSL https://codehead.sh/install | sh -s -- 0.2.31 a given version
#
# What it does, in order:
#   1. finds the release build for this OS and CPU on github.com/hansfuchs/codehead
#   2. downloads it with its .sha256 file and compares the two
#   3. with a logged-in `gh`: checks the build's GitHub attestation too
#   4. puts the `codehead` binary into $CODEHEAD_INSTALL_DIR (default ~/.local/bin)
# 0 sudo, 0 other files touched. The body runs only once the whole script is here.

set -eu

REPO=hansfuchs/codehead
DEST=${CODEHEAD_INSTALL_DIR:-$HOME/.local/bin}

say() { printf '  %s\n' "$*"; }
good() { printf '  [ok] %s\n' "$*"; }
warn() { printf '  [!]  %s\n' "$*"; }
fail() {
  printf 'codehead install: %s\n' "$*" >&2
  exit 1
}

need() {
  for c in "$@"; do
    command -v "$c" >/dev/null 2>&1 || fail "missing tool: $c"
  done
}

get() {
  # https only, also after redirects; HTTP errors fail
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 "$@"
}

checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    fail "missing tool: sha256sum or shasum"
  fi
}

detect_target() {
  os=$(uname -s)
  cpu=$(uname -m)
  case "$os" in
    Linux) os=unknown-linux-gnu ;;
    Darwin) os=apple-darwin ;;
    *) fail "no build for $os (Linux and macOS only)" ;;
  esac
  case "$cpu" in
    aarch64 | arm64) cpu=aarch64 ;;
    x86_64 | amd64) cpu=x86_64 ;;
    *) fail "no build for CPU $cpu" ;;
  esac
  # an x86_64 shell under Rosetta on Apple silicon: take the arm64 build
  if [ "$os" = apple-darwin ] && [ "$cpu" = x86_64 ] && [ "$(sysctl -in sysctl.proc_translated 2>/dev/null)" = 1 ]; then
    cpu=aarch64
  fi
  echo "$cpu-$os"
}

# The tag behind /releases/latest, and the repo's real owner/name (attestations use it).
latest_tag() {
  url=$(get -o /dev/null -w '%{url_effective}' -I "https://github.com/$REPO/releases/latest") || fail "github.com unreachable"
  case "$url" in
    */releases/tag/v*) echo "$url" ;;
    *) fail "0 releases yet: https://github.com/$REPO/releases" ;;
  esac
}

install_codehead() {
  need curl tar uname mktemp awk
  [ "$(id -u)" -ne 0 ] || fail "run as your own user: the binary goes into your home"

  target=$(detect_target)
  tag_url=$(latest_tag)
  owner_repo=$(printf '%s' "$tag_url" | sed -e 's#^https://github.com/##' -e 's#/releases/.*##')
  version=${1:-${tag_url##*/tag/v}}
  version=${version#v}
  printf '%s' "$version" | grep -Eq '^[0-9]+(\.[0-9]+)*$' || fail "bad version: $version"

  file="codehead-$version-$target.tar.gz"
  base="https://github.com/$REPO/releases/download/v$version"
  work=$(mktemp -d "${TMPDIR:-/tmp}/codehead.XXXXXX")
  trap 'rm -rf "$work"' EXIT INT TERM

  printf 'codehead %s for %s\n' "$version" "$target"
  get -o "$work/$file" "$base/$file" || fail "no $target build in v$version"
  get -o "$work/sum" "$base/$file.sha256" || fail "no checksum file for $file"
  [ "$(awk '{print $1}' "$work/sum")" = "$(checksum "$work/$file")" ] || fail "checksum of $file does not match: not installed"
  good "sha256 matches"

  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    gh attestation verify "$work/$file" --repo "$owner_repo" >/dev/null 2>&1 || fail "attestation check of $file failed: not installed"
    good "attestation: built by github.com/$owner_repo"
  fi

  # 1 file in the archive, a regular file called codehead
  [ "$(tar -tzf "$work/$file")" = codehead ] || fail "$file holds more than the binary"
  tar -xzf "$work/$file" -C "$work"
  [ -f "$work/codehead" ] && [ ! -L "$work/codehead" ] || fail "$file: no regular codehead binary"
  chmod 0755 "$work/codehead"
  shown=$("$work/codehead" --version 2>/dev/null) || fail "the binary does not start on this machine"

  # copy next to the target, then rename over it: a running codehead keeps its old file
  mkdir -p "$DEST"
  cp "$work/codehead" "$DEST/.codehead-new.$$"
  mv -f "$DEST/.codehead-new.$$" "$DEST/codehead"
  good "$shown -> $DEST/codehead"

  case ":$PATH:" in
    *":$DEST:"*)
      found=$(command -v codehead || true)
      [ "$found" = "$DEST/codehead" ] || warn "$found is first on PATH: remove it to run this one"
      ;;
    *)
      warn "$DEST is not on PATH. Add it, for example:"
      case "${SHELL##*/}" in
        fish) say "fish_add_path $DEST" ;;
        *) say "export PATH=\"$DEST:\$PATH\"    # in ~/.${SHELL##*/}rc" ;;
      esac
      ;;
  esac

  found_harness=
  for h in claude codex opencode goose cursor-agent pi omp devin grok kimi amp hermes openclaw; do
    command -v "$h" >/dev/null 2>&1 && found_harness=1
  done
  [ -n "$found_harness" ] || warn "0 harnesses on PATH (claude, codex, opencode, goose, …): https://codehead.sh/docs/requirements/"

  # portless: dev servers at https://<worktree>.<app>.localhost, 1 per worktree (needs Node 24+)
  if command -v portless >/dev/null 2>&1; then
    good "portless: $(portless --version 2>/dev/null | head -1)"
  elif command -v npm >/dev/null 2>&1 && [ "$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)" -ge 24 ]; then
    say "installing portless (npm -g) …"
    if npm install -g portless >/dev/null 2>&1; then good "portless: installed"; else warn "npm install -g portless failed: https://github.com/vercel-labs/portless"; fi
  else
    warn "portless needs Node 24+ and npm: https://github.com/vercel-labs/portless (agents use \$CODEHEAD_PORT meanwhile)"
  fi

  # yazi: the file browser of the thread view (ctrl+y, files view y)
  if command -v yazi >/dev/null 2>&1; then
    good "yazi: $(yazi --version 2>/dev/null | head -1)"
  elif command -v brew >/dev/null 2>&1; then
    say "installing yazi (brew) …"
    if brew install yazi >/dev/null 2>&1; then good "yazi: installed"; else warn "brew install yazi failed: https://yazi-rs.github.io/docs/installation"; fi
  elif command -v cargo-binstall >/dev/null 2>&1; then
    say "installing yazi (cargo binstall) …"
    if cargo binstall -y yazi-build >/dev/null 2>&1; then good "yazi: installed"; else warn "cargo binstall yazi-build failed: https://yazi-rs.github.io/docs/installation"; fi
  else
    warn "yazi not found: install it for the file browser (https://yazi-rs.github.io/docs/installation)"
  fi
  printf '\nNext: run codehead in a git repository.\n'
}

install_codehead "$@"
