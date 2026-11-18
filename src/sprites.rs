//! Agent sprites: 7x4 glyph portraits. Pool: sprites/*.sprites (compiled in, hand-curated).
//! Set: the id prefix (`pcb` of `pcb-042`), 1 per file. The agent form picks a set, then a sprite of it.
//! Choice per agent: its profile (agents::Profile), else a stable pick by name (DEFAULT_SET).
//! New agent: a random sprite of DEFAULT_SET.

use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

pub const W: usize = 7;
pub const H: usize = 4;

const POOLS: [&str; 3] = [include_str!("../sprites/pcb.sprites"), include_str!("../sprites/chip.sprites"), include_str!("../sprites/regi.sprites")];

/// Set of the stable pick (agents without a saved sprite) and of the random pick (new agents).
pub const DEFAULT_SET: &str = "regi";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprite {
    /// Pool id (`pcb-042`), or `custom`.
    pub id: String,
    pub rows: [String; H],
}

/// Pad / cut a row to W cells.
fn fit(row: &str) -> String {
    let mut out = String::new();
    let mut w = 0;
    for c in row.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > W {
            break;
        }
        out.push(c);
        w += cw;
    }
    out + &" ".repeat(W - w)
}

pub fn from_text(id: &str, text: &str) -> Sprite {
    let mut lines = text.lines();
    Sprite { id: id.to_string(), rows: std::array::from_fn(|_| fit(lines.next().unwrap_or(""))) }
}

/// Blocks: id line + H rows, blank line between. `#` lines: comments. Other blocks: skipped.
fn parse(text: &str) -> Vec<Sprite> {
    let mut out = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    for l in text.lines().filter(|l| !l.starts_with('#')).chain([""]) {
        if !l.trim().is_empty() {
            block.push(l);
            continue;
        }
        if block.len() == H + 1 {
            out.push(from_text(block[0].trim(), &block[1..].join("\n")));
        }
        block.clear();
    }
    out
}

pub fn pool() -> &'static [Sprite] {
    static POOL: OnceLock<Vec<Sprite>> = OnceLock::new();
    POOL.get_or_init(|| POOLS.iter().flat_map(|t| parse(t)).collect())
}

pub fn index_of(id: &str) -> Option<usize> {
    pool().iter().position(|s| s.id == id)
}

/// Set of a pool id: the part before the last `-`. None: `custom`.
pub fn set_of(id: &str) -> Option<&str> {
    id.rsplit_once('-').map(|(set, _)| set)
}

/// Set names in pool order.
pub fn sets() -> &'static [String] {
    static SETS: OnceLock<Vec<String>> = OnceLock::new();
    SETS.get_or_init(|| {
        let mut out: Vec<String> = Vec::new();
        for s in pool() {
            if let Some(set) = set_of(&s.id)
                && !out.iter().any(|x| x == set)
            {
                out.push(set.to_string());
            }
        }
        out
    })
}

/// Pool indices of SET, in pool order.
pub fn members(set: &str) -> Vec<usize> {
    pool().iter().enumerate().filter(|(_, s)| set_of(&s.id) == Some(set)).map(|(i, _)| i).collect()
}

/// FNV-1a: stable across builds (std hashers are not).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Pool sprite for an agent without a saved one: a stable pick from DEFAULT_SET.
pub fn default_for(name: &str) -> Sprite {
    let m = members(DEFAULT_SET);
    pool()[m[(fnv(name) % m.len() as u64) as usize]].clone()
}

/// Random pool index of SET (all sets: None), never CURRENT (when the choice has more than 1 sprite).
pub fn random(set: Option<&str>, current: Option<usize>) -> usize {
    let choice: Vec<usize> = match set {
        Some(set) => members(set),
        None => (0..pool().len()).collect(),
    };
    let n = choice.len();
    if n == 0 {
        return current.unwrap_or(0);
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let mut x = nanos ^ 0x9e3779b97f4a7c15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let k = (x % n as u64) as usize;
    if Some(choice[k]) == current && n > 1 { choice[(k + 1) % n] } else { choice[k] }
}

/// H lines in COLOR, SIDE[i] appended to row i.
pub fn lines(s: &Sprite, color: Color, side: Vec<Vec<Span<'static>>>) -> Vec<Line<'static>> {
    let mut side = side.into_iter();
    s.rows
        .iter()
        .map(|r| {
            let mut spans = vec![Span::styled(r.clone(), Style::default().fg(color))];
            spans.extend(side.next().unwrap_or_default());
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn pools_parse() {
        let p = pool();
        assert!(p.len() >= 2, "{}", p.len());
        assert_eq!(p[0].id, "pcb-000");
        assert_eq!(p[0].rows[2], " ◆─┼─◆ ");
        assert!(p.iter().any(|s| s.id == "chip-000"));
        assert!(p.iter().all(|s| s.rows.iter().all(|r| r.width() == W)));
        let regi = members("regi");
        assert_eq!(regi.len(), 100);
        for (n, &i) in regi.iter().enumerate() {
            assert_eq!(p[i].id, format!("regi-{n:03}"));
        }
        let portraits: std::collections::BTreeSet<_> = regi.iter().map(|&i| &p[i].rows).collect();
        assert_eq!(portraits.len(), 100, "regi portraits must be distinct");
        let rows: Vec<_> = POOLS[2].lines().filter(|line| !line.starts_with('#') && !line.trim().is_empty()).collect();
        assert_eq!(rows.len(), 100 * (H + 1));
        assert!(rows.as_chunks::<{ H + 1 }>().0.iter().all(|block| block[1..].iter().all(|row| row.width() == W)));
    }

    #[test]
    fn sets_in_pool_order() {
        assert_eq!(sets(), ["pcb", "chip", "regi"]);
        assert_eq!(set_of("pcb-042"), Some("pcb"));
        assert_eq!(set_of("custom"), None);
        let m = members("chip");
        assert!(m.iter().all(|&i| pool()[i].id.starts_with("chip-")));
    }

    #[test]
    fn random_stays_in_set() {
        let m = members("regi");
        for _ in 0..50 {
            let i = random(Some("regi"), Some(m[0]));
            assert!(m.contains(&i) && i != m[0]);
        }
    }

    #[test]
    fn default_is_regi() {
        for k in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            assert_eq!(set_of(&default_for(k).id), Some(DEFAULT_SET));
        }
    }

    #[test]
    fn regi_frames_drop_single_lines_and_open_corners() {
        for &i in &members("regi") {
            let r = &pool()[i].rows;
            assert!(r[0].starts_with('╭') && r[0].ends_with('╮'), "{}: open corners", pool()[i].id);
            assert!(r[3].starts_with('╰') && r[3].ends_with('╯'), "{}: open corners", pool()[i].id);
            for row in [&r[0], &r[3]] {
                let inner: String = row.chars().skip(1).take(W - 2).collect();
                assert_ne!(inner.chars().filter(|c| matches!(c, '┬' | '┴')).count(), 1, "{}: single line", pool()[i].id);
            }
        }
    }

    #[test]
    fn parse_tolerates_curation() {
        // trailing spaces stripped, a block deleted, extra blank lines
        let t = "# c\n\na-1\n╮     ╭\n╰┬───┬╯\n ◆─┼─◆\n ╰┴┴┴╯\n\n\n\nbad\nx\n\na-2\n1\n2\n3\n4\n";
        let p = parse(t);
        assert_eq!(p.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a-1", "a-2"]);
        assert_eq!(p[0].rows[2], " ◆─┼─◆ ");
    }

    #[test]
    fn default_is_stable_and_rows_fit() {
        assert_eq!(default_for("abc"), default_for("abc"));
        let s = from_text("custom", "  /\\\n <oo>\n  ||\n");
        assert_eq!(s.rows, ["  /\\   ", " <oo>  ", "  ||   ", "       "]);
        assert_eq!(from_text("x", "123456789").rows[0], "1234567");
    }
}
