---
title: Notes and aliases
description: Context from your notes folder, and short names for long paths.
---

## Notes

Notes are the `.md` files under the notes root (`notes` key, default: `notes/` of the codehead
app directory). On the new tab, `tab` to the notes field and `enter`: a folder tree with fuzzy
search over names and content. Picked notes go into the task as paths.

`codehead -c` copies the picked note paths to the clipboard and starts nothing.

## Path aliases

An alias is a short name for a directory:

```toml
[aliases]
"project:platform" = "~/projects/exampleorg/project/notes/platform/"
```

Aliases work as paths in the prompt: `project:platform/` lists the target directory. Prompts
keep the short alias; sent messages expand it to the full path. Edit aliases on the
**settings** tab (`ctrl+n` adds, `ctrl+d` removes).
