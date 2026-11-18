//! Agent sprite pools: 7x4 (cols x rows, ~square on screen) variants of 2 base styles.
//!
//! ```sh
//! cargo run --example sprites            # write sprites/pcb.sprites + sprites/chip.sprites if missing
//! cargo run --example sprites -- --force # regenerate them (overwrites the curation)
//! cargo run --example sprites -- --sheet # contact sheet of the current pools, 10 per row
//! cargo run --example sprites -- --force --out /tmp/pools  # a try run into another dir
//! ```
//!
//! Pool files hold hand-curated state: 0 overwrite without --force. The pool format: an id line,
//! then 4 rows of 7 cells, a blank line between sprites (`src/sprites.rs` reads it).

use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthChar;

const W: usize = 7;
const H: usize = 4;
/// Variants per style (the base is -000).
const N: usize = 100;
const SEED: &str = "666";

/// Part family: light, heavy, double. A sprite never mixes heavy and double parts.
#[derive(Clone, Copy, PartialEq)]
enum Fam {
    L,
    H,
    D,
}
use Fam::{D, H as Hv, L};

/// 1 choice of a part: its strings (top: 2 rows; a pair: left, right; nose, jaw: 1), its family.
type Opt = (Vec<&'static str>, Fam);

fn opts(v: &[(&[&'static str], Fam)]) -> Vec<Opt> {
    v.iter().map(|(s, f)| (s.to_vec(), *f)).collect()
}

fn light(v: &[&[&'static str]]) -> Vec<Opt> {
    v.iter().map(|s| (s.to_vec(), L)).collect()
}

fn chars(s: &'static str, f: Fam) -> Vec<Opt> {
    // 1 option per char: slices of S
    s.char_indices().map(|(i, c)| (vec![&s[i..i + c.len_utf8()]], f)).collect()
}

fn eyes() -> Vec<Opt> {
    light(&[
        &["◆", "◆"], &["◇", "◇"], &["●", "●"], &["○", "○"], &["◉", "◉"], &["■", "■"], &["□", "□"], &["▪", "▪"], &["◈", "◈"], &["⊙", "⊙"], &["⊗", "⊗"],
        &["⊕", "⊕"], &["▣", "▣"], &["◍", "◍"], &["◣", "◢"], &["◥", "◤"], &["╲", "╱"], &["◉", "○"], &["●", "○"], &["◆", "◇"], &["▴", "▴"], &["▾", "▾"],
    ])
}

/// A style: its parts (name, options; option 0: the base), its base rows.
struct Style {
    key: &'static str,
    parts: Vec<(&'static str, Vec<Opt>)>,
    base: [&'static str; H],
}

impl Style {
    /// The rows of 1 pick per part (parts in order: top, side, eyes, gap, nose, bottom).
    fn build(&self, p: &[&Opt]) -> Vec<String> {
        let (top, side, eyes, gap, nose, bottom) = (&p[0].0, &p[1].0, &p[2].0, &p[3].0, &p[4].0, &p[5].0);
        vec![top[0].to_string(), top[1].to_string(), [side[0], eyes[0], gap[0], nose[0], gap[1], eyes[1], side[1]].concat(), bottom[0].to_string()]
    }
}

fn pcb() -> Style {
    let mut nose = chars("┼┬┴│▾▿╳·", L);
    nose.extend(opts(&[(&["╋"], Hv), (&["╪"], D), (&["╫"], D)]));
    let mut jaw = light(&[
        &[" ╰┴┴┴╯ "], &[" ╰┬┬┬╯ "], &[" └┴┴┴┘ "], &[" ╰┴┬┴╯ "], &[" ╰▾▾▾╯ "], &[" ╰┴▾┴╯ "], &[" ╰───╯ "], &[" ╰┼┼┼╯ "], &["╶╰┴┴┴╯╴"], &[" ╰┴─┴╯ "],
        &[" ╰▿▿▿╯ "], &[" ╰╌╌╌╯ "], &[" ╰╳╳╳╯ "], &[" ╰─┴─╯ "], &[" ┴┴┴┴┴ "], &[" ╰┴┼┴╯ "], &[" ╰┬─┬╯ "],
    ]);
    jaw.extend(opts(&[(&[" ╰┸┸┸╯ "], Hv), (&[" ┗┻┻┻┛ "], Hv), (&[" ╚╩╩╩╝ "], D), (&[" ╰╨╨╨╯ "], D)]));
    let mut cheek = light(&[&[" ", " "], &["╶", "╴"], &["─", "─"], &["·", "·"]]);
    cheek.extend(opts(&[(&["━", "━"], Hv)]));
    let mut bridge = light(&[&["─", "─"], &[" ", " "], &["╌", "╌"], &["┄", "┄"], &["·", "·"], &["╶", "╴"]]);
    bridge.extend(opts(&[(&["━", "━"], Hv), (&["═", "═"], D)]));
    let top = opts(&[
        (&["╮     ╭", "╰┬───┬╯"], L), (&["┐     ┌", "└┬───┬┘"], L), (&["╮  ╷  ╭", "╰┬─┴─┬╯"], L), (&["╗     ╔", "╚╤═══╤╝"], D),
        (&["┓     ┏", "┗┳━━━┳┛"], Hv), (&["╲     ╱", "╰┬───┬╯"], L), (&["╮╷   ╷╭", "╰┼───┼╯"], L), (&["╮     ╭", "╰┬─┬─┬╯"], L),
        (&["╮     ╭", "╰╥───╥╯"], D), (&["╮  ┃  ╭", "╰┬─┸─┬╯"], Hv), (&["╮     ╭", "─┬───┬─"], L), (&["╮     ╭", "╰┬╌╌╌┬╯"], L),
        (&["╭╴   ╶╮", "╰┬───┬╯"], L), (&["╲╷   ╷╱", "╰┼───┼╯"], L), (&["╮  ┊  ╭", "╰┬─┴─┬╯"], L), (&["╮     ╭", "╰┲━━━┱╯"], Hv),
        (&["╮ ╷ ╷ ╭", "╰┬┴─┴┬╯"], L), (&["╲╲   ╱╱", "╰┬───┬╯"], L), (&["╮     ╭", "╰┬┄┄┄┬╯"], L), (&["╮  ╳  ╭", "╰┬───┬╯"], L),
    ]);
    Style { key: "pcb", parts: vec![("top", top), ("cheek", cheek), ("eyes", eyes()), ("bridge", bridge), ("nose", nose), ("jaw", jaw)], base: ["╮     ╭", "╰┬───┬╯", " ◆─┼─◆ ", " ╰┴┴┴╯ "] }
}

fn chip() -> Style {
    let mut nose = chars("▾▿▼┼╳·┴│┬", L);
    nose.extend(opts(&[(&["═"], D), (&["╋"], Hv)]));
    let mut chin = light(&[
        &["╰┴┬┴┬┴╯"], &["└┴┬┴┬┴┘"], &["╰┬┬┬┬┬╯"], &["╰┴┴┴┴┴╯"], &["╰┬┴┬┴┬╯"], &["╰─┬─┬─╯"], &["╰┴┼┴┼┴╯"],
        &["╰┴┬▾┬┴╯"], &["╰┴▾▾▾┴╯"], &["╰┴┬─┬┴╯"], &["╰─┴┬┴─╯"], &["╰┴┬┬┬┴╯"], &["╰┴╌╌╌┴╯"], &["╰┬─┴─┬╯"],
    ]);
    chin.extend(opts(&[(&["╰┴╥┴╥┴╯"], D), (&["╚╧╤╧╤╧╝"], D), (&["┗┷┯┷┯┷┛"], Hv)]));
    let mut pins = light(&[&["┤", "├"], &["│", "│"]]);
    pins.extend(opts(&[(&["╡", "╞"], D), (&["╢", "╟"], D), (&["┨", "┠"], Hv), (&["┥", "┝"], Hv)]));
    let mut gap = light(&[&[" ", " "], &["─", "─"], &["╌", "╌"], &["┄", "┄"], &["·", "·"], &["╶", "╴"]]);
    gap.extend(opts(&[(&["═", "═"], D), (&["━", "━"], Hv)]));
    let top = opts(&[
        (&["╮  ╷  ╭", "╰┬─┴─┬╯"], L), (&["╮     ╭", "╰┬───┬╯"], L), (&["╮  ╽  ╭", "╰┬─┸─┬╯"], Hv), (&["┐  ╷  ┌", "└┬─┴─┬┘"], L),
        (&["╮ ╷ ╷ ╭", "╰┬┴─┴┬╯"], L), (&["╮╷ ╷ ╷╭", "╰┴─┴─┴╯"], L), (&["╲  ╷  ╱", "╰┬─┴─┬╯"], L), (&["╗  ╷  ╔", "╚╤═╧═╤╝"], D),
        (&["┓  ╻  ┏", "┗┳━┻━┳┛"], Hv), (&["╮  ┊  ╭", "╰┬─┴─┬╯"], L), (&["╮  ◦  ╭", "╰┬─┴─┬╯"], L), (&["╮  ╳  ╭", "╰┬───┬╯"], L),
        (&["╭╴ ╷ ╶╮", "╰┬─┴─┬╯"], L), (&["╮  ╷  ╭", "╰┲━┷━┱╯"], Hv), (&["╮  ╷  ╭", "╰┬╌┴╌┬╯"], L), (&["╮  ╷  ╭", "╰┬─┼─┬╯"], L),
    ]);
    Style { key: "chip", parts: vec![("top", top), ("pins", pins), ("eyes", eyes()), ("gap", gap), ("nose", nose), ("chin", chin)], base: ["╮  ╷  ╭", "╰┬─┴─┬╯", "┤◆ ▾ ◆├", "╰┴┬┴┬┴╯"] }
}

/// splitmix64: the same seed, the same pools.
struct Rng(u64);

impl Rng {
    fn new(seed: &str) -> Rng {
        // FNV-1a of the seed text
        Rng(seed.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 0..N
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The base, then N variants: 1-3 parts changed (weights 35 / 45 / 20), 0 heavy + double mix, 0 repeats.
fn variants(s: &Style) -> Vec<Vec<String>> {
    let mut rng = Rng::new(&format!("{SEED}-{}", s.key));
    let base: Vec<&Opt> = s.parts.iter().map(|(_, o)| &o[0]).collect();
    let base = s.build(&base);
    assert_eq!(base, s.base, "{}: base parts build the base", s.key);
    let mut seen = std::collections::HashSet::from([base.clone()]);
    let mut out = vec![base];
    while out.len() <= N {
        let roll = rng.below(100);
        let k = if roll < 35 { 1 } else if roll < 80 { 2 } else { 3 };
        let mut change: Vec<usize> = (0..s.parts.len()).collect();
        for i in 0..k {
            let j = i + rng.below(change.len() - i);
            change.swap(i, j);
        }
        change.truncate(k);
        let pick: Vec<&Opt> = s.parts.iter().enumerate().map(|(i, (_, o))| if change.contains(&i) { &o[1 + rng.below(o.len() - 1)] } else { &o[0] }).collect();
        let fams: std::collections::HashSet<u8> = pick.iter().filter(|o| o.1 != L).map(|o| o.1 as u8).collect();
        if fams.len() > 1 {
            continue;
        }
        let rows = s.build(&pick);
        if seen.insert(rows.clone()) {
            out.push(rows);
        }
    }
    out
}

/// ROWS: 4 rows of 7 cells, 0 wide chars.
fn check(rows: &[String]) {
    assert_eq!(rows.len(), H, "{rows:?}");
    for r in rows {
        assert_eq!(r.chars().count(), W, "{r:?}");
        assert!(r.chars().all(|c| c.width() == Some(1)), "{r:?}: a wide or 0-width char");
    }
}

fn write_pool(path: &Path, s: &Style, force: bool) {
    if path.exists() && !force {
        println!("skip {}: exists (curated state). use --force to regenerate", path.display());
        return;
    }
    let blocks: Vec<String> = variants(s)
        .iter()
        .enumerate()
        .map(|(i, rows)| {
            check(rows);
            format!("{}-{i:03}\n{}", s.key, rows.join("\n"))
        })
        .collect();
    let head = format!(
        "# agent sprite pool: {}\n# format: id line, then 4 rows of 7 cells, blank line between sprites.\n# curate: delete a whole block to drop a sprite. trailing spaces optional (parser pads to 7).\n",
        s.key
    );
    std::fs::write(path, format!("{head}\n{}\n", blocks.join("\n\n"))).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    println!("wrote {}: {} sprites", path.display(), blocks.len());
}

/// The pool at PATH, PER sprites a row: the number above each.
fn sheet(path: &Path, per: usize) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let sprites: Vec<Vec<&str>> = text.split("\n\n").filter(|b| !b.trim().is_empty() && !b.starts_with('#')).map(|b| b.lines().collect()).collect();
    let pad = |s: &str| format!("{s}{}", " ".repeat(W.saturating_sub(s.chars().count())));
    for group in sprites.chunks(per) {
        println!("{}", group.iter().map(|s| pad(s[0].rsplit('-').next().unwrap_or_default())).collect::<Vec<_>>().join("  "));
        for r in 1..=H {
            println!("{}", group.iter().map(|s| pad(s.get(r).copied().unwrap_or_default())).collect::<Vec<_>>().join("  "));
        }
        println!();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // --out DIR: the pools go there (a try run), else sprites/ of the repo
    let dir = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sprites"));
    let pool = |key: &str| dir.join(format!("{key}.sprites"));
    let styles = [pcb(), chip()];
    if args.iter().any(|a| a == "--sheet") {
        for s in &styles {
            sheet(&pool(s.key), 10);
        }
        return;
    }
    let force = args.iter().any(|a| a == "--force");
    for s in &styles {
        write_pool(&pool(s.key), s, force);
    }
}
