//! Agent avatars: animated 7x4 braille portraits, 2 categories (the files are shared with the lab,
//! examples/lab):
//! - regi: the regi successors (regi.rs): a 1-dot braille frame or wireframe solid, eye dots
//!   that talk and blink by status, the solids float
//! - geo: ray marched solids (sdf.rs, geo.rs) in braille, each with its own motion
//!
//! An avatar: a category, an index, an eye pattern (regi; None: its own) and a float speed.
//! Saved as `regi:12`, `regi:12 eyes=3 speed=0.5`, `geo:40 speed=2`.
//! Spawn: spawn.rs, the agent forming out of braille goo on a 13x6 canvas.
#![allow(dead_code)]

pub mod geo;
pub mod glyph;
pub mod glyphs;
pub mod mono;
pub mod regi;
pub mod sdf;
pub mod spawn;

use std::sync::OnceLock;
use std::time::Instant;

pub const W: usize = 7;
pub const H: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
    Regi,
    Geo,
}

pub const CATS: [Cat; 2] = [Cat::Regi, Cat::Geo];

impl Cat {
    pub fn name(self) -> &'static str {
        match self {
            Cat::Regi => "regi",
            Cat::Geo => "geo",
        }
    }

    pub fn count(self) -> usize {
        match self {
            Cat::Regi => regi::all().len(),
            Cat::Geo => sdf::AVATARS.len() + geo::all().len(),
        }
    }

    fn parse(s: &str) -> Option<Cat> {
        CATS.into_iter().find(|c| c.name() == s)
    }
}

/// Status an avatar acts out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    Working,
    Thinking,
    #[default]
    Idle,
    Blocked,
    Done,
}

pub const STATUSES: [Status; 5] = [Status::Working, Status::Thinking, Status::Idle, Status::Blocked, Status::Done];

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::Thinking => "thinking",
            Status::Idle => "idle",
            Status::Blocked => "blocked",
            Status::Done => "done",
        }
    }

    fn index(self) -> usize {
        STATUSES.iter().position(|s| *s == self).unwrap_or(2)
    }
}

/// Float speeds the picker cycles through.
pub const SPEEDS: [f32; 5] = [1.0, 0.5, 0.25, 2.0, 3.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    pub cat: Cat,
    pub idx: usize,
    /// Regi: index into regi::PATTERNS (None: the avatar's own).
    pub eyes: Option<usize>,
    pub speed: f32,
}

impl Default for Avatar {
    fn default() -> Avatar {
        Avatar { cat: Cat::Regi, idx: 0, eyes: None, speed: 1.0 }
    }
}

impl Avatar {
    /// `regi:12 eyes=3 speed=0.5`. None: not an avatar id (an old sprite id, `custom`).
    pub fn parse(id: &str) -> Option<Avatar> {
        let mut words = id.split_whitespace();
        let (cat, idx) = words.next()?.split_once(':')?;
        let cat = Cat::parse(cat)?;
        let idx: usize = idx.parse().ok()?;
        if idx >= cat.count() {
            return None;
        }
        let mut a = Avatar { cat, idx, eyes: None, speed: 1.0 };
        for w in words {
            match w.split_once('=') {
                Some(("eyes", v)) => a.eyes = v.parse().ok().filter(|&k| k < regi::PATTERNS.len()),
                Some(("speed", v)) => a.speed = v.parse().ok().filter(|s: &f32| s.is_finite() && *s > 0.0).unwrap_or(1.0),
                _ => {}
            }
        }
        Some(a)
    }

    pub fn id(&self) -> String {
        let mut s = format!("{}:{}", self.cat.name(), self.idx);
        if let Some(e) = self.eyes {
            s += &format!(" eyes={e}");
        }
        if self.speed != 1.0 {
            s += &format!(" speed={}", self.speed);
        }
        s
    }

    /// Name for the picker: the shape, the eyes.
    pub fn name(&self) -> String {
        match self.cat {
            Cat::Regi => regi::all()[self.idx].name(&self.look()),
            Cat::Geo => geo_at(self.idx).1,
        }
    }

    fn look(&self) -> regi::Look {
        regi::Look { pattern: self.eyes.map(|k| regi::PATTERNS[k]), speed: self.speed }
    }

    /// The 4 rows (7 cells each) at time T (s) for status S.
    pub fn rows(&self, s: Status, t: f32) -> [String; H] {
        rows(&self.cells(s, t))
    }

    fn cells(&self, s: Status, t: f32) -> Vec<glyph::Cell> {
        match self.cat {
            Cat::Regi => regi::render(&regi::all()[self.idx], s.index(), t, &self.look()),
            Cat::Geo => {
                let (v, o) = geo_view(geo_at(self.idx).0, s, t * self.speed, self.idx as f32 * 1.7, (t * 30.0) as usize);
                glyph::render(&v, W, H, o)
            }
        }
    }
}

fn rows(cells: &[glyph::Cell]) -> [String; H] {
    std::array::from_fn(|r| cells[r * W..(r + 1) * W].iter().map(|c| c.ch).collect())
}

/// Seconds since the first call: the clock of every avatar.
pub fn now() -> f32 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f32()
}

fn geo_at(i: usize) -> (sdf::Shape, String) {
    let n = sdf::AVATARS.len();
    if i < n { (sdf::AVATARS[i], sdf::AVATARS[i].name().to_string()) } else { (sdf::Shape::Geo((i - n) as u16), geo::all()[i - n].name.clone()) }
}

/// A geo avatar at time T in braille: the view and render options of status S. The 6 first
/// solids turn by status; geo shapes play their own motion, slowed or stopped by status.
fn geo_view(shape: sdf::Shape, s: Status, t: f32, phase: f32, frame: usize) -> (sdf::View, glyph::Opts) {
    use sdf::Shape;
    let mut v = sdf::View::new(shape);
    let mut o = glyph::Opts::new(glyph::Mode::Braille);
    o.floor = 0.3;
    v.zoom = 1.15;
    let tt = match s {
        Status::Working => t,
        Status::Thinking => t * 0.6,
        Status::Idle => t * 0.25,
        Status::Blocked => 1.0 + phase,
        Status::Done => phase,
    };
    v.t = tt;
    if let Shape::Geo(i) = shape {
        (v.yaw, v.pitch, v.roll) = geo::all()[i as usize].pose(tt);
    } else {
        v.pitch = if matches!(shape, Shape::Monolith | Shape::Pillar | Shape::Pyramid) { 0.15 } else { 0.45 };
        v.yaw = match s {
            Status::Working => t * 1.8 + phase,
            Status::Thinking => 0.5 * (t * 0.7 + phase).sin(),
            Status::Idle => 0.35 * (t * 0.5 + phase).sin(),
            _ => 0.4,
        };
    }
    match s {
        Status::Thinking => o.jitter = frame / 3,
        Status::Blocked if (t * 0.8 + phase).fract() < 0.25 => v.yaw += (t * 45.0).sin() * 0.12,
        Status::Done => o.gain = 0.55,
        _ => {}
    }
    (v, o)
}

/// A spawn under way: animation ANIM of avatar A, started at START (now()).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn {
    pub anim: usize,
    pub start: f32,
}

/// Seconds of the text decode after a spawn.
pub const REVEAL: f32 = 0.35;

/// Line SPANS, decoded to progress P (0..1): noise resolves left to right (the lab `decode`).
/// Not yet reached: dim dots; the noise band: NOISE color; resolved: the span styles.
pub fn decode(spans: Vec<ratatui::text::Span<'static>>, p: f32, noise: ratatui::style::Style, dim: ratatui::style::Style) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::text::Span;
    if p >= 1.0 {
        return spans;
    }
    const NOISE: [char; 10] = ['░', '▒', '▓', '/', '\\', '|', '-', '+', '*', '#'];
    const BAND: f32 = 0.15;
    let len = spans.iter().map(|s| s.content.chars().count()).sum::<usize>().max(1) as f32;
    let cyc = p * (1.0 + BAND);
    let frame = (now() * 30.0) as u64;
    let mut out = Vec::new();
    let mut i = 0usize;
    for s in spans {
        for ch in s.content.chars() {
            let at = i as f32 / len;
            let cell = if ch == ' ' {
                Span::styled(" ", s.style)
            } else if cyc > at + BAND {
                Span::styled(ch.to_string(), s.style)
            } else if cyc > at {
                let n = regi::hash(frame * 131 + i as u64);
                Span::styled(NOISE[(n * NOISE.len() as f32) as usize % NOISE.len()].to_string(), noise)
            } else {
                Span::styled("·", dim)
            };
            out.push(cell);
            i += 1;
        }
    }
    out
}

pub const SPAWN_W: usize = spawn::CW;
pub const SPAWN_H: usize = spawn::CH;
/// Cell of the 7x4 sprite in the spawn canvas.
pub const SPAWN_AT: (usize, usize) = (3, 1);

impl Spawn {
    /// A random animation, starting now.
    pub fn random() -> Spawn {
        Spawn::random_except(&[])
    }

    /// A random animation none of TAKEN (the ones playing), starting now.
    pub fn random_except(taken: &[usize]) -> Spawn {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as u64);
        let count = spawn::count();
        let mut anim = (regi::hash(n) * count as f32) as usize % count;
        // the next free one (all taken: any)
        for _ in 0..count {
            if !taken.contains(&anim) {
                break;
            }
            anim = (anim + 1) % count;
        }
        // tests read the screens: a spawn over them would hide the cards
        let start = if cfg!(test) { now() - 1000.0 } else { now() };
        Spawn { anim, start }
    }

    /// The flash after the snap included.
    pub fn done(&self) -> bool {
        now() - self.start > self.end()
    }

    /// Seconds from START to the end of the flash.
    fn end(&self) -> f32 {
        spawn::timing(self.anim).2 + 0.15
    }

    /// Progress 0..1 of the text decode after the spawn (0: spawn under way, 1: text shown).
    pub fn reveal(&self) -> f32 {
        ((now() - self.start - self.end()) / REVEAL).clamp(0.0, 1.0)
    }

    /// Spawn and text decode over.
    pub fn over(&self) -> bool {
        self.reveal() >= 1.0
    }

    /// The shape the goo forms: the frame of status S at the snap (the 1st frame the status
    /// animation shows after the spawn: 0 jump at the snap).
    fn target(&self, a: &Avatar, s: Status) -> Vec<glyph::Cell> {
        a.cells(s, self.start + spawn::timing(self.anim).2)
    }

    /// The 13x6 canvas rows of A (the sprite at SPAWN_AT), status S after the snap. None: over.
    pub fn rows(&self, a: &Avatar, s: Status) -> Option<[String; SPAWN_H]> {
        if self.done() {
            return None;
        }
        let live = |_: f32| a.cells(s, now());
        let cells = spawn::render(self.anim, &self.target(a, s), &live, now() - self.start);
        Some(std::array::from_fn(|r| cells[r * SPAWN_W..(r + 1) * SPAWN_W].iter().map(|c| c.ch).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let a = Avatar { cat: Cat::Geo, idx: 40, eyes: None, speed: 2.0 };
        assert_eq!(a.id(), "geo:40 speed=2");
        assert_eq!(Avatar::parse(&a.id()), Some(a));
        let b = Avatar { cat: Cat::Regi, idx: 3, eyes: Some(5), speed: 0.5 };
        assert_eq!(Avatar::parse(&b.id()), Some(b));
        assert_eq!(Avatar::parse("pcb-042"), None);
        assert_eq!(Avatar::parse("regi:99999"), None);
    }

    #[test]
    fn every_avatar_draws_7x4() {
        for cat in CATS {
            for idx in 0..cat.count() {
                let rows = Avatar { cat, idx, eyes: None, speed: 1.0 }.rows(Status::Working, 1.3);
                assert!(rows.iter().all(|r| r.chars().count() == W), "{cat:?} {idx}");
            }
        }
    }

    #[test]
    fn spawns_at_once_differ() {
        let mut taken = Vec::new();
        for _ in 0..spawn::count() {
            let s = Spawn::random_except(&taken);
            assert!(!taken.contains(&s.anim));
            taken.push(s.anim);
        }
    }

    #[test]
    fn a_spawn_ends_on_the_first_frame_of_the_status() {
        // the goo's last frame before the snap = the status animation's frame at the snap: 0 jump
        let blank = |c: char| c == ' ' || c == '\u{2800}';
        for (cat, idx, eyes) in [(Cat::Regi, 0, None), (Cat::Regi, 7, Some(3)), (Cat::Regi, 21, None), (Cat::Geo, 2, None)] {
            let a = Avatar { cat, idx, eyes, speed: 1.0 };
            for s in [Status::Idle, Status::Working, Status::Thinking, Status::Blocked, Status::Done] {
                for anim in 0..spawn::count() {
                    let sp = Spawn { anim, start: 3.7 };
                    let end = spawn::timing(anim).2;
                    let target = sp.target(&a, s);
                    assert!(target == a.cells(s, sp.start + end));
                    let goo = spawn::render(anim, &target, &|_| a.cells(s, sp.start + end), end - 0.01);
                    let (ax, ay) = SPAWN_AT;
                    let off = (0..W * H).filter(|&k| {
                        let (g, t) = (goo[(ay + k / W) * SPAWN_W + ax + k % W].ch, target[k].ch);
                        blank(g) != blank(t)
                    });
                    assert_eq!(off.count(), 0, "{cat:?} {idx} {s:?} anim {anim} ({})", spawn::name(anim));
                }
            }
        }
    }

    #[test]
    fn a_spawn_ends() {
        let s = Spawn { anim: 7, start: now() - 10.0 };
        assert!(s.done() && s.rows(&Avatar::default(), Status::Idle).is_none());
        let s = Spawn { anim: 7, start: now() };
        assert!(s.rows(&Avatar::default(), Status::Idle).is_some());
        assert_eq!(s.reveal(), 0.0);
    }

    #[test]
    fn decode_resolves() {
        use ratatui::{style::Style, text::Span};
        let line = || vec![Span::raw("  GIGAS")];
        let text = |v: Vec<Span<'static>>| v.iter().map(|s| s.content.to_string()).collect::<String>();
        assert_eq!(text(decode(line(), 1.0, Style::default(), Style::default())), "  GIGAS");
        assert_eq!(text(decode(line(), 0.0, Style::default(), Style::default())), "  ·····");
    }
}
