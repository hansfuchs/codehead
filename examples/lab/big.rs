//! Big page: the regis at 1.5x, 2x, 3x and 4x the avatar (7x4 cells), each size in 2 thread
//! layouts after game UIs (not the avatar line above the prompt box). Mock threads: fixed text,
//! the crew of the float page (REGI + 4 random), the line mode of the float page's columns.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::glyph::{self, BG, CORAL, Cell, INK, MUTED, Rgb};
use super::{regi, write};

/// The sizes: name, cells (w, h). 1.5x: 22x24 dots (the width even).
pub const SIZES: [(&str, (usize, usize)); 4] = [("1.5x", (11, 6)), ("2x", (14, 8)), ("3x", (21, 12)), ("4x", (28, 16))];

/// The layouts per size: name, what it shows.
pub const LAYOUTS: [[(&str, &str); 2]; 4] = [
    [("dialogue box", "RPG text box: portrait inside, name tab, text typed in"), ("log portraits", "chat log, a portrait beside each agent message")],
    [("party sidebar", "the thread's agents stacked left, context and token bars"), ("unit panel", "strategy HUD: portrait, stats, command grid, prompt inside")],
    [("side stage", "visual novel: the agent stands beside the log, the bubble points at it"), ("cut-in", "turn start: a band sweeps in with the agent, then the thread")],
    [("character select", "pick screen: big portrait, stat sheet, the crew in a row"), ("attract mode", "idle thread: the agent floats over the dimmed log")],
];

/// The crew's names (REGI first, as on the float page).
const NAMES: [&str; 5] = ["REGI", "ENOCH", "GIGAS", "NOVA", "PIKE"];

/// The crew's status when the page has none (s: status): REGI works, the others think, idle, done.
const CREW_STATUS: [usize; 5] = [0, 1, 2, 4, 2];

/// The mock thread: (agent index or None for the owner, text).
const LOG: [(Option<usize>, &str); 5] = [
    (None, "the regi float looks steppy at low speeds. why?"),
    (Some(0), "cause: Bresenham on rounded end points. the whole outline jumps 1 dot."),
    (Some(0), "lab page 8 compares 8 line modes. shade: 4x more steps."),
    (None, "add a page for bigger sprites, with layouts from games"),
    (Some(0), "page 9 added: 4 sizes, 2 layouts each. ←→ size, ↑↓ layout, l lines."),
];

/// What the page draws with.
pub struct Big<'a> {
    pub t: f32,
    pub size: usize,
    pub layout: usize,
    pub lines: regi::Lines,
    /// The crew: indices into regi::all(), REGI's first.
    pub crew: &'a [usize],
    pub status: Option<usize>,
    /// REGI's eye pattern (index into regi::PATTERNS).
    pub regi_eyes: usize,
}

impl Big<'_> {
    fn status(&self, i: usize) -> usize {
        if i == 0 { self.status.unwrap_or(CREW_STATUS[0]) } else { CREW_STATUS[i % 5] }
    }

    /// Crew member I as cells of SIZE.
    fn sprite(&self, i: usize, size: (usize, usize)) -> Vec<Cell> {
        let r = &regi::all()[self.crew[i % self.crew.len()]];
        let look = regi::Look { pattern: (i == 0).then_some(regi::PATTERNS[self.regi_eyes]), speed: 1.0 };
        regi::render_sized(r, self.status(i), self.t, &look, self.lines, size)
    }

    pub fn draw(&self, buf: &mut Buffer, r: Rect) {
        let (name, cells) = SIZES[self.size];
        let (lname, about) = LAYOUTS[self.size][self.layout];
        let head = format!("big · {name} ({}x{} cells)  ·  {}/2 {lname}  ·  lines: {}", cells.0, cells.1, self.layout + 1, self.lines.name());
        write(buf, r.x, r.y, &head, CORAL);
        write(buf, r.x, r.y + 1, about, MUTED);
        // the mock terminal: a frame around the thread
        let w = r.width.min(120);
        let area = Rect { x: r.x, y: r.y + 3, width: w, height: r.height.saturating_sub(3) };
        frame(buf, area, glyph::mix(BG, MUTED, 0.5), "thread · regi: avatar float");
        let inner = Rect { x: area.x + 2, y: area.y + 1, width: area.width.saturating_sub(4), height: area.height.saturating_sub(2) };
        match (self.size, self.layout) {
            (0, 0) => self.dialogue(buf, inner, cells),
            (0, _) => self.log_portraits(buf, inner, cells),
            (1, 0) => self.party(buf, inner, cells),
            (1, _) => self.unit_panel(buf, inner, cells),
            (2, 0) => self.side_stage(buf, inner, cells),
            (2, _) => self.cut_in(buf, inner, cells),
            (3, 0) => self.select(buf, inner, cells),
            _ => self.attract(buf, inner, cells),
        }
    }

    /// The log from line FIRST on, in RECT (wrapped), bottom-aligned; agent lines in INK, owner
    /// lines in MUTED with a ›. Returns the rows it took.
    fn log(&self, buf: &mut Buffer, rect: Rect, from: usize, to: usize, dim: f32) -> u16 {
        let mut rows: Vec<(String, Rgb)> = Vec::new();
        for &(who, text) in &LOG[from..to] {
            let (prefix, fg) = match who {
                None => ("› ", MUTED),
                Some(_) => ("  ", INK),
            };
            for (k, line) in wrap(text, rect.width.saturating_sub(2) as usize).into_iter().enumerate() {
                rows.push((format!("{}{line}", if k == 0 { prefix } else { "  " }), glyph::mix(BG, fg, dim)));
            }
            rows.push((String::new(), INK));
        }
        let skip = rows.len().saturating_sub(rect.height as usize);
        let top = rect.y + rect.height.saturating_sub((rows.len() - skip) as u16);
        for (i, (s, fg)) in rows.iter().skip(skip).enumerate() {
            write(buf, rect.x, top + i as u16, s, *fg);
        }
        (rows.len() - skip) as u16
    }

    /// The prompt box on the last 3 rows of RECT; returns the rect above it.
    fn prompt(&self, buf: &mut Buffer, rect: Rect) -> Rect {
        let y = rect.y + rect.height.saturating_sub(3);
        frame(buf, Rect { x: rect.x, y, width: rect.width, height: 3 }, MUTED, "");
        let cursor = if (self.t * 2.0).fract() < 0.5 { "▏" } else { " " };
        write(buf, rect.x + 2, y + 1, &format!("› {cursor}"), INK);
        Rect { height: rect.height.saturating_sub(3), ..rect }
    }

    // ─── 1.5x ────────────────────────────────────────────────────────────────

    /// RPG text box: the last agent line typed in beside the portrait, a name tab on the border,
    /// ▼ when the text is out. The log above it, dim.
    fn dialogue(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let rest = self.prompt(buf, r);
        let bh = ch as u16 + 2;
        let by = rest.y + rest.height.saturating_sub(bh + 1);
        self.log(buf, Rect { height: by.saturating_sub(rest.y + 1), ..rest }, 0, 4, 0.55);
        let boxr = Rect { x: rest.x, y: by, width: rest.width, height: bh };
        frame(buf, boxr, CORAL, "");
        write(buf, boxr.x + 2, by, &format!("┤ {} ├", NAMES[0]), CORAL);
        blit_clear(buf, boxr.x + 2, by + 1, cw, &self.sprite(0, (cw, ch)));
        // the text: 40 chars/s, a pause of 2 s at the end, again
        let text = LOG[4].1;
        let n = text.chars().count();
        let shown = ((self.t * 40.0) as usize) % (n + 80);
        let tx = boxr.x + 4 + cw as u16;
        let width = boxr.width.saturating_sub(cw as u16 + 7) as usize;
        let typed: String = text.chars().take(shown.min(n)).collect();
        for (i, line) in wrap(&typed, width).iter().enumerate() {
            write(buf, tx, by + 1 + i as u16, line, INK);
        }
        if shown >= n && (self.t * 2.5).fract() < 0.6 {
            write(buf, boxr.x + boxr.width - 3, by + bh - 2, "▼", CORAL);
        }
    }

    /// Chat log with portraits: each agent message has the portrait in a gutter on its left, the
    /// owner's lines right aligned.
    fn log_portraits(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let rest = self.prompt(buf, r);
        let mut y = rest.y + rest.height;
        for (k, &(who, text)) in LOG.iter().enumerate().rev() {
            match who {
                Some(a) => {
                    let h = ch as u16 + 1;
                    if y < rest.y + h {
                        break;
                    }
                    y -= h;
                    // older messages: the portrait in the done color (not live), the newest live
                    let cells = if k == LOG.len() - 1 { self.sprite(a, (cw, ch)) } else { self.frozen(a, (cw, ch)) };
                    blit_clear(buf, rest.x, y, cw, &cells);
                    write(buf, rest.x + cw as u16 + 2, y, NAMES[a], CORAL);
                    for (i, line) in wrap(text, rest.width.saturating_sub(cw as u16 + 2) as usize).iter().enumerate() {
                        write(buf, rest.x + cw as u16 + 2, y + 1 + i as u16, line, if k == LOG.len() - 1 { INK } else { glyph::mix(BG, INK, 0.7) });
                    }
                }
                None => {
                    if y < rest.y + 2 {
                        break;
                    }
                    y -= 2;
                    let s = format!("{text} ‹");
                    let x = rest.x + rest.width.saturating_sub(s.chars().count() as u16);
                    write(buf, x, y, &s, MUTED);
                }
            }
        }
    }

    /// Crew member I still (done status, its 1st frame).
    fn frozen(&self, i: usize, size: (usize, usize)) -> Vec<Cell> {
        let r = &regi::all()[self.crew[i % self.crew.len()]];
        let look = regi::Look { pattern: (i == 0).then_some(regi::PATTERNS[self.regi_eyes]), speed: 1.0 };
        regi::render_sized(r, 4, 0.0, &look, self.lines, size)
    }

    // ─── 2x ──────────────────────────────────────────────────────────────────

    /// Party sidebar: the agents of the thread stacked on the left, each with its name, status,
    /// a context bar and a token bar; ▶ on the one with the turn.
    fn party(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let side = cw as u16 + 4;
        let slot = ch as u16 + 4;
        let n = ((r.height / slot) as usize).clamp(1, 3);
        for i in 0..n {
            let y = r.y + i as u16 * slot;
            let on = i == 0;
            let fg = if on { CORAL } else { glyph::mix(BG, MUTED, 0.8) };
            write(buf, r.x, y, if on { "▶" } else { " " }, CORAL);
            write(buf, r.x + 2, y, NAMES[i], fg);
            write(buf, r.x + 2 + NAMES[i].len() as u16 + 1, y, status_name(self.status(i)), glyph::mix(BG, MUTED, 0.7));
            blit_clear(buf, r.x + 2, y + 1, cw, &self.sprite(i, (cw, ch)));
            let bw = cw as u16;
            bar(buf, r.x + 2, y + 1 + ch as u16, bw, [0.62, 0.35, 0.8][i], CORAL, "ctx");
            bar(buf, r.x + 2, y + 2 + ch as u16, bw, [0.4, 0.7, 0.2][i], INK, "tok");
        }
        // the divider, then the log and the prompt on the right
        for y in r.y..r.y + r.height {
            write(buf, r.x + side, y, "│", glyph::mix(BG, MUTED, 0.4));
        }
        let right = Rect { x: r.x + side + 2, y: r.y, width: r.width.saturating_sub(side + 2), height: r.height };
        let rest = self.prompt(buf, right);
        self.log(buf, rest, 0, LOG.len(), 1.0);
    }

    /// Unit panel: a strategy-game HUD at the bottom instead of the prompt header; portrait,
    /// stats, a 3x2 command grid; the prompt line inside the panel.
    fn unit_panel(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let ph = ch as u16 + 2;
        let py = r.y + r.height.saturating_sub(ph);
        let panel = Rect { x: r.x, y: py, width: r.width, height: ph };
        frame(buf, panel, CORAL, "");
        self.log(buf, Rect { height: py.saturating_sub(r.y + 1), ..r }, 0, LOG.len(), 1.0);
        blit_clear(buf, panel.x + 2, py + 1, cw, &self.sprite(0, (cw, ch)));
        // the portrait's own box inside the panel
        for y in py + 1..py + ph - 1 {
            write(buf, panel.x + cw as u16 + 3, y, "│", glyph::mix(BG, MUTED, 0.5));
        }
        let sx = panel.x + cw as u16 + 5;
        write(buf, sx, py + 1, NAMES[0], CORAL);
        write(buf, sx + 6, py + 1, status_name(self.status(0)), MUTED);
        let stats = [("model", "opus 5.5"), ("harness", "claude"), ("mode", "autopilot")];
        for (i, (k, v)) in stats.iter().enumerate() {
            write(buf, sx, py + 2 + i as u16, &format!("{k:<8}{v}"), glyph::mix(BG, INK, 0.8));
        }
        bar(buf, sx, py + 6, 18, 0.62, CORAL, "ctx");
        // the prompt in the panel's last inner row
        let cursor = if (self.t * 2.0).fract() < 0.5 { "▏" } else { " " };
        write(buf, sx, py + ph - 2, &format!("› {cursor}"), INK);
        // the command grid on the right
        let cmds = ["⏎ send", "^c stop", "! shell", "@ hand", "⇥ files", "? keys"];
        let gx = panel.x + panel.width.saturating_sub(32);
        for (i, c) in cmds.iter().enumerate() {
            let (col, row) = (i % 3, i / 3);
            let (x, y) = (gx + col as u16 * 10, py + 1 + row as u16 * 3);
            frame(buf, Rect { x, y, width: 9, height: 3 }, glyph::mix(BG, MUTED, 0.6), "");
            write(buf, x + 1, y + 1, c, INK);
        }
    }

    // ─── 3x ──────────────────────────────────────────────────────────────────

    /// Side stage: the agent stands on the right, the newest message in a bubble whose tail points
    /// at it; a floor line under the agent, the status under the floor.
    fn side_stage(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let rest = self.prompt(buf, r);
        let sx = rest.x + rest.width.saturating_sub(cw as u16 + 1);
        let sy = rest.y + rest.height.saturating_sub(ch as u16 + 2);
        blit_clear(buf, sx, sy, cw, &self.sprite(0, (cw, ch)));
        let floor: String = "▔".repeat(cw);
        write(buf, sx, sy + ch as u16, &floor, glyph::mix(BG, MUTED, 0.5));
        write(buf, sx, sy + ch as u16 + 1, &format!("{} · {}", NAMES[0], status_name(self.status(0))), CORAL);
        // the bubble: the newest line, left of the stage, its tail at the agent's mouth height
        let bw = rest.width.saturating_sub(cw as u16 + 6).min(56);
        let lines = wrap(LOG[4].1, bw.saturating_sub(4) as usize);
        let bh = lines.len() as u16 + 2;
        let bx = sx.saturating_sub(bw + 3);
        let by = sy + (ch as u16 / 2).saturating_sub(bh / 2);
        frame(buf, Rect { x: bx, y: by, width: bw, height: bh }, INK, "");
        for (i, l) in lines.iter().enumerate() {
            write(buf, bx + 2, by + 1 + i as u16, l, INK);
        }
        write(buf, bx + bw - 1, by + bh / 2, "├──", INK);
        // the log above the bubble, dim
        self.log(buf, Rect { x: rest.x, y: rest.y, width: bx.saturating_sub(rest.x) + bw, height: by.saturating_sub(rest.y + 1) }, 0, 4, 0.6);
    }

    /// Cut-in: every 5 s a band sweeps in from the right with the agent at 3x and its name, holds,
    /// sweeps out to the left; between the cut-ins the thread as now (7x4 above the prompt).
    fn cut_in(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let rest = self.prompt(buf, r);
        let p = self.t % 5.0;
        // the thread with the small avatar above the prompt
        let small_y = rest.y + rest.height.saturating_sub(5);
        self.log(buf, Rect { height: small_y.saturating_sub(rest.y + 1), ..rest }, 0, LOG.len(), 1.0);
        blit_clear(buf, rest.x, small_y, 7, &self.sprite(0, (7, 4)));
        write(buf, rest.x + 9, small_y + 1, NAMES[0], CORAL);
        // the band: 0..0.3 s in, 0.3..1.8 s hold, 1.8..2.1 s out
        let w = rest.width as f32;
        let off = if p < 0.3 {
            w * (1.0 - ease(p / 0.3))
        } else if p < 1.8 {
            0.0
        } else if p < 2.1 {
            -w * ease((p - 1.8) / 0.3)
        } else {
            return;
        };
        let bh = ch as u16 + 2;
        let by = rest.y + rest.height.saturating_sub(bh) / 2;
        for row in 0..bh {
            for col in 0..rest.width {
                let x = col as f32 + off;
                if x < 0.0 || x >= w {
                    continue;
                }
                let edge = row == 0 || row == bh - 1;
                // speed lines right of the agent and its name, the edges solid
                let lines = col as usize > cw + 30 && (col as usize + row as usize * 2 + (self.t * 30.0) as usize).is_multiple_of(7);
                let ch = if edge { '━' } else if lines { '╱' } else { ' ' };
                let fg = if edge { CORAL } else { glyph::mix(BG, CORAL, 0.35) };
                super::put(buf, rest.x + x as u16, by + row, Cell { ch, fg, bg: Some(glyph::mix(BG, CORAL, 0.08)) });
            }
        }
        let sx = rest.x as f32 + 4.0 + off;
        if sx >= rest.x as f32 && sx + (cw as f32) < rest.x as f32 + w {
            let cells: Vec<Cell> = self.sprite(0, (cw, ch)).into_iter().map(|c| Cell { bg: Some(glyph::mix(BG, CORAL, 0.08)), ..c }).collect();
            super::blit(buf, sx as u16, by + 1, cw, &cells);
        }
        let tx = rest.x as f32 + cw as f32 + 8.0 + off;
        if tx >= rest.x as f32 && tx + 20.0 < rest.x as f32 + w {
            write(buf, tx as u16, by + bh / 2 - 1, &spaced(NAMES[0]), CORAL);
            write(buf, tx as u16, by + bh / 2 + 1, "takes the turn", INK);
        }
    }

    // ─── 4x ──────────────────────────────────────────────────────────────────

    /// Character select: the picked agent big on the left, a stat sheet on the right, the crew
    /// as 7x4 avatars in a row below with a cursor; the pick moves every 2.5 s.
    fn select(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let n = self.crew.len().min(5);
        let pick = ((self.t / 2.5) as usize) % n;
        write(buf, r.x, r.y, "pick an agent for this thread", MUTED);
        blit_clear(buf, r.x + 2, r.y + 2, cw, &self.sprite(pick, (cw, ch)));
        let sx = r.x + cw as u16 + 6;
        write(buf, sx, r.y + 2, &spaced(NAMES[pick]), CORAL);
        let a = &regi::all()[self.crew[pick]];
        let regi::Kind::Solid(solid, motion) = a.kind;
        let stats = [("solid", solid.name().to_string()), ("motion", format!("{motion:?}").to_lowercase()), ("eyes", if pick == 0 { regi::PATTERNS[self.regi_eyes].name().to_string() } else { a.pattern.name().to_string() }), ("talk", a.talk.name().to_string()), ("status", status_name(self.status(pick)).to_string())];
        for (i, (k, v)) in stats.iter().enumerate() {
            write(buf, sx, r.y + 4 + i as u16, &format!("{k:<8}{v}"), INK);
        }
        // made-up gauges from the avatar's numbers: a fixed look per agent
        let g = |seed: u64| 0.3 + 0.7 * regi::hash(self.crew[pick] as u64 * 7 + seed);
        for (i, (k, v)) in [("focus", g(1)), ("speed", g(2)), ("calm", g(3))].iter().enumerate() {
            bar(buf, sx, r.y + 10 + i as u16, 24, *v, CORAL, k);
        }
        // the crew row
        let ry = r.y + ch as u16 + 4;
        for i in 0..n {
            let x = r.x + 2 + i as u16 * 11;
            let on = i == pick;
            frame(buf, Rect { x, y: ry, width: 9, height: 6 }, if on { CORAL } else { glyph::mix(BG, MUTED, 0.5) }, "");
            blit_clear(buf, x + 1, ry + 1, 7, &self.sprite(i, (7, 4)));
            write(buf, x + 1, ry + 6, NAMES[i], if on { CORAL } else { MUTED });
        }
    }

    /// Attract mode: an idle thread; the log dims, the agent floats over it at 4x, slowly from side
    /// to side; a blinking hint to wake it.
    fn attract(&self, buf: &mut Buffer, r: Rect, (cw, ch): (usize, usize)) {
        let rest = self.prompt(buf, r);
        self.log(buf, rest, 0, LOG.len(), 0.3);
        let span = rest.width.saturating_sub(cw as u16) as f32;
        let x = rest.x as f32 + span * (0.5 + 0.5 * (self.t * 0.15).sin());
        let y = rest.y as f32 + rest.height.saturating_sub(ch as u16 + 1) as f32 * (0.5 + 0.3 * (self.t * 0.23).sin());
        let idle = regi::render_sized(&regi::all()[self.crew[0]], self.status.unwrap_or(2), self.t, &regi::Look { pattern: Some(regi::PATTERNS[self.regi_eyes]), speed: 1.0 }, self.lines, (cw, ch));
        blit_clear(buf, x as u16, y as u16, cw, &idle);
        if (self.t * 1.2).fract() < 0.6 {
            let hint = format!("type to wake {}", NAMES[0]);
            write(buf, rest.x + rest.width.saturating_sub(hint.len() as u16) / 2, rest.y + rest.height - 1, &hint, CORAL);
        }
    }
}

/// CELLS (W wide) at (X, Y); blank braille cells left out (the log shows through).
fn blit_clear(buf: &mut Buffer, x: u16, y: u16, w: usize, cells: &[Cell]) {
    for (i, c) in cells.iter().enumerate() {
        if c.ch != '\u{2800}' && c.ch != ' ' {
            super::put(buf, x + (i % w) as u16, y + (i / w) as u16, *c);
        }
    }
}

/// A rounded frame around R in FG, TITLE on its top border.
fn frame(buf: &mut Buffer, r: Rect, fg: Rgb, title: &str) {
    if r.width < 2 || r.height < 2 {
        return;
    }
    let (x1, y1) = (r.x + r.width - 1, r.y + r.height - 1);
    for x in r.x + 1..x1 {
        write(buf, x, r.y, "─", fg);
        write(buf, x, y1, "─", fg);
    }
    for y in r.y + 1..y1 {
        write(buf, r.x, y, "│", fg);
        write(buf, x1, y, "│", fg);
    }
    write(buf, r.x, r.y, "╭", fg);
    write(buf, x1, r.y, "╮", fg);
    write(buf, r.x, y1, "╰", fg);
    write(buf, x1, y1, "╯", fg);
    if !title.is_empty() {
        write(buf, r.x + 2, r.y, &format!(" {title} "), fg);
    }
}

/// A gauge: LABEL, then W cells, the FRAC part full.
fn bar(buf: &mut Buffer, x: u16, y: u16, w: u16, frac: f32, fg: Rgb, label: &str) {
    write(buf, x, y, label, MUTED);
    let bw = w.saturating_sub(label.len() as u16 + 1);
    let full = (frac.clamp(0.0, 1.0) * bw as f32).round() as u16;
    for i in 0..bw {
        let (ch, c) = if i < full { ("━", fg) } else { ("─", glyph::mix(BG, MUTED, 0.4)) };
        write(buf, x + label.len() as u16 + 1 + i, y, ch, c);
    }
}

/// S wrapped at W chars (words kept whole).
fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > w.max(1) {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// "REGI" → "R E G I".
fn spaced(s: &str) -> String {
    s.chars().map(|c| c.to_string()).collect::<Vec<_>>().join(" ")
}

/// Ease out (cubic), 0..1.
fn ease(u: f32) -> f32 {
    1.0 - (1.0 - u.clamp(0.0, 1.0)).powi(3)
}

fn status_name(s: usize) -> &'static str {
    ["working", "thinking", "idle", "blocked", "done"][s.min(4)]
}

