# codehead: rules for agents

`just check` must pass before each commit: rustfmt, clippy (warnings fail), tests, tidy.

## Tree

```text
src/
  main.rs      CLI dispatch only
  engine/      thread host, store, harnesses, MCP server, flows. 0 UI.
  infra/       OS + external tools: config, git, gh, file watcher, clipboard, editor. 0 UI.
  domain/      app data shared by features: agents, threads, notes, projects. 0 UI.
  ui/          shared widgets: textarea, dropdown, theme, markdown, avatar. 0 feature code.
  features/    1 folder per feature: chat, changes, files, merge, quick_menu, ...
  screens/     compose features: thread_view, start_screen, tutorial.
  perf/        perf suite (`just perf`).
tests/tidy/    repo rules that rustfmt and clippy cannot check.
```

Imports (checked by `tests/tidy`):
- `engine`, `infra`, `domain` → never `ui`, `features`, `screens`.
- `ui` → never `features`, `screens`.
- a feature → never another feature, never `screens`. Code 2 features need → `domain` (data) or `ui` (widget).
- `screens` → anything. A screen wires features together: it owns their state, routes keys, mouse and draw.

Folders:
- 1 folder per feature or screen. `mod.rs` holds its public surface (`pub(crate) use`). Other files private where possible.
- module names: snake_case, full words (`context_menu`, not `ctxmenu`).
- `mod.rs` style only: never `foo.rs` next to `foo/` (clippy `self_named_module_files`).

## Code

- file: max. 600 lines. Split by concern, never by line count.
- fn: max. 60 lines (blank and comment lines not counted).
- nesting: max. 3 blocks (clippy `excessive_nesting`; `impl` + `fn` count). Use early `return`, `let … else`, helper fns.
- names: 0 single-character identifiers except `i`, `j`, `x`, `y` (clippy `min_ident_chars`). `frame`, not `f`. `key`, not `k`.
- names say what the value holds or the fn does: `started` for an `Instant`, `modifiers` for key modifiers, `red, green, blue`. Never a fixed word per letter (`b` → `bytes` for a branch). 0 `_value` / `_2` suffixes, 0 words twice in a row, 0 key codes in fn names, 0 trailing `_` (tidy `names`).
- 0 `#[allow]`. 0 `#[expect]`. Fix the code.
- comments: explain why, never what. Structure goes into names, types and fns, not into `// section` comments.
- 1 state struct per concern. A struct field group (`// prompt`, `// stream`) → its own struct with its own `key`, `mouse`, `draw`.

Blank lines between blocks (checked + fixed by tidy):

1. after a multi-line block statement (`if`, `for`, `while`, `loop`, `match`, block, closure body)
2. before a multi-line block statement
3. between a group of `let` bindings and the statement after it
4. before a comment line that opens a group
5. before the tail expression of a fn with more than 5 statements
6. tests: between act and assert
7. max. 1 blank line in a row, 0 after `{` and before `}` (rustfmt)

```rust
fn mouse(&mut self, event: MouseEvent) {
    let at = Position::new(event.column, event.row);
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if let Some(overlay) = self.overlays.last_mut() {
        overlay.mouse(event);
        return;
    }

    if click && self.home_rect.contains(at) {
        self.exit = Exit::Back;
    }
}
```

## Tests

- tests live in their own file: `<feature>_tests.rs` or `<feature>_<topic>_tests.rs`, next to the code. 0 inline `mod tests { … }`.
- attach: `#[cfg(test)] #[path = "chat_ask_tests.rs"] mod tests;` in the tested file (or `mod chat_tests;` in `mod.rs`).
- UI interactions: view-level key + mouse tests with text in the prompt, drawn rows via `TestBackend`.

## Commands

```sh
just check                            # all checks, like CI
TIDY_FIX=1 cargo test --test tidy     # insert missing blank lines
cargo test --test tidy                # tree, sizes, imports, blank lines
just perf                             # UI budget: every case under 20 ms
just install                          # release build into ~/.local/bin
```
