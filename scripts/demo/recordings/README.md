# recording scripts

1 Markdown file = 1 video: `<name>.md` → `site/public/demo/<name>.mp4`. Files that start with
`_` hold shared turns only (`_story.md`). Change a file, then record again:

```sh
just demo stream                                                      # 1 video
~/.cache/codehead-demo/venv/bin/python scripts/demo/demo.py --explore stream   # 4x, PNGs, 0 video
```

## Frontmatter

```yaml
---
label: live stream          # tab name on the website
order: 1                    # tab order
caption: EVA fixes the bug  # text below the video
ctx: EVA=64000, HAL=24000  # context tokens per agent at the start (default 30000)
font: 26                    # px per cell row (default 17: about 190 columns; 26: about 120)
---
```

## Sections

| section | content |
| --- | --- |
| `# seed <id> · <title>` | a thread that exists before the video; next line: `agents a, b · branch x · worktree /path · state done · age 1800 · span 300` (age, span in s) |
| `# live` | the turns the fake harness plays during the video, in order per agent |
| `# turns` | named turns for other files, 0 effect here |
| `# keys` | 1 fenced block: what the owner does |

## Turns

A `##` heading starts a turn or a message:

- `## EVA`: a turn of agent EVA, its steps below
- `## owner`: an owner message (seeds), its text below
- `## HAL → EVA`: a message of 1 agent to another (seeds), its text below

Options after ` · `: `name X` (reuse it elsewhere), `use X` (the steps or text of turn X),
`when <regex>` (live: plays when the prompt matches), `cost 0.38` (USD), `delay 1.2` (s
before the 1st step).

## Steps

A `###` heading per step. Options after ` · `: a duration (`300ms`, `7.4s`), `fails`, `asks`
(the agent's mode decides; a supervised agent asks).

| step | body |
| --- | --- |
| `### think` | the thought |
| `### say` | the Markdown answer, streamed |
| `### read <path>` | 1 fenced block: the file |
| `### grep <pattern> · in <path>` | 1 fenced block: the matches |
| `### bash <description>` | a `sh` block: the command, then a block: the output |
| `### edit <path>` | an `old` block, a `new` block |
| `### write <path>` | 1 fenced block: the content |
| `### send HAL, ENOCH` | the message (codehead `send`: the agent gets it) |
| `### status blocked` | the note (`working`, `blocked`, `done`) |
| `### question` | per question `#### <text> · <header> [· multi]`, then `- label: description` rows |
| `### fail` | the error the turn ends with (`You've hit your limit …`: a usage limit) |
| `### sleep 2` | |

Paths are relative to the repository `/home/dev/projects/payd`. `{{fix}}` and `{{overflow}}`
become the hashes of the 2 commits on `fix/stop-the-410-retry-storm`.

## Keys

1 command per line, `#` starts a comment:

| command | effect |
| --- | --- |
| `wait '<regex>' [timeout s]` | until the screen shows it (default 30 s) |
| `sleep 1.5` | |
| `type "text\n"` | types like a person; `\n`: enter |
| `key ctrl+enter [n] [gap s]` | `enter tab esc up down left right space backspace f2 pgup pgdn`, `ctrl+<letter>`, `ctrl+left` … |
| `click '<regex>' [dx]` | the pointer glides to the match (+dx columns) and clicks |
| `wheel up 6` | scroll at the pointer |
| `shot <label>` | `--explore`: a PNG and the screen text |

Use single quotes for regexes: `'Go ahead\?'`.
