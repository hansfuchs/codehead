---
title: Installation
description: Install, update and remove codehead.
---

## Install script

```sh
curl -fsSL https://codehead.sh/install | sh
```

The script picks the release build for your system from
[GitHub releases](https://github.com/hansfuchs/codehead/releases), checks its sha256 (and its
build provenance, when `gh` is logged in) and puts `codehead` into `~/.local/bin`. No sudo.
`CODEHEAD_INSTALL_DIR` sets another directory.

A given version:

```sh
curl -fsSL https://codehead.sh/install | sh -s -- 0.2.31
```

## Updates

Every commit on `main` that passes CI is a release. codehead looks for a newer release once a
day in the background and prints 1 line when it quits. To update:

```sh
codehead --update
```

`CODEHEAD_NO_UPDATE_CHECK=1` turns the check off.

## From source

```sh
git clone https://github.com/hansfuchs/codehead
cd codehead
just install   # or: cargo install --locked --path .
```

## Uninstall

Stop running threads first (`codehead ctl <thread> '{"op":"stop"}'`, or let them idle out),
then remove the binary and, if you like, its data:

```sh
rm ~/.local/bin/codehead
rm -rf ~/.local/share/codehead ~/.config/codehead ~/.cache/codehead ~/.config/codehead.toml
```
