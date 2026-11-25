//! codehead lab: ASCII experiments for the mascot, agent avatars and loaders.
//!
//! ```sh
//! cargo run --release --example lab                      # the TUI: 9 pages
//! cargo run --release --example lab -- print shape 72 34  # 1 frame as ANSI text (mode w h [yaw])
//! cargo run --release --example lab -- page 6 1.2 200 42  # 1 page as plain text (page t w h [pick] [status])
//! ```
//!
//! Keys: 1-9 / tab: page · m: render mode · t: tint · e: eye pattern (regi, monolith) · f: float speed · space: pause · ←→↑↓: turn / pick · s: status · r: new avatars (float, big) · l: line mode (big) · q: quit

mod big;
mod fx;
// codehead's avatar module as a whole (avatar-frames: `Avatar`, spawns); the lab's own copies of
// its files below stay for the lab's pages
#[allow(clippy::duplicate_mod)]
#[path = "avatar/mod.rs"]
mod avatar;
#[path = "avatar/geo.rs"]
mod geo;
#[path = "avatar/glyph.rs"]
mod glyph;
#[path = "avatar/glyphs.rs"]
mod glyphs;
#[path = "avatar/mono.rs"]
mod mono;
// the app path (draw3d_in, clean, shade at 7x4) unused here: the lab draws through render_sized
#[allow(dead_code)]
#[path = "avatar/regi.rs"]
mod regi;
#[path = "avatar/sdf.rs"]
mod sdf;
#[path = "avatar/spawn.rs"]
mod spawn;
mod tokens;

use std::io::Write;
use std::time::{Duration, Instant};

use glyph::{BG, CORAL, Cell, INK, MODES, MUTED, Mode, Opts, Rgb, Tint, WARN};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::Color;
use sdf::{AVATARS, Shape, View};

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

fn put(buf: &mut Buffer, x: u16, y: u16, c: Cell) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_char(c.ch);
        cell.set_fg(rgb(c.fg));
        cell.set_bg(rgb(c.bg.unwrap_or(BG)));
    }
}

fn blit(buf: &mut Buffer, x: u16, y: u16, w: usize, cells: &[Cell]) {
    for (i, c) in cells.iter().enumerate() {
        put(buf, x + (i % w) as u16, y + (i / w) as u16, *c);
    }
}

fn write(buf: &mut Buffer, x: u16, y: u16, s: &str, fg: Rgb) {
    for (i, ch) in s.chars().enumerate() {
        put(buf, x + i as u16, y, Cell { ch, fg, bg: None });
    }
}

/// Pixel font for the wordmark: 4x8 px per letter (3 px ascender, 5 px x-height).
fn glyph_rows(c: char) -> [&'static str; 8] {
    match c {
        'c' => [
            "....", "....", "....", ".###", "#...", "#...", "#...", ".###",
        ],
        'o' => [
            "....", "....", "....", ".##.", "#..#", "#..#", "#..#", ".##.",
        ],
        'd' => [
            "...#", "...#", "...#", ".###", "#..#", "#..#", "#..#", ".###",
        ],
        'e' => [
            "....", "....", "....", ".##.", "#..#", "####", "#...", ".###",
        ],
        'h' => [
            "#...", "#...", "#...", "###.", "#..#", "#..#", "#..#", "#..#",
        ],
        'a' => [
            "....", "....", "....", "###.", "...#", ".###", "#..#", ".###",
        ],
        _ => [
            "....", "....", "....", "....", "....", "....", "....", "....",
        ],
    }
}

/// Half blocks from a pixel grid (W x 2N): N rows of cells, 1 color.
fn halves(px: &[bool], w: usize, fg: Rgb) -> Vec<Cell> {
    let rows = px.len() / w / 2;
    let mut out = Vec::with_capacity(w * rows);
    for r in 0..rows {
        for x in 0..w {
            let ch = match (px[r * 2 * w + x], px[(r * 2 + 1) * w + x]) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                _ => ' ',
            };
            out.push(Cell { ch, fg, bg: None });
        }
    }
    out
}

/// "codehead" as pixel letters: 4 rows of half blocks.
fn wordmark(fg: Rgb) -> (usize, Vec<Cell>) {
    let word = "codehead";
    let w = word.len() * 5 - 1;
    let mut px = vec![false; w * 8];
    for (i, c) in word.chars().enumerate() {
        for (y, row) in glyph_rows(c).iter().enumerate() {
            for (x, b) in row.chars().enumerate() {
                px[y * w + i * 5 + x] = b == '#';
            }
        }
    }
    (w, halves(&px, w, fg))
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Working,
    Thinking,
    Idle,
    Blocked,
    Done,
}

const STATUSES: [Status; 5] = [
    Status::Working,
    Status::Thinking,
    Status::Idle,
    Status::Blocked,
    Status::Done,
];

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::Thinking => "thinking",
            Status::Idle => "idle",
            Status::Blocked => "blocked",
            Status::Done => "done",
        }
    }
}

/// An agent avatar at time T in braille: the view and render options of status S. The 6 first
/// solids turn by status; geo shapes play their own motion, slowed, shaken or stopped by status.
fn avatar(shape: Shape, s: Status, t: f32, phase: f32, frame: usize) -> (View, Opts) {
    let mut v = View::new(shape);
    let mut o = Opts::new(Mode::Braille);
    o.floor = 0.3;
    v.zoom = 1.15;
    // time as the status plays it
    let tt = match s {
        Status::Working => t,
        Status::Thinking => t * 0.6,
        Status::Idle => t * 0.25,
        Status::Blocked => 1.0 + phase,
        Status::Done => phase,
    };
    v.t = tt;
    if let Shape::Geo(i) = shape {
        let (yaw, pitch, roll) = geo::all()[i as usize].pose(tt);
        (v.yaw, v.pitch, v.roll) = (yaw, pitch, roll);
    } else {
        let tilt = if matches!(shape, Shape::Monolith | Shape::Pillar | Shape::Pyramid) {
            0.15
        } else {
            0.45
        };
        v.pitch = tilt;
        v.yaw = match s {
            Status::Working => t * 1.8 + phase,
            Status::Thinking => 0.5 * (t * 0.7 + phase).sin(),
            Status::Idle => 0.35 * (t * 0.5 + phase).sin(),
            _ => 0.4,
        };
    }
    match s {
        Status::Thinking => o.jitter = frame / 3,
        Status::Blocked => {
            if (t * 0.8 + phase).fract() < 0.25 {
                v.yaw += (t * 45.0).sin() * 0.12;
            }
            o.tint = Tint::Mono(WARN);
        }
        Status::Done => {
            o.gain = 0.55;
            o.tint = Tint::Mono(MUTED);
        }
        _ => {}
    }
    (v, o)
}

/// Avatars: the 6 first solids, then the geo catalog.
fn avatar_count() -> usize {
    AVATARS.len() + geo::all().len()
}

fn avatar_at(i: usize) -> (Shape, String) {
    if i < AVATARS.len() {
        (AVATARS[i], AVATARS[i].name().to_string())
    } else {
        (
            Shape::Geo((i - AVATARS.len()) as u16),
            geo::all()[i - AVATARS.len()].name.clone(),
        )
    }
}

struct App {
    page: usize,
    mode: usize,
    tint: Tint,
    paused: bool,
    t: f32,
    yaw: f32,
    pitch: f32,
    status: Option<usize>,
    /// Avatars page: the selected avatar.
    pick: usize,
    /// Regi page: the selected successor. Spawn page: the selected animation. Monolith page: the
    /// design in view.
    rpick: usize,
    spick: usize,
    mpick: usize,
    /// Monolith page: the eye pattern.
    epat: usize,
    /// Regi page: 1 eye pattern for all (None: each its own), the float speed (SPEEDS).
    rpat: Option<usize>,
    speed: usize,
    /// Avatars per row at the last draw (for ↑↓).
    per_row: std::cell::Cell<usize>,
    loaders: fx::Loaders,
    frame: usize,
    /// Float page: the seed of the 4 random avatars (r: next).
    fseed: u64,
    /// Float page: the 1st line mode in view (←→ when they do not all fit).
    fcol: usize,
    /// Big page: the size (big::SIZES), the layout of that size, the line mode (regi::LINES).
    bsize: usize,
    blayout: usize,
    bline: usize,
    /// Tokens page: the ctx tail (tokens::TAILS), the colors (tokens::COLORS).
    ttail: usize,
    tcolor: usize,
    /// Float page: per (row, line mode) the last cells and the changes (t, dots) of the last
    /// FWINDOW seconds; its key (seed, status, speed) clears it when one of them changes.
    fhist: std::collections::HashMap<(usize, usize), (Vec<glyph::Cell>, std::collections::VecDeque<(f32, u32)>)>,
    fkey: (u64, Option<usize>, usize),
}

/// REGI's avatar (`regi:36 eyes=11`), the 1st row of the float page.
const REGI_AVATAR: (usize, usize) = (36, 11);

/// The float page counts the changes of the last 4 s.
const FWINDOW: f32 = 4.0;

/// REGI's avatar, then 4 distinct random ones of SEED.
fn float_picks(seed: u64) -> Vec<usize> {
    let n = regi::all().len();
    let mut out = vec![REGI_AVATAR.0];
    let mut k = 0;
    while out.len() < 5 {
        let i = ((regi::hash(seed.wrapping_mul(97).wrapping_add(k)) * n as f32) as usize).min(n - 1);
        if !out.contains(&i) {
            out.push(i);
        }
        k += 1;
    }
    out
}

const PAGES: [&str; 10] = [
    "moai", "logos", "avatars", "loaders", "regi", "spawn", "monolith", "float", "big", "tokens",
];

/// The float speeds of the regi page.
const SPEEDS: [f32; 5] = [1.0, 0.5, 0.25, 2.0, 3.0];

impl App {
    fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                put(buf, x, y, Cell::BLANK);
            }
        }
        // tabs
        let mut x = 2;
        for (i, p) in PAGES.iter().enumerate() {
            let label = format!(" {} {} ", i + 1, p);
            let on = i == self.page;
            for (j, ch) in label.chars().enumerate() {
                put(
                    buf,
                    x + j as u16,
                    0,
                    Cell {
                        ch,
                        fg: if on { BG } else { MUTED },
                        bg: on.then_some(CORAL),
                    },
                );
            }
            x += label.chars().count() as u16 + 1;
        }
        write(
            buf,
            area.width.saturating_sub(16),
            0,
            "codehead · lab",
            MUTED,
        );
        let body = Rect {
            x: 2,
            y: 2,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(4),
        };
        match self.page {
            0 => self.moai(buf, body),
            1 => self.logos(buf, body),
            2 => self.avatars(buf, body),
            3 => self.loader_page(buf, body),
            4 => self.regis(buf, body),
            5 => self.spawns(buf, body),
            6 => self.monolith(buf, body),
            7 => self.floats(buf, body),
            8 => big::Big { t: self.t, size: self.bsize, layout: self.blayout, lines: regi::LINES[self.bline], crew: &float_picks(self.fseed), status: self.status, regi_eyes: REGI_AVATAR.1 }.draw(buf, body),
            _ => tokens::Tokens { t: self.t, tail: self.ttail, color: self.tcolor }.draw(buf, body),
        }
        let keys = match self.page {
            0 => "m mode · t tint · ←→↑↓ turn · space pause · q quit",
            2 => "←→↑↓ pick · s status · space pause · q quit",
            4 => "←→↑↓ pick · e eyes for all · f float speed · s status · space pause · q quit",
            5 => "←→↑↓ pick · s status · space pause · q quit",
            6 => "←→ design · e eyes · ↑↓ tilt · m mode · t tint · s status · space pause · q quit",
            7 => "←→ modes · r new avatars · f float speed · s status · space pause · q quit",
            8 => "←→ size · ↑↓ layout · l line mode · r new crew · s status · space pause · q quit",
            9 => "v ctx tail · c colors · space pause · q quit",
            _ => "1-9, 0 page · space pause · q quit",
        };
        write(buf, 2, area.height.saturating_sub(1), keys, MUTED);
    }

    fn moai(&self, buf: &mut Buffer, r: Rect) {
        let mode = MODES[self.mode];
        let h = r.height.saturating_sub(3) as usize;
        let w = (h * 2).min(r.width as usize);
        let mut v = moai_view(self.yaw + 0.3 * (self.t * 0.35).sin(), self.pitch);
        v.glow = 0.75 + 0.25 * (self.t * 1.7).sin();
        let mut o = Opts::new(mode);
        o.tint = self.tint;
        let cells = glyph::render(&v, w, h, o);
        let x0 = r.x + (r.width - w as u16) / 2;
        blit(buf, x0, r.y, w, &cells);
        let label = format!("{}  ·  {}", mode.name(), mode.about());
        write(
            buf,
            r.x + (r.width.saturating_sub(label.chars().count() as u16)) / 2,
            r.y + h as u16 + 1,
            &label,
            MUTED,
        );
    }

    fn logos(&self, buf: &mut Buffer, r: Rect) {
        let t = self.t;
        // A: monolith, the moai from below in shape-matched glyphs, the pixel wordmark beside it
        let mut v = moai_view(0.08 * (t * 0.4).sin(), 0.0);
        v.glow = 0.8 + 0.2 * (t * 1.5).sin();
        let (mw, mh) = (44usize, 23usize);
        blit(
            buf,
            r.x,
            r.y,
            mw,
            &glyph::render(&v, mw, mh, Opts::new(Mode::Shape)),
        );
        let (ww, word) = wordmark(INK);
        blit(buf, r.x + mw as u16 + 3, r.y + 8, ww, &word);
        write(
            buf,
            r.x + mw as u16 + 3,
            r.y + 13,
            "keep your head in the code",
            MUTED,
        );
        write(
            buf,
            r.x,
            r.y + mh as u16,
            "A  monolith · shape-matched glyphs + pixel wordmark",
            CORAL,
        );

        // B: profile in edge glyphs
        let bx = r.x + 96;
        let mut p = View::new(Shape::Moai);
        p.yaw = 1.3;
        p.pitch = -0.15;
        p.glow = 0.0;
        let (pw, ph) = (30usize, 16usize);
        blit(
            buf,
            bx,
            r.y,
            pw,
            &glyph::render(&p, pw, ph, Opts::new(Mode::Edge)),
        );
        write(
            buf,
            bx,
            r.y + ph as u16 + 1,
            "B  profile · edge glyphs",
            CORAL,
        );
    }

    fn avatars(&self, buf: &mut Buffer, r: Rect) {
        let n = avatar_count();
        let per_row = ((r.width as usize + 3) / 10).max(1);
        self.per_row.set(per_row);
        let rows_fit = ((r.height as usize).saturating_sub(3) / 5).max(1);
        let first_row = (self.pick / per_row).saturating_sub(rows_fit - 1);
        let status = self.status.map(|k| STATUSES[k]);
        let s = status.unwrap_or(Status::Working);
        write(
            buf,
            r.x,
            r.y,
            &format!(
                "{:>3}/{n}  {}  ·  {}",
                self.pick + 1,
                avatar_at(self.pick).1,
                s.name()
            ),
            CORAL,
        );
        for i in first_row * per_row..n.min((first_row + rows_fit) * per_row) {
            let (col, row) = ((i % per_row) as u16, (i / per_row - first_row) as u16);
            let (x, y) = (r.x + col * 10, r.y + 2 + row * 5);
            let s = status.unwrap_or(if i < AVATARS.len() {
                STATUSES[i % STATUSES.len()]
            } else {
                Status::Working
            });
            let (v, o) = avatar(avatar_at(i).0, s, self.t, i as f32 * 1.7, self.frame);
            blit(buf, x, y, 7, &glyph::render(&v, 7, 4, o));
            write(
                buf,
                x,
                y + 4,
                &format!("{:03}", i + 1),
                if i == self.pick {
                    CORAL
                } else {
                    glyph::mix(BG, MUTED, 0.6)
                },
            );
        }
    }

    fn look(&self) -> regi::Look {
        regi::Look {
            pattern: self.rpat.map(|k| regi::PATTERNS[k]),
            speed: SPEEDS[self.speed],
        }
    }

    /// Successors of the regi sprites: flat frames, then 3d wireframes; the eyes talk. The new
    /// ones after a blank row.
    fn regis(&self, buf: &mut Buffer, r: Rect) {
        let all = regi::all();
        let look = self.look();
        let per_row = ((r.width as usize + 3) / 10).max(1);
        self.per_row.set(per_row);
        let kept = all.iter().filter(|x| !x.new).count();
        let slot = |i: usize| {
            if i < kept {
                i
            } else {
                kept.div_ceil(per_row) * per_row + per_row + i - kept
            }
        };
        let rows_fit = ((r.height as usize).saturating_sub(3) / 5).max(1);
        let first_row = (slot(self.rpick) / per_row).saturating_sub(rows_fit - 1);
        let status = self.status;
        let head = format!(
            "{:>3}/{}  {}  ·  {}  ·  eyes: {}  ·  float {}x",
            self.rpick + 1,
            all.len(),
            all[self.rpick].name(&look),
            status.map(|k| STATUSES[k].name()).unwrap_or("mixed"),
            look.pattern.map(|p| p.name()).unwrap_or("each its own"),
            look.speed
        );
        write(buf, r.x, r.y, &head, CORAL);
        for (i, a) in all.iter().enumerate() {
            let (row, col) = (slot(i) / per_row, slot(i) % per_row);
            if row < first_row || row >= first_row + rows_fit {
                continue;
            }
            let (x, y) = (
                r.x + col as u16 * 10,
                r.y + 2 + (row - first_row) as u16 * 5,
            );
            let s = status.unwrap_or(i % STATUSES.len());
            blit(buf, x, y, 7, &regi::render(a, s, self.t, &look));
            let label = format!("{:03}{}", i + 1, if a.new { " new" } else { "" });
            write(
                buf,
                x,
                y + 4,
                &label,
                if i == self.rpick {
                    CORAL
                } else {
                    glyph::mix(BG, MUTED, 0.6)
                },
            );
        }
    }

    /// Line modes for the regi float, side by side: 1 column per mode, 1 row per avatar (REGI's,
    /// then 4 random). Under each: the changes of the last FWINDOW s, the dots per change (a
    /// whole-outline jump: many dots at once, then a long hold).
    fn floats(&mut self, buf: &mut Buffer, r: Rect) {
        let s = self.status.unwrap_or(2);
        let key = (self.fseed, self.status, self.speed);
        if key != self.fkey {
            self.fkey = key;
            self.fhist.clear();
        }
        let picks = float_picks(self.fseed);
        let speed = SPEEDS[self.speed];
        let head = format!("float · line modes  ·  {}  ·  float {}x  ·  REGI + 4 random", STATUSES[s].name(), speed);
        write(buf, r.x, r.y, &head, CORAL);
        write(buf, r.x, r.y + 1, "under each: changes in the last 4 s · steps per change (dots flipped + cells retoned; many steps, few changes: jumps)", MUTED);
        let (lw, cw) = (24u16, 20u16);
        // the modes in view: as many as fit, from fcol (←→)
        let fit = ((r.width.saturating_sub(lw) / cw) as usize).clamp(1, regi::LINES.len());
        let first = self.fcol.min(regi::LINES.len() - fit);
        let shown = first..first + fit;
        if fit < regi::LINES.len() {
            let more = format!("modes {}-{} of {} (←→)", first + 1, first + fit, regi::LINES.len());
            write(buf, r.x, r.y + 2, &more, MUTED);
        }
        for (c, lines) in regi::LINES.iter().enumerate().filter(|(c, _)| shown.contains(c)) {
            let x = r.x + lw + (c - first) as u16 * cw;
            write(buf, x, r.y + 2, lines.name(), INK);
            write(buf, x, r.y + 3, lines.about(), MUTED);
        }
        let dim = glyph::mix(BG, MUTED, 0.6);
        for (row, &i) in picks.iter().enumerate() {
            let a = &regi::all()[i];
            let y = r.y + 5 + row as u16 * 6;
            if y + 5 > r.bottom() {
                break;
            }
            let eyes = (row == 0).then_some(REGI_AVATAR.1);
            let look = regi::Look { pattern: eyes.map(|k| regi::PATTERNS[k]), speed };
            let regi::Kind::Solid(solid, motion) = a.kind;
            let title = if row == 0 { format!("REGI · regi:{i}") } else { format!("regi:{i}") };
            write(buf, r.x, y, &title, if row == 0 { CORAL } else { INK });
            write(buf, r.x, y + 1, solid.name(), MUTED);
            write(buf, r.x, y + 2, &format!("{motion:?}").to_lowercase(), MUTED);
            write(buf, r.x, y + 3, look.pattern.unwrap_or(a.pattern).name(), dim);
            for (c, &lines) in regi::LINES.iter().enumerate().filter(|(c, _)| shown.contains(c)) {
                let x = r.x + lw + (c - first) as u16 * cw;
                let cells = regi::render_lines(a, s, self.t, &look, lines);
                blit(buf, x, y, 7, &cells);
                let (prev, log) = self.fhist.entry((row, c)).or_default();
                if !prev.is_empty() {
                    let n: u32 = cells.iter().zip(prev.iter()).map(|(a, b)| ((a.ch as u32) ^ (b.ch as u32)).count_ones() + u32::from(a.ch == b.ch && a.fg != b.fg)).sum();
                    if n > 0 {
                        log.push_back((self.t, n));
                    }
                }
                *prev = cells;
                while log.front().is_some_and(|&(t, _)| t < self.t - FWINDOW) {
                    log.pop_front();
                }
                let dots: u32 = log.iter().map(|&(_, n)| n).sum();
                let per = if log.is_empty() { 0.0 } else { dots as f32 / log.len() as f32 };
                write(buf, x, y + 4, &format!("{:>2} ch · {per:.1} steps", log.len()), dim);
            }
        }
    }

    /// Spawn animations: the agent forms out of braille goo, snaps in, holds, starts over.
    fn spawns(&self, buf: &mut Buffer, r: Rect) {
        let (cw, ch) = (spawn::CW as u16 + 2, spawn::CH as u16 + 3);
        let per_row = ((r.width + 2) / cw).max(1) as usize;
        self.per_row.set(per_row);
        let rows_fit = ((r.height.saturating_sub(2)) / ch).max(1) as usize;
        let first_row = (self.spick / per_row).saturating_sub(rows_fit - 1);
        // 1 agent for all: the pick of the regi page
        let star = regi::star();
        let live = |s: f32| regi::render(&star, 2, s, &regi::Look::default());
        // the goo forms the 1st frame after the snap
        let rest = live(0.0);
        let n = spawn::count();
        let head = format!(
            "{:>2}/{n}  {}  ·  {}  ·  loop {:.1} s",
            self.spick + 1,
            spawn::name(self.spick),
            star.name(&regi::Look::default()),
            spawn::PERIOD
        );
        write(buf, r.x, r.y, &head, CORAL);
        for i in first_row * per_row..n.min((first_row + rows_fit) * per_row) {
            let (col, row) = ((i % per_row) as u16, (i / per_row - first_row) as u16);
            let (x, y) = (r.x + col * cw, r.y + 2 + row * ch);
            blit(
                buf,
                x,
                y,
                spawn::CW,
                &spawn::render(i, &rest, &live, self.t),
            );
            let fg = if i == self.spick {
                CORAL
            } else {
                glyph::mix(BG, MUTED, 0.6)
            };
            let (fam, var) = spawn::label(i);
            let cut = |s: String| -> String { s.chars().take(spawn::CW + 1).collect() };
            write(
                buf,
                x,
                y + spawn::CH as u16,
                &cut(format!("{:02} {fam}", i + 1)),
                fg,
            );
            write(
                buf,
                x + 3,
                y + spawn::CH as u16 + 1,
                &cut(var.to_string()),
                fg,
            );
        }
    }

    /// Monoliths: 1 design at a time, the moai page's render modes.
    fn monolith(&self, buf: &mut Buffer, r: Rect) {
        let all = mono::all();
        mono::STATUS.store(
            self.status.unwrap_or(0),
            std::sync::atomic::Ordering::Relaxed,
        );
        let mode = MODES[self.mode];
        let h = r.height.saturating_sub(3) as usize;
        let w = (h * 2).min(r.width as usize);
        let idx = self.mpick * regi::PATTERNS.len() + self.epat;
        let mut v = View::new(Shape::Mono(idx as u16));
        v.yaw = 0.35 + self.yaw + 0.3 * (self.t * 0.35).sin();
        v.pitch = -0.2 + self.pitch;
        v.light = sdf::V(0.35, 0.9, 0.35).norm();
        v.rim = 0.04;
        v.t = self.t;
        let mut o = Opts::new(mode);
        o.tint = self.tint;
        let cells = glyph::render(&v, w, h, o);
        let x0 = r.x + (r.width - w as u16) / 2;
        blit(buf, x0, r.y, w, &cells);
        let status = STATUSES[self.status.unwrap_or(0)].name();
        let label = format!(
            "{:>2}/{}  {}  ·  {}  ·  {}",
            self.mpick + 1,
            mono::BODIES.len(),
            all[idx].name(),
            status,
            mode.name()
        );
        write(
            buf,
            r.x + (r.width.saturating_sub(label.chars().count() as u16)) / 2,
            r.y + h as u16 + 1,
            &label,
            MUTED,
        );
    }

    fn loader_page(&mut self, buf: &mut Buffer, r: Rect) {
        for (i, (name, about)) in fx::NAMES.iter().enumerate() {
            let y = r.y + i as u16 * 2;
            write(buf, r.x, y, name, INK);
            let row = self.loaders.draw(i, self.t);
            blit(buf, r.x + 14, y, row.len().max(1), &row);
            write(buf, r.x + 14 + 34, y, about, MUTED);
        }
    }
}

/// The moai from below, 3/4 turned: YAW 0 = the default angle.
pub fn moai_view(yaw: f32, pitch: f32) -> View {
    let mut v = View::new(Shape::Moai);
    v.yaw = 0.22 + yaw;
    v.pitch = -0.22 + pitch;
    v.light = sdf::V(0.35, 0.9, 0.35).norm();
    v
}

/// Frames for the website hero: frames <mode> <cols> <rows> <n> → JSON on stdout. 1 frame per
/// turn step from -0.6 to 0.6 rad; per frame the glyphs and 1 tone digit per cell (0-5 by
/// brightness, 6 an eye, space: blank).
fn frames(args: &[String]) {
    let mode = MODES
        .iter()
        .find(|m| Some(m.name()) == args.first().map(|s| s.as_str()))
        .copied()
        .unwrap_or(Mode::Edge);
    let num = |i: usize, d: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (w, h, n) = (num(1, 64), num(2, 34), num(3, 41));
    let mut out = format!("{{\"cols\":{w},\"rows\":{h},\"yaw\":[-0.6,0.6],\"frames\":[");
    for f in 0..n {
        let yaw = -0.6 + 1.2 * f as f32 / (n - 1).max(1) as f32;
        let cells = glyph::render(&moai_view(yaw, 0.0), w, h, Opts::new(mode));
        let lum = |c: Rgb| 0.3 * c.0 as f32 + 0.59 * c.1 as f32 + 0.11 * c.2 as f32;
        let (lo, hi) = (lum((70, 42, 30)), lum(INK));
        let (mut text, mut tone) = (String::new(), String::new());
        for (i, c) in cells.iter().enumerate() {
            if i > 0 && i % w == 0 {
                text.push('\n');
                tone.push('\n');
            }
            text.push(c.ch);
            tone.push(if c.ch == ' ' {
                ' '
            } else if c.fg.0 as i32 - c.fg.1 as i32 > 80 {
                '6'
            } else {
                char::from(
                    b'0' + (((lum(c.fg) - lo) / (hi - lo)).clamp(0.0, 1.0) * 5.0).round() as u8,
                )
            });
        }
        let esc = |s: &str| {
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        };
        out += &format!(
            "{}{{\"c\":\"{}\",\"t\":\"{}\"}}",
            if f > 0 { "," } else { "" },
            esc(&text),
            esc(&tone)
        );
    }
    out += "]}";
    println!("{out}");
}

/// The website's crew: avatar-frames <fps> <seconds> <id>... → JSON on stdout. Per avatar id
/// (`geo:40`, `regi:12 eyes=3`): its name, per status FPS*SECONDS frames of 4 rows (7 cells),
/// and 1 spawn (the goo forming the sprite, 13x6 cells, then 0.5 s of the idle avatar).
fn avatar_frames(args: &[String]) {
    use avatar::{Avatar, STATUSES, spawn};
    let fps: f32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let secs: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let n = (fps * secs) as usize;
    let rows_of = |cells: &[avatar::glyph::Cell], w: usize| -> Vec<String> { cells.chunks(w).map(|r| r.iter().map(|c| c.ch).collect()).collect() };
    let mut list = Vec::new();
    for (k, id) in args.iter().skip(2).enumerate() {
        let Some(a) = Avatar::parse(id) else {
            eprintln!("not an avatar id: {id}");
            continue;
        };
        let mut st = serde_json::Map::new();
        for s in STATUSES {
            let fr: Vec<Vec<String>> = (0..n).map(|i| rows_of(&a.cells(s, i as f32 / fps), avatar::W)).collect();
            st.insert(s.name().into(), serde_json::json!(fr));
        }
        let anim = (k * 7 + 3) % spawn::count();
        let snap = spawn::timing(anim).2;
        let target = a.cells(avatar::Status::Idle, snap);
        let live = |t: f32| a.cells(avatar::Status::Idle, snap + t);
        let m = ((snap + 0.15) * fps).ceil() as usize;
        let sp: Vec<Vec<String>> = (0..m).map(|i| rows_of(&spawn::render(anim, &target, &live, i as f32 / fps), spawn::CW)).collect();
        list.push(serde_json::json!({ "id": id, "name": a.name(), "status": st, "spawn": sp, "at": avatar::SPAWN_AT }));
    }
    println!("{}", serde_json::json!({ "fps": fps, "w": avatar::W, "h": avatar::H, "sw": spawn::CW, "sh": spawn::CH, "avatars": list }));
}

/// The website's monolith: monolith-frames <body> <cols> <rows> <n> → JSON on stdout. Braille.
/// Per eye pattern, per turn step (-0.5..0.5 rad): the stone with every eye off (its sockets), as
/// glyphs + 1 tone digit per cell (moai `frames`), and per eye dot the cells it lights (index,
/// glyph). The page plays the talk of the dots live (status, pattern: hotkeys).
fn monolith_frames(args: &[String]) {
    use std::sync::atomic::Ordering;
    let num = |i: usize, d: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (body, w, h, n) = (num(0, 6), num(1, 64), num(2, 34), num(3, 21));
    let lum = |c: Rgb| 0.3 * c.0 as f32 + 0.59 * c.1 as f32 + 0.11 * c.2 as f32;
    let (lo, hi) = (lum((70, 42, 30)), lum(INK));
    let tone = |c: &Cell| -> char {
        if c.ch == ' ' || c.ch == '\u{2800}' {
            ' '
        } else if c.fg.0 as i32 - c.fg.1 as i32 > 80 {
            '6'
        } else {
            char::from(b'0' + (((lum(c.fg) - lo) / (hi - lo)).clamp(0.0, 1.0) * 5.0).round() as u8)
        }
    };
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    };
    let mut out = format!(
        "{{\"cols\":{w},\"rows\":{h},\"yaw\":[-0.5,0.5],\"body\":\"{}\",\"patterns\":[",
        mono::BODIES[body].name()
    );
    for (pi, pat) in regi::PATTERNS.iter().enumerate() {
        let m = &mono::all()[body * regi::PATTERNS.len() + pi];
        let dots: Vec<String> = pat
            .dots()
            .iter()
            .map(|(x, y)| format!("[{x},{y}]"))
            .collect();
        out += &format!(
            "{}{{\"name\":\"{}\",\"talk\":\"{}\",\"dots\":[{}],\"frames\":[",
            if pi > 0 { "," } else { "" },
            pat.name(),
            m.talk.name(),
            dots.join(",")
        );
        for f in 0..n {
            let mut v = View::new(Shape::Mono((body * regi::PATTERNS.len() + pi) as u16));
            v.yaw = 0.35 - 0.5 + f as f32 / (n - 1).max(1) as f32;
            v.pitch = -0.2;
            v.light = sdf::V(0.35, 0.9, 0.35).norm();
            v.rim = 0.04;
            let o = Opts::new(Mode::Braille);
            mono::FORCE.store(2, Ordering::Relaxed);
            let off = glyph::render(&v, w, h, o);
            mono::FORCE.store(1, Ordering::Relaxed);
            let on = glyph::render(&v, w, h, o);
            mono::FORCE.store(0, Ordering::Relaxed);
            // each changed cell to its nearest eye dot
            let centers: Vec<(f32, f32)> = m
                .eyes()
                .into_iter()
                .map(|c| {
                    let (u, vv) = v.project(c);
                    (u * h as f32 + w as f32 / 2.0, (1.0 - vv) * h as f32 / 2.0)
                })
                .collect();
            let mut eyes: Vec<Vec<String>> = vec![Vec::new(); centers.len()];
            for (i, (a, b)) in off.iter().zip(&on).enumerate() {
                if a.ch == b.ch && a.fg == b.fg {
                    continue;
                }
                let (cx, cy) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                let k = (0..centers.len()).min_by(|&p, &q| {
                    let d = |c: (f32, f32)| (c.0 - cx).powi(2) + (c.1 - cy).powi(2) * 4.0;
                    d(centers[p]).total_cmp(&d(centers[q]))
                });
                if let Some(k) = k {
                    eyes[k].push(format!("[{i},\"{}\"]", esc(&b.ch.to_string())));
                }
            }
            let (mut text, mut tones) = (String::new(), String::new());
            for (i, c) in off.iter().enumerate() {
                if i > 0 && i % w == 0 {
                    text.push('\n');
                    tones.push('\n');
                }
                text.push(c.ch);
                tones.push(tone(c));
            }
            let eyes: Vec<String> = eyes.iter().map(|e| format!("[{}]", e.join(","))).collect();
            out += &format!(
                "{}{{\"c\":\"{}\",\"t\":\"{}\",\"e\":[{}]}}",
                if f > 0 { "," } else { "" },
                esc(&text),
                esc(&tones),
                eyes.join(",")
            );
        }
        out += "]}";
    }
    out += "]}";
    println!("{out}");
}

/// The favicon: the moai in braille, seen from below in profile, as an SVG of dots on the
/// paper color. favicon <rows> <yaw> <pitch> <zoom> <focus>: rows of cells, 2x as many columns,
/// so the dot grid is square.
fn favicon(args: &[String]) {
    let num = |i: usize, d: f32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let h = num(0, 8.0) as usize;
    let w = h * 2;
    let mut v = View::new(Shape::Moai);
    v.yaw = num(1, 1.05);
    v.pitch = num(2, -0.25);
    v.zoom = num(3, 1.3);
    v.focus = num(4, 0.3);
    v.light = sdf::V(0.6, 0.6, 0.5).norm();
    let cells = glyph::render(&v, w, h, Opts::new(Mode::Braille));
    const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
    let n = w * 2;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {n} {n}\"><rect width=\"{n}\" height=\"{n}\" fill=\"#150700\"/>"
    );
    for (i, c) in cells.iter().enumerate() {
        let bits = (c.ch as u32).wrapping_sub(0x2800);
        if bits > 0xFF {
            continue;
        }
        let fill = format!("#{:02x}{:02x}{:02x}", c.fg.0, c.fg.1, c.fg.2);
        for dy in 0..4 {
            for dx in 0..2 {
                if bits & BIT[dy][dx] != 0 {
                    let (x, y) = ((i % w) * 2 + dx, (i / w) * 4 + dy);
                    out += &format!(
                        "<circle cx=\"{}.5\" cy=\"{}.5\" r=\".42\" fill=\"{fill}\"/>",
                        x, y
                    );
                }
            }
        }
    }
    out += "</svg>";
    println!("{out}");
}

fn print_frame(args: &[String]) {
    let mode = MODES
        .iter()
        .find(|m| Some(m.name()) == args.first().map(|s| s.as_str()))
        .copied()
        .unwrap_or(Mode::Shape);
    let w: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(72);
    let h: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(34);
    let v = moai_view(args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0), 0.0);
    let mut o = Opts::new(mode);
    let env = |k: &str, d: f32| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    o.ink = (
        env("INK0", o.ink.0),
        env("INK1", o.ink.1),
        env("FLOOR", o.ink.2),
    );
    o.contrast = env("K", o.contrast);
    o.crease = env("CREASE", o.crease);
    let cells = glyph::render(&v, w, h, o);
    let mut out = std::io::stdout().lock();
    for row in cells.chunks(w) {
        for c in row {
            let _ = match c.bg {
                Some(b) => write!(
                    out,
                    "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m{}",
                    c.fg.0, c.fg.1, c.fg.2, b.0, b.1, b.2, c.ch
                ),
                None => write!(
                    out,
                    "\x1b[38;2;{};{};{}m\x1b[49m{}",
                    c.fg.0, c.fg.1, c.fg.2, c.ch
                ),
            };
        }
        let _ = writeln!(out, "\x1b[0m");
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("favicon") {
        favicon(&args[1..]);
        return Ok(());
    }
    if args.first().map(|s| s.as_str()) == Some("monolith-frames") {
        monolith_frames(&args[1..]);
        return Ok(());
    }
    if args.first().map(|s| s.as_str()) == Some("avatar-frames") {
        avatar_frames(&args[1..]);
        return Ok(());
    }
    if args.first().map(|s| s.as_str()) == Some("frames") {
        frames(&args[1..]);
        return Ok(());
    }
    if args.first().map(|s| s.as_str()) == Some("ppm") {
        // the raw shading as a P6 image: ppm <size> <yaw>
        let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(320);
        let v = moai_view(args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0), 0.0);
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "P6 {n} {n} 255");
        for y in 0..n {
            for x in 0..n {
                let p = v.sample(
                    2.0 * x as f32 / n as f32 - 1.0,
                    1.0 - 2.0 * y as f32 / n as f32,
                );
                let c = if p.hit {
                    glyph::color(Tint::Ink, &p)
                } else {
                    BG
                };
                let _ = out.write_all(&[c.0, c.1, c.2]);
            }
        }
        return Ok(());
    }
    if args.first().map(|s| s.as_str()) == Some("print") {
        print_frame(&args[1..]);
        return Ok(());
    }
    let mut app = App {
        page: 0,
        mode: 0,
        tint: Tint::Ink,
        paused: false,
        t: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        status: None,
        pick: 0,
        rpick: 0,
        spick: 0,
        mpick: 0,
        epat: 0,
        rpat: None,
        speed: 0,
        per_row: std::cell::Cell::new(1),
        loaders: fx::Loaders::new(),
        frame: 0,
        fseed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        fhist: Default::default(),
        fkey: (0, None, 0),
        fcol: 0,
        bsize: 0,
        blayout: 0,
        bline: 0,
        ttail: 0,
        tcolor: 0,
    };
    if args.first().map(|s| s.as_str()) == Some("page") {
        // 1 page as plain text: page <1-9> <t> [w h] [pick] [status 1-5] [eyes: monolith 0-10, regi 1-11]
        let num = |i: usize, d: f32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
        app.page = (num(1, 1.0) as usize)
            .saturating_sub(1)
            .min(PAGES.len() - 1);
        app.t = num(2, 0.0);
        let (w, h) = (num(3, 160.0) as u16, num(4, 40.0) as u16);
        let pick = num(5, 0.0) as usize;
        (app.pick, app.rpick, app.spick, app.mpick) = (pick, pick, pick, pick % mono::BODIES.len());
        // big page: pick = size * 2 + layout
        (app.bsize, app.blayout) = ((pick / 2) % big::SIZES.len(), pick % 2);
        // tokens page: pick = color * 3 + tail
        (app.ttail, app.tcolor) = (pick % tokens::TAILS.len(), (pick / tokens::TAILS.len()) % tokens::COLORS.len());
        app.epat = num(7, 0.0) as usize % regi::PATTERNS.len();
        app.rpat = (num(7, 0.0) as usize)
            .checked_sub(1)
            .filter(|_| app.page == 4)
            .map(|k| k % regi::PATTERNS.len());
        app.status = (num(6, 0.0) as usize).checked_sub(1);
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        app.draw(&mut buf, area);
        for y in 0..h {
            let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
            println!("{}", row.trim_end());
        }
        return Ok(());
    }
    let mut term = ratatui::init();
    let mut last = Instant::now();
    let res = loop {
        let now = Instant::now();
        if !app.paused {
            app.t += (now - last).as_secs_f32();
            app.frame += 1;
        }
        last = now;
        if let Err(e) = term.draw(|f| {
            let area = f.area();
            app.draw(f.buffer_mut(), area);
        }) {
            break Err(e);
        }
        if event::poll(Duration::from_millis(33))?
            && let Event::Key(k) = event::read()?
            && k.kind == KeyEventKind::Press
        {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                KeyCode::Char(c @ '1'..='9') => app.page = c as usize - '1' as usize,
                KeyCode::Char('0') => app.page = 9,
                KeyCode::Char('v') if app.page == 9 => app.ttail = (app.ttail + 1) % tokens::TAILS.len(),
                KeyCode::Char('c') if app.page == 9 => app.tcolor = (app.tcolor + 1) % tokens::COLORS.len(),
                KeyCode::Tab => app.page = (app.page + 1) % PAGES.len(),
                KeyCode::BackTab => app.page = (app.page + PAGES.len() - 1) % PAGES.len(),
                KeyCode::Char('m') => app.mode = (app.mode + 1) % MODES.len(),
                KeyCode::Char('t') => {
                    app.tint = match app.tint {
                        Tint::Ink => Tint::Coral,
                        _ => Tint::Ink,
                    }
                }
                KeyCode::Char('s') => {
                    app.status = match app.status {
                        None => Some(0),
                        Some(k) if k + 1 < STATUSES.len() => Some(k + 1),
                        _ => None,
                    }
                }
                KeyCode::Char(' ') => app.paused = !app.paused,
                KeyCode::Left if app.page == 4 => app.rpick = app.rpick.saturating_sub(1),
                KeyCode::Right if app.page == 4 => {
                    app.rpick = (app.rpick + 1).min(regi::all().len() - 1)
                }
                KeyCode::Up if app.page == 4 => {
                    app.rpick = app.rpick.saturating_sub(app.per_row.get())
                }
                KeyCode::Down if app.page == 4 => {
                    app.rpick = (app.rpick + app.per_row.get()).min(regi::all().len() - 1)
                }
                KeyCode::Left if app.page == 5 => app.spick = app.spick.saturating_sub(1),
                KeyCode::Right if app.page == 5 => {
                    app.spick = (app.spick + 1).min(spawn::count() - 1)
                }
                KeyCode::Up if app.page == 5 => {
                    app.spick = app.spick.saturating_sub(app.per_row.get())
                }
                KeyCode::Down if app.page == 5 => {
                    app.spick = (app.spick + app.per_row.get()).min(spawn::count() - 1)
                }
                KeyCode::Left if app.page == 8 => app.bsize = app.bsize.saturating_sub(1),
                KeyCode::Right if app.page == 8 => app.bsize = (app.bsize + 1).min(big::SIZES.len() - 1),
                KeyCode::Up if app.page == 8 => app.blayout = app.blayout.saturating_sub(1),
                KeyCode::Down if app.page == 8 => app.blayout = (app.blayout + 1).min(big::LAYOUTS[0].len() - 1),
                KeyCode::Char('l') if app.page == 8 => app.bline = (app.bline + 1) % regi::LINES.len(),
                KeyCode::Char('r') if app.page == 8 => app.fseed = app.fseed.wrapping_add(1),
                KeyCode::Left if app.page == 7 => app.fcol = app.fcol.saturating_sub(1),
                KeyCode::Right if app.page == 7 => app.fcol = (app.fcol + 1).min(regi::LINES.len() - 1),
                KeyCode::Left if app.page == 6 => {
                    app.mpick = (app.mpick + mono::BODIES.len() - 1) % mono::BODIES.len()
                }
                KeyCode::Right if app.page == 6 => app.mpick = (app.mpick + 1) % mono::BODIES.len(),
                KeyCode::Char('e') if app.page == 4 => {
                    app.rpat = match app.rpat {
                        None => Some(0),
                        Some(k) if k + 1 < regi::PATTERNS.len() => Some(k + 1),
                        _ => None,
                    }
                }
                KeyCode::Char('e') => app.epat = (app.epat + 1) % regi::PATTERNS.len(),
                KeyCode::Char('r') if app.page == 7 => app.fseed = app.fseed.wrapping_add(1),
                KeyCode::Char('f') => app.speed = (app.speed + 1) % SPEEDS.len(),
                KeyCode::Left if app.page == 2 => app.pick = app.pick.saturating_sub(1),
                KeyCode::Right if app.page == 2 => {
                    app.pick = (app.pick + 1).min(avatar_count() - 1)
                }
                KeyCode::Up if app.page == 2 => {
                    app.pick = app.pick.saturating_sub(app.per_row.get())
                }
                KeyCode::Down if app.page == 2 => {
                    app.pick = (app.pick + app.per_row.get()).min(avatar_count() - 1)
                }
                KeyCode::Left => app.yaw -= 0.15,
                KeyCode::Right => app.yaw += 0.15,
                KeyCode::Up => app.pitch -= 0.1,
                KeyCode::Down => app.pitch += 0.1,
                _ => {}
            }
        }
    };
    ratatui::restore();
    res
}
