---
title: Shortcuts
description: "#name in a prompt: a text you wrote once, sent in full."
---

A shortcut is a name for a text you send often. Type `#review` in any prompt and it stands for
the text behind it, for example:

```text
bring in another agent to review the code you wrote and have it write comments for each finding
```

A new install has 1 shortcut, `#review`, with that text.

## In the prompt

- `#` opens a dropdown of your shortcuts, filtered as you type the name. A 2nd box beside it
  shows the text of the shortcut under the cursor.
- `↑` `↓` put `#name` into the prompt at once: a live preview.
- `tab` / `enter` put the text in place of `#name`.
- A `#name` left in the prompt expands when you send. The agents get the text, your prompt
  history keeps the short name.
- The mouse over a `#name` (or the cursor in it) shows its text in a popup.

Shortcuts mix with everything else in a prompt: `@HAL #review` sends the review text to HAL,
`>sub #review` starts a [sub-thread](/docs/concepts/threads/#sub-threads) with it.

## Edit them

**settings** → **shortcuts**: the list left, the text right.

| Key | Action |
| --- | --- |
| `↑` `↓`, `j` `k` | pick a shortcut |
| `n`, `enter` on **+ add** | a new shortcut |
| `enter`, `→` | edit the text |
| `r` | rename |
| `d d` | delete |
| `ctrl+s`, `ctrl+enter` | save, back to the list |
| `esc` | back; edited text: `esc` again within 2 s drops the edits |

A name takes letters, digits, `_` and `-`.

## The file

The shortcuts live in `~/.config/codehead/shortcuts.toml`, 1 key per shortcut. codehead reads
the file again when it changes, so an edit in your editor applies at once.

```toml
review = '''
bring in another agent to review the code you wrote and have it write comments for each finding
'''
tests = '''
run the tests. fix what fails. report the counts.
'''
```
