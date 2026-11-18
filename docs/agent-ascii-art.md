# agent ascii art

- 2 base styles, 7x4 cells (cols x rows). cell aspect 1:2 → ~square on screen (7:8)
- pools: `../sprites/pcb.sprites`, `../sprites/chip.sprites` (base as -000, then 100 variants, hand-curated), `../sprites/regi.sprites` (braille dot eyes in a round box, 100 variants: dot pattern, thin frame; each pattern also in the plain box)
- set: id prefix (`pcb`, `chip`, `regi`), 1 per file. agent form: `sprites` row picks the set, `sprite` row browses it, `r` random in the set
- stable pick (0 saved): `pcb` + `chip` only (`DEFAULT_SETS` in `src/sprites.rs`)
- regenerate pools: `just sprites --force` (`cargo run --example sprites`, overwrites curation; `--sheet`: contact sheet, `--out DIR`: try run)
- codehead compiles the pools in (`src/sprites.rs`): after curation, `cargo build --release`
- agent choice: `sprite` field of the agent file (`~/.config/codehead/agents/<name>.toml`), next to voice + instructions. 0 saved → stable pick by name

## PCB DAEMON

```
╮     ╭
╰┬───┬╯
 ◆─┼─◆ 
 ╰┴┴┴╯ 
```

## DIP CHIP

```
╮  ╷  ╭
╰┬─┴─┬╯
┤◆ ▾ ◆├
╰┴┬┴┬┴╯
```
