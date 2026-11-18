"""Agent sprite pools: 7x4 (cols x rows, ~square on screen) variants of 2 base styles.

Run: python3 agent_ascii_art.py [--force]
     python3 agent_ascii_art.py --sheet   (contact sheet of current pools, 10 per row)
- writes docs/agent-ascii-art.md (base styles)
- writes sprites/pcb.sprites + sprites/chip.sprites (base as -000, then 100 variants)
- pool files hold hand-curated state: no overwrite without --force
"""
import os, random, sys, unicodedata

W, H = 7, 4
BASE = {
    'pcb':  ["╮     ╭", "╰┬───┬╯", " ◆─┼─◆ ", " ╰┴┴┴╯ "],
    'chip': ["╮  ╷  ╭", "╰┬─┴─┬╯", "┤◆ ▾ ◆├", "╰┴┬┴┬┴╯"],
}

# Part alternatives. Index 0 = base. (value, family): L light, H heavy, D double.
# A sprite never mixes H and D parts.
EYES = [('◆', '◆'), ('◇', '◇'), ('●', '●'), ('○', '○'), ('◉', '◉'), ('■', '■'), ('□', '□'),
        ('▪', '▪'), ('◈', '◈'), ('⊙', '⊙'), ('⊗', '⊗'), ('⊕', '⊕'), ('▣', '▣'), ('◍', '◍'),
        ('◣', '◢'), ('◥', '◤'), ('╲', '╱'), ('◉', '○'), ('●', '○'), ('◆', '◇'), ('▴', '▴'), ('▾', '▾')]
EYES = [(e, 'L') for e in EYES]

def L(*xs): return [(x, 'L') for x in xs]

PCB = {
    'top': [(["╮     ╭", "╰┬───┬╯"], 'L'), (["┐     ┌", "└┬───┬┘"], 'L'), (["╮  ╷  ╭", "╰┬─┴─┬╯"], 'L'),
            (["╗     ╔", "╚╤═══╤╝"], 'D'), (["┓     ┏", "┗┳━━━┳┛"], 'H'), (["╲     ╱", "╰┬───┬╯"], 'L'),
            (["╮╷   ╷╭", "╰┼───┼╯"], 'L'), (["╮     ╭", "╰┬─┬─┬╯"], 'L'), (["╮     ╭", "╰╥───╥╯"], 'D'),
            (["╮  ┃  ╭", "╰┬─┸─┬╯"], 'H'), (["╮     ╭", "─┬───┬─"], 'L'), (["╮     ╭", "╰┬╌╌╌┬╯"], 'L'),
            (["╭╴   ╶╮", "╰┬───┬╯"], 'L'), (["╲╷   ╷╱", "╰┼───┼╯"], 'L'), (["╮  ┊  ╭", "╰┬─┴─┬╯"], 'L'),
            (["╮     ╭", "╰┲━━━┱╯"], 'H'), (["╮ ╷ ╷ ╭", "╰┬┴─┴┬╯"], 'L'), (["╲╲   ╱╱", "╰┬───┬╯"], 'L'),
            (["╮     ╭", "╰┬┄┄┄┬╯"], 'L'), (["╮  ╳  ╭", "╰┬───┬╯"], 'L')],
    'cheek': L((' ', ' '), ('╶', '╴'), ('─', '─'), ('·', '·')) + [(('━', '━'), 'H')],
    'eyes': EYES,
    'bridge': L(('─', '─'), (' ', ' '), ('╌', '╌'), ('┄', '┄'), ('·', '·'), ('╶', '╴'))
              + [(('━', '━'), 'H'), (('═', '═'), 'D')],
    'nose': L(*'┼┬┴│▾▿╳·') + [('╋', 'H'), ('╪', 'D'), ('╫', 'D')],
    'jaw': L(" ╰┴┴┴╯ ", " ╰┬┬┬╯ ", " └┴┴┴┘ ", " ╰┴┬┴╯ ", " ╰▾▾▾╯ ", " ╰┴▾┴╯ ", " ╰───╯ ",
             " ╰┼┼┼╯ ", "╶╰┴┴┴╯╴", " ╰┴─┴╯ ", " ╰▿▿▿╯ ", " ╰╌╌╌╯ ", " ╰╳╳╳╯ ", " ╰─┴─╯ ",
             " ┴┴┴┴┴ ", " ╰┴┼┴╯ ", " ╰┬─┬╯ ")
           + [(" ╰┸┸┸╯ ", 'H'), (" ┗┻┻┻┛ ", 'H'), (" ╚╩╩╩╝ ", 'D'), (" ╰╨╨╨╯ ", 'D')],
}

CHIP = {
    'top': [(["╮  ╷  ╭", "╰┬─┴─┬╯"], 'L'), (["╮     ╭", "╰┬───┬╯"], 'L'), (["╮  ╽  ╭", "╰┬─┸─┬╯"], 'H'),
            (["┐  ╷  ┌", "└┬─┴─┬┘"], 'L'), (["╮ ╷ ╷ ╭", "╰┬┴─┴┬╯"], 'L'), (["╮╷ ╷ ╷╭", "╰┴─┴─┴╯"], 'L'),
            (["╲  ╷  ╱", "╰┬─┴─┬╯"], 'L'), (["╗  ╷  ╔", "╚╤═╧═╤╝"], 'D'), (["┓  ╻  ┏", "┗┳━┻━┳┛"], 'H'),
            (["╮  ┊  ╭", "╰┬─┴─┬╯"], 'L'), (["╮  ◦  ╭", "╰┬─┴─┬╯"], 'L'), (["╮  ╳  ╭", "╰┬───┬╯"], 'L'),
            (["╭╴ ╷ ╶╮", "╰┬─┴─┬╯"], 'L'), (["╮  ╷  ╭", "╰┲━┷━┱╯"], 'H'), (["╮  ╷  ╭", "╰┬╌┴╌┬╯"], 'L'),
            (["╮  ╷  ╭", "╰┬─┼─┬╯"], 'L')],
    'pins': L(('┤', '├'), ('│', '│')) + [(('╡', '╞'), 'D'), (('╢', '╟'), 'D'), (('┨', '┠'), 'H'), (('┥', '┝'), 'H')],
    'eyes': EYES,
    'gap': L((' ', ' '), ('─', '─'), ('╌', '╌'), ('┄', '┄'), ('·', '·'), ('╶', '╴'))
           + [(('═', '═'), 'D'), (('━', '━'), 'H')],
    'nose': L(*'▾▿▼┼╳·┴│┬') + [('═', 'D'), ('╋', 'H')],
    'chin': L("╰┴┬┴┬┴╯", "└┴┬┴┬┴┘", "╰┬┬┬┬┬╯", "╰┴┴┴┴┴╯", "╰┬┴┬┴┬╯", "╰─┬─┬─╯", "╰┴┼┴┼┴╯",
              "╰┴┬▾┬┴╯", "╰┴▾▾▾┴╯", "╰┴┬─┬┴╯", "╰─┴┬┴─╯", "╰┴┬┬┬┴╯", "╰┴╌╌╌┴╯", "╰┬─┴─┬╯")
            + [("╰┴╥┴╥┴╯", 'D'), ("╚╧╤╧╤╧╝", 'D'), ("┗┷┯┷┯┷┛", 'H')],
}

def build_pcb(p):
    top, cheek, eyes, bridge, nose, jaw = p
    return [top[0], top[1], cheek[0] + eyes[0] + bridge[0] + nose + bridge[1] + eyes[1] + cheek[1], jaw]

def build_chip(p):
    top, pins, eyes, gap, nose, chin = p
    return [top[0], top[1], pins[0] + eyes[0] + gap[0] + nose + gap[1] + eyes[1] + pins[1], chin]

STYLES = {'pcb': (PCB, build_pcb), 'chip': (CHIP, build_chip)}

def variants(key, n=100, seed=666):
    parts, build = STYLES[key]
    names = list(parts)
    rng = random.Random(f'{seed}-{key}')
    base = build([parts[k][0][0] for k in names])
    assert base == BASE[key], (key, base)
    seen, out = {tuple(base)}, []
    while len(out) < n:
        k = rng.choices([1, 2, 3], weights=[35, 45, 20])[0]
        change = set(rng.sample(names, k))
        pick = [rng.choice(parts[x][1:]) if x in change else parts[x][0] for x in names]
        if len({f for _, f in pick} - {'L'}) > 1:
            continue
        rows = build([v for v, _ in pick])
        if tuple(rows) in seen:
            continue
        seen.add(tuple(rows)); out.append(rows)
    return [base] + out

def check(rows):
    assert len(rows) == H, rows
    for r in rows:
        assert len(r) == W, (r, len(r))
        assert all(unicodedata.east_asian_width(c) not in ('W', 'F') for c in r), r

POOL_HEAD = '''# agent sprite pool: {name}
# format: id line, then 4 rows of 7 cells, blank line between sprites.
# curate: delete a whole block to drop a sprite. trailing spaces optional (parser pads to 7).
'''

def write_pool(path, key, force):
    if os.path.exists(path) and not force:
        print(f'skip {path}: exists (curated state). use --force to regenerate')
        return
    blocks = []
    for i, rows in enumerate(variants(key)):
        check(rows)
        blocks.append(f'{key}-{i:03d}\n' + '\n'.join(rows))
    open(path, 'w').write(POOL_HEAD.format(name=key) + '\n' + '\n\n'.join(blocks) + '\n')
    print(f'wrote {path}: {len(blocks)} sprites')

def sheet(path, per=10):
    sp = [b.split('\n') for b in open(path).read().split('\n\n') if b.strip() and not b.startswith('#')]
    for i in range(0, len(sp), per):
        g = sp[i:i + per]
        print('  '.join(s[0].split('-')[-1].ljust(W) for s in g))
        for r in range(1, H + 1):
            print('  '.join(s[r].ljust(W) for s in g))
        print()

MD = '''# agent ascii art

- 2 base styles, 7x4 cells (cols x rows). cell aspect 1:2 → ~square on screen (7:8)
- pools: `../sprites/pcb.sprites`, `../sprites/chip.sprites` (base as -000, then 100 variants, hand-curated)
- regenerate pools: `python3 agent_ascii_art.py --force` (overwrites curation)
- codehead compiles the pools in (`src/sprites.rs`): after curation, `cargo build --release`
- agent choice: `sprite` field of the agent file (`~/.config/codehead/agents/<name>.toml`), next to voice + instructions. 0 saved → stable pick by name

## PCB DAEMON

```
{pcb}
```

## DIP CHIP

```
{chip}
```
'''

if __name__ == '__main__':
    here = os.path.dirname(os.path.abspath(__file__))
    pool = lambda key: os.path.join(here, '..', 'sprites', f'{key}.sprites')
    if '--sheet' in sys.argv:
        for key in STYLES:
            sheet(pool(key))
        sys.exit()
    open(os.path.join(here, 'agent-ascii-art.md'), 'w').write(
        MD.format(pcb='\n'.join(BASE['pcb']), chip='\n'.join(BASE['chip'])))
    for key in STYLES:
        write_pool(pool(key), key, '--force' in sys.argv)
