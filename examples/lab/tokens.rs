//! Tokens page: the tokens box of the thread header (cost.rs, compact) now and with line bars
//! (━ used, ─ rest, after the ctx / tok lines of the big page). Each box at its real size in the
//! header: 3 agents (62 wide, 7 rows) and 1 agent (44 wide, 4 rows). Mock numbers; REGI's context
//! and the 5h window grow, the next message's share pulses.
//!
//! 1 line per agent for context: the context % is the context tokens over the model's window, so
//! a 2nd bar for those tokens has the same length. The tok column (all turns) has 0 max: a number.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::glyph::{self, BG, CORAL, INK, MUTED, Rgb, WARN};
use super::write;

/// The next message's share (cost.rs: info).
const INFO: Rgb = (120, 170, 230);

/// The rest of a line bar (big.rs: bar).
fn track() -> Rgb {
    glyph::mix(BG, MUTED, 0.4)
}

/// What the ctx line's tail shows.
pub const TAILS: [&str; 3] = ["%", "tokens", "tokens / window"];

/// What the lines take their color from.
pub const COLORS: [&str; 2] = ["agent", "lab (ctx coral, windows ink)"];

struct Agent {
    name: &'static str,
    color: Rgb,
    tokens: u64,
    usd: f64,
    /// Context tokens of the latest session, the model's context window.
    ctx: u64,
    window: u64,
    spark: &'static str,
    live: bool,
}

struct Window {
    label: &'static str,
    pct: f64,
    resets: &'static str,
    /// % of the window per $.
    pct_per_usd: f64,
}

/// What the page draws with.
pub struct Tokens {
    pub t: f32,
    pub tail: usize,
    pub color: usize,
}

impl Tokens {
    fn agents(&self) -> Vec<Agent> {
        // REGI's context grows 55 % → 92 % over 24 s, then compacts
        let p = (self.t / 24.0).fract() as f64;
        let regi_ctx = (110_000.0 + p * 74_000.0) as u64;
        vec![
            Agent { name: "REGI", color: (130, 205, 160), tokens: 1_240_000 + (p * 90_000.0) as u64, usd: 4.21 + p * 0.4, ctx: regi_ctx, window: 200_000, spark: "··▁▂▂▃▅▆▇█▆▅▃", live: true },
            Agent { name: "ENOCH", color: (205, 150, 235), tokens: 386_000, usd: 1.12, ctx: 62_000, window: 200_000, spark: "▃▅▂··········", live: false },
            Agent { name: "NOVA", color: (235, 190, 110), tokens: 92_000, usd: 0.31, ctx: 171_000, window: 200_000, spark: "·····▂▁·······", live: false },
        ]
    }

    fn windows(&self) -> Vec<Window> {
        let p = (self.t / 24.0).fract() as f64;
        vec![
            Window { label: "5h", pct: 42.0 + p * 30.0, resets: "2h", pct_per_usd: 0.6 },
            Window { label: "7d", pct: 88.0 + p * 4.0, resets: "3d", pct_per_usd: 0.09 },
        ]
    }

    /// The next message: ($, tokens), pulses with the typed text.
    fn next(&self) -> (f64, u64) {
        let s = 0.5 + 0.5 * (self.t * 0.8).sin() as f64;
        (1.2 + s * 2.6, 120_000 + (s * 60_000.0) as u64)
    }

    pub fn draw(&self, buf: &mut Buffer, r: Rect) {
        write(buf, r.x, r.y, "tokens · the thread header's tokens box: now and with line bars", CORAL);
        let about = format!("ctx tail: {}  ·  colors: {}  ·  1 line per agent: ctx % = context tokens / window", TAILS[self.tail], COLORS[self.color]);
        write(buf, r.x, r.y + 1, &about, MUTED);
        let agents = self.agents();
        let wins = self.windows();
        // 3 agents: 62 x 7 inside (MAX_COST_BOX 66, header_rows(3) 9); 1 agent: 44 x 4
        let big = (62u16, 7u16);
        let small = (44u16, 4u16);
        let y = r.y + 3;
        let x2 = r.x + big.0 + 8;
        self.boxed(buf, r.x, y, big, "now · 3 agents", |b, a| self.now_box(b, a, &agents, &wins));
        self.boxed(buf, x2, y, big, "new · 3 agents", |b, a| self.new_box(b, a, &agents, &wins));
        let y = y + big.1 + 4;
        self.boxed(buf, r.x, y, small, "now · 1 agent (narrowest)", |b, a| self.now_box(b, a, &agents[..1], &wins));
        self.boxed(buf, x2, y, small, "new · 1 agent (narrowest)", |b, a| self.new_box(b, a, &agents[..1], &wins));
        let y = y + small.1 + 4;
        let notes = [
            "agent rows: ctx % column + last-hour spark → 1 ctx line (fill: context tokens / window)",
            "plan windows: bg block + ░ → ━ used, ━ next message share (blue), ─ rest",
            "window color: ink, ≥ 70 % warn, ≥ 90 % coral (now: same thresholds)",
            "tok column (all turns) stays a number: 0 max for a bar",
            "dropped: last-hour spark (0 room next to the ctx line)",
        ];
        for (i, n) in notes.iter().enumerate() {
            write(buf, r.x, y + i as u16, &format!("· {n}"), MUTED);
        }
    }

    /// A frame of the header (straight lines) around a box of SIZE inside, TITLE over it.
    fn boxed(&self, buf: &mut Buffer, x: u16, y: u16, size: (u16, u16), title: &str, draw: impl Fn(&mut Buffer, Rect)) {
        let line = glyph::mix(BG, MUTED, 0.5);
        write(buf, x, y, title, MUTED);
        let (w, h) = (size.0 + 4, size.1 + 2);
        let y = y + 1;
        for i in 0..w {
            let (top, bot) = match i {
                0 => ("┌", "└"),
                _ if i == w - 1 => ("┐", "┘"),
                _ => ("─", "─"),
            };
            write(buf, x + i, y, top, line);
            write(buf, x + i, y + h - 1, bot, line);
        }
        for j in 1..h - 1 {
            write(buf, x, y + j, "│", line);
            write(buf, x + w - 1, y + j, "│", line);
        }
        draw(buf, Rect { x: x + 2, y: y + 1, width: size.0, height: size.1 });
    }

    /// The box as cost.rs draws it now (compact): columns, bg bars with ░.
    fn now_box(&self, buf: &mut Buffer, a: Rect, agents: &[Agent], wins: &[Window]) {
        let w = a.width as usize;
        let name_w = 6;
        let spark_w = w.saturating_sub(name_w + 7 + 9 + 5 + 2);
        let mut y = a.y;
        let head = format!("{:name_w$}{:>7}{:>9}{:>5}  {}", "", "tok", "$", "ctx", if spark_w >= 9 { "last hour" } else { "" });
        let cost = format!("${:.2} thread", agents.iter().map(|g| g.usd).sum::<f64>());
        write(buf, a.x, y, head.trim_end(), MUTED);
        thread_cost(buf, a, y, head.trim_end(), &cost);
        y += 1;
        for g in agents {
            write(buf, a.x, y, g.name, g.color);
            let nums = format!("{:>7}{:>9}{:>5}", human(g.tokens), format!("${:.2}", g.usd), format!("{:.0}%", g.ctx as f64 * 100.0 / g.window as f64));
            write(buf, a.x + name_w as u16, y, &nums, INK);
            let sp: String = g.spark.chars().cycle().take(spark_w).collect();
            write(buf, a.x + (name_w + 21 + 2) as u16, y, &sp, INFO);
            y += 1;
        }
        y = self.all_row(buf, a, y, agents);
        // windows: side by side, bg block used, ▒ share, ░ rest
        let (usd, _) = self.next();
        let n = wins.len();
        let seg = w.saturating_sub(2 * (n - 1)) / n;
        let mut x = a.x;
        for l in wins {
            write(buf, x, y, &format!("{} ", l.label), MUTED);
            let bw = seg - l.label.len() - 1 - 9;
            let used = cells(l.pct, bw);
            let share = cells(usd * l.pct_per_usd, bw).max(1).min(bw - used);
            let bx = x + l.label.len() as u16 + 1;
            let col = level(l.pct, glyph::mix(BG, MUTED, 0.55));
            for i in 0..bw {
                if i < used {
                    bg_cell(buf, bx + i as u16, y, col);
                } else if i < used + share {
                    write(buf, bx + i as u16, y, "▒", INFO);
                } else {
                    write(buf, bx + i as u16, y, "░", glyph::mix(BG, MUTED, 0.5));
                }
            }
            write(buf, bx + bw as u16, y, &format!(" {:>3.0}% {:>3}", l.pct, l.resets), MUTED);
            x += seg as u16 + 2;
        }
        self.next_row(buf, a, y + 1, wins);
    }

    /// The box with line bars: 1 ctx line per agent, line bars for the windows.
    fn new_box(&self, buf: &mut Buffer, a: Rect, agents: &[Agent], wins: &[Window]) {
        let w = a.width as usize;
        let name_w = 6;
        let tail_w = match self.tail {
            0 => 4,
            1 => 4,
            _ => 9,
        };
        let bar_w = w.saturating_sub(name_w + 7 + 9 + 2 + 1 + tail_w);
        let mut y = a.y;
        let head = format!("{:name_w$}{:>7}{:>9}  context", "", "tok", "$");
        let cost = format!("${:.2} thread", agents.iter().map(|g| g.usd).sum::<f64>());
        write(buf, a.x, y, &head, MUTED);
        thread_cost(buf, a, y, &head, &cost);
        y += 1;
        for g in agents {
            write(buf, a.x, y, g.name, g.color);
            let nums = format!("{:>7}{:>9}", human(g.tokens), format!("${:.2}", g.usd));
            write(buf, a.x + name_w as u16, y, &nums, INK);
            let pct = g.ctx as f64 * 100.0 / g.window as f64;
            let fill = if self.color == 0 { g.color } else { CORAL };
            // a full context: coral in both schemes
            let fill = if pct >= 90.0 { CORAL } else { fill };
            let fill = if g.live { fill } else { glyph::mix(BG, fill, 0.7) };
            let bx = a.x + (name_w + 16 + 2) as u16;
            line_bar(buf, bx, y, bar_w, pct, 0.0, fill);
            let tail = match self.tail {
                0 => format!("{:.0}%", pct),
                1 => kilo(g.ctx),
                _ => format!("{}/{}", kilo(g.ctx), kilo(g.window)),
            };
            write(buf, bx + bar_w as u16 + 1, y, &format!("{tail:>tail_w$}"), MUTED);
            y += 1;
        }
        y = self.all_row(buf, a, y, agents);
        let (usd, _) = self.next();
        let n = wins.len();
        let seg = w.saturating_sub(2 * (n - 1)) / n;
        let mut x = a.x;
        for l in wins {
            write(buf, x, y, &format!("{} ", l.label), MUTED);
            let bw = seg - l.label.len() - 1 - 9;
            let bx = x + l.label.len() as u16 + 1;
            line_bar(buf, bx, y, bw, l.pct, usd * l.pct_per_usd, level(l.pct, INK));
            write(buf, bx + bw as u16, y, &format!(" {:>3.0}% {:>3}", l.pct, l.resets), MUTED);
            x += seg as u16 + 2;
        }
        self.next_row(buf, a, y + 1, wins);
    }

    /// The `all` row (2+ agents): tokens, $. Returns the next row.
    fn all_row(&self, buf: &mut Buffer, a: Rect, y: u16, agents: &[Agent]) -> u16 {
        if agents.len() < 2 {
            return y;
        }
        write(buf, a.x, y, "all", MUTED);
        let tok: u64 = agents.iter().map(|g| g.tokens).sum();
        let usd: f64 = agents.iter().map(|g| g.usd).sum();
        write(buf, a.x + 6, y, &format!("{:>7}{:>9}", human(tok), format!("${usd:.2}")), INK);
        y + 1
    }

    fn next_row(&self, buf: &mut Buffer, a: Rect, y: u16, wins: &[Window]) {
        if y >= a.bottom() {
            return;
        }
        let (usd, tok) = self.next();
        let mut parts = vec![("next  ".to_string(), MUTED), (format!("≥{} tok", human(tok)), INK), (format!(" ~${usd:.2}"), MUTED)];
        for l in wins {
            parts.push((format!(" · {} ", l.label), MUTED));
            parts.push((format!("+{:.1}%", usd * l.pct_per_usd), INFO));
        }
        // clipped at the box's right edge (cost.rs: a Paragraph, 0 wrap)
        let mut x = a.x;
        for (s, c) in parts {
            for ch in s.chars() {
                if x >= a.right() {
                    return;
                }
                write(buf, x, y, &ch.to_string(), c);
                x += 1;
            }
        }
    }
}

/// COST at the right end of the column row HEAD, when it fits (cost.rs: compact head).
fn thread_cost(buf: &mut Buffer, a: Rect, y: u16, head: &str, cost: &str) {
    let room = (a.width as usize).saturating_sub(head.chars().count());
    if room > cost.len() {
        write(buf, a.right() - cost.len() as u16, y, cost, MUTED);
    }
}

/// A line bar of W cells at X, Y: PCT used (━ in FG), SHARE % after it (━ in INFO), ─ rest.
fn line_bar(buf: &mut Buffer, x: u16, y: u16, w: usize, pct: f64, share: f64, fg: Rgb) {
    let used = cells(pct, w);
    let share = if share > 0.0 { cells(share, w).max(1).min(w - used) } else { 0 };
    for i in 0..w {
        let (ch, c) = if i < used {
            ("━", fg)
        } else if i < used + share {
            ("━", INFO)
        } else {
            ("─", track())
        };
        write(buf, x + i as u16, y, ch, c);
    }
}

/// P % of W cells.
fn cells(p: f64, w: usize) -> usize {
    (((p.clamp(0.0, 100.0) / 100.0) * w as f64).round() as usize).min(w)
}

/// BASE, warn from 70 %, coral from 90 % (cost.rs: caution, error).
fn level(pct: f64, base: Rgb) -> Rgb {
    if pct >= 90.0 {
        CORAL
    } else if pct >= 70.0 {
        WARN
    } else {
        base
    }
}

/// A cell of solid COL at X, Y.
fn bg_cell(buf: &mut Buffer, x: u16, y: u16, col: Rgb) {
    if let Some(c) = buf.cell_mut((x, y)) {
        c.set_char(' ');
        c.set_bg(ratatui::style::Color::Rgb(col.0, col.1, col.2));
    }
}

/// V in whole k: 171_700 → `172k` (a context tail: 4 chars max. up to 999k).
fn kilo(v: u64) -> String {
    format!("{}k", (v + 500) / 1000)
}

/// cost.rs: human.
fn human(v: u64) -> String {
    match v {
        0 => "·".into(),
        1..1_000 => v.to_string(),
        1_000..1_000_000 => format!("{:.1}k", v as f64 / 1e3),
        _ => format!("{:.1}M", v as f64 / 1e6),
    }
}
