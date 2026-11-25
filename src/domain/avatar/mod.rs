//! Agent avatars: animated 7x4 braille portraits, 3 categories (the files are shared with the lab,
//! examples/lab):
//! - REGI: the regi successors (regi.rs): a 1-dot braille wireframe solid, eye dots that talk
//!   and blink by status, the solids float
//! - GEO A: ray marched solids (sdf.rs, geo.rs) in braille, each with its own motion
//! - GEO B: the GEO A solids as outlines: only the silhouette and the folds on, the areas empty
//!
//! An avatar: a category, an index, an eye pattern (regi; None: its own) and a float speed.
//! Saved as `regi:12`, `regi:12 eyes=3 speed=0.5`, `geo:40 speed=2`, `geob:40`.
//! Spawn: spawn.rs, the agent forming out of braille goo on a 13x6 canvas.

use std::fmt::Write as _;
pub(crate) mod geo;
pub(crate) mod glyph;
pub(crate) mod regi;
pub(crate) mod sdf;
pub(crate) mod spawn;

use std::ops::ControlFlow;
use std::sync::OnceLock;
use std::time::Instant;

pub(crate) const WIDTH: usize = 7;
pub(crate) const HEIGHT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cat {
    Regi,
    Geo,
    GeoB,
}

pub(crate) const CATS: [Cat; 3] = [Cat::Regi, Cat::Geo, Cat::GeoB];

impl Cat {
    /// The key in an avatar id.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Cat::Regi => "regi",
            Cat::Geo => "geo",
            Cat::GeoB => "geob",
        }
    }

    /// The name the picker shows.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Cat::Regi => "REGI",
            Cat::Geo => "GEO A",
            Cat::GeoB => "GEO B",
        }
    }

    pub(crate) fn count(self) -> usize {
        match self {
            Cat::Regi => regi::all().len(),
            Cat::Geo | Cat::GeoB => sdf::AVATARS.len() + geo::all().len(),
        }
    }

    fn parse(text: &str) -> Option<Cat> {
        CATS.into_iter().find(|character| character.name() == text)
    }
}

/// Status an avatar acts out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Status {
    Working,
    Thinking,
    #[default]
    Idle,
    Blocked,
    Done,
}

pub(crate) const STATUSES: [Status; 5] = [
    Status::Working,
    Status::Thinking,
    Status::Idle,
    Status::Blocked,
    Status::Done,
];

impl Status {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::Thinking => "thinking",
            Status::Idle => "idle",
            Status::Blocked => "blocked",
            Status::Done => "done",
        }
    }

    fn index(self) -> usize {
        STATUSES.iter().position(|text| *text == self).unwrap_or(2)
    }
}

/// Float speeds the picker cycles through.
pub(crate) const SPEEDS: [f32; 5] = [1.0, 0.5, 0.25, 2.0, 3.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Avatar {
    pub cat: Cat,
    pub idx: usize,
    /// Regi: index into `regi::PATTERNS` (None: the avatar's own).
    pub eyes: Option<usize>,
    pub speed: f32,
}

impl Default for Avatar {
    fn default() -> Avatar {
        Avatar {
            cat: Cat::Regi,
            idx: 0,
            eyes: None,
            speed: 1.0,
        }
    }
}

impl Avatar {
    /// `regi:12 eyes=3 speed=0.5`. None: not an avatar id (an old sprite id, `custom`).
    pub(crate) fn parse(id: &str) -> Option<Avatar> {
        let mut words = id.split_whitespace();
        let (cat, idx) = words.next()?.split_once(':')?;
        let cat = Cat::parse(cat)?;
        let idx: usize = idx.parse().ok()?;

        if idx >= cat.count() {
            return None;
        }

        let mut agent = Avatar {
            cat,
            idx,
            eyes: None,
            speed: 1.0,
        };

        parse_avatar_options(words, &mut agent);

        Some(agent)
    }

    pub(crate) fn id(&self) -> String {
        let mut text = format!("{}:{}", self.cat.name(), self.idx);

        if let Some(error) = self.eyes {
            let _ = write!(text, " eyes={error}");
        }

        if self.speed.to_bits() != 1.0_f32.to_bits() {
            let _ = write!(text, " speed={}", self.speed);
        }

        text
    }

    /// Name for the picker: the shape, the eyes.
    pub(crate) fn name(&self) -> String {
        match self.cat {
            Cat::Regi => regi::all()[self.idx].name(self.look()),
            Cat::Geo | Cat::GeoB => geo_at(self.idx).1,
        }
    }

    fn look(&self) -> regi::Look {
        regi::Look {
            pattern: self.eyes.map(|key| regi::PATTERNS[key]),
            speed: self.speed,
        }
    }

    /// The 4 rows (7 cells each) at time T (s) for status S.
    pub(crate) fn rows(&self, text: Status, thread: f32) -> [String; HEIGHT] {
        rows(&self.cells(text, thread))
    }

    /// The 4 rows at time T for status S and the tone of each cell (0..1 of the color, row by row;
    /// regi: by line coverage, see `regi::render_toned`; geo: 1).
    pub(crate) fn toned(&self, text: Status, thread: f32) -> ([String; HEIGHT], Vec<f32>) {
        match self.cat {
            Cat::Regi => {
                let (cells, tones) = regi::render_toned(&regi::all()[self.idx], text.index(), thread, self.look());

                (rows(&cells), tones)
            }
            _ => (self.rows(text, thread), vec![1.0; WIDTH * HEIGHT]),
        }
    }

    pub(crate) fn cells(&self, text: Status, thread: f32) -> Vec<glyph::Cell> {
        match self.cat {
            Cat::Regi => regi::render(&regi::all()[self.idx], text.index(), thread, self.look()),
            Cat::Geo => {
                let (value, output) = geo_view(
                    geo_at(self.idx).0,
                    text,
                    thread * self.speed,
                    self.idx as f32 * 1.7,
                    (thread * 30.0) as usize,
                );

                glyph::render(&value, WIDTH, HEIGHT, output)
            }
            Cat::GeoB => {
                let (value_value, output_value) = geo_view(
                    geo_at(self.idx).0,
                    text,
                    thread * self.speed,
                    self.idx as f32 * 1.7,
                    (thread * 30.0) as usize,
                );

                glyph::outline(&value_value, WIDTH, HEIGHT, output_value)
            }
        }
    }
}

fn parse_avatar_options(words: std::str::SplitWhitespace<'_>, agent: &mut Avatar) {
    for width in words {
        match width.split_once('=') {
            Some(("eyes", value)) => agent.eyes = value.parse().ok().filter(|&key| key < regi::PATTERNS.len()),
            Some(("speed", value_value)) => {
                parse_avatar_speed(agent, value_value);
            }
            _ => {}
        }
    }
}

fn parse_avatar_speed(agent: &mut Avatar, value_value: &str) {
    agent.speed = value_value
        .parse()
        .ok()
        .filter(|text: &f32| text.is_finite() && *text > 0.0)
        .unwrap_or(1.0);
}

fn rows(cells: &[glyph::Cell]) -> [String; HEIGHT] {
    std::array::from_fn(|result| {
        cells[result * WIDTH..(result + 1) * WIDTH]
            .iter()
            .map(|character| character.ch)
            .collect()
    })
}

/// Seconds since the first call: the clock of every avatar.
pub(crate) fn now() -> f32 {
    static T0: OnceLock<Instant> = OnceLock::new();

    #[cfg(test)]
    if let Some(thread) = FROZEN.with(std::cell::Cell::get) {
        return thread;
    }

    T0.get_or_init(Instant::now).elapsed().as_secs_f32()
}

#[cfg(test)]
thread_local! {
    /// Clock of [`now`] in tests (None: the real clock): a slow CI runner never moves an animation on.
    static FROZEN: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// Freeze [`now`] of this thread at T (None: the real clock again).
#[cfg(test)]
pub(crate) fn freeze(thread: Option<f32>) {
    FROZEN.with(|character| character.set(thread));
}

fn geo_at(i: usize) -> (sdf::Shape, String) {
    let count = sdf::AVATARS.len();

    if i < count {
        (sdf::AVATARS[i], sdf::AVATARS[i].name().to_string())
    } else {
        (sdf::Shape::Geo((i - count) as u16), geo::all()[i - count].name.clone())
    }
}

/// A geo avatar at time T in braille: the view and render options of status S. The 6 first
/// solids turn by status; geo shapes play their own motion, slowed or stopped by status.
fn geo_view(shape: sdf::Shape, text: Status, thread: f32, phase: f32, frame: usize) -> (sdf::View, glyph::Opts) {
    use sdf::Shape;
    let mut view = sdf::View::new(shape);
    let mut output = glyph::Opts::new();

    output.floor = 0.3;
    view.zoom = 1.15;

    let tt = match text {
        Status::Working => thread,
        Status::Thinking => thread * 0.6,
        Status::Idle => thread * 0.25,
        Status::Blocked => 1.0 + phase,
        Status::Done => phase,
    };

    view.time = tt;

    if let Shape::Geo(i) = shape {
        (view.yaw, view.pitch, view.roll) = geo::all()[i as usize].pose(tt);
    } else {
        view.pitch = if matches!(shape, Shape::Monolith | Shape::Pillar | Shape::Pyramid) {
            0.15
        } else {
            0.45
        };

        view.yaw = match text {
            Status::Working => thread * 1.8 + phase,
            Status::Thinking => 0.5 * (thread * 0.7 + phase).sin(),
            Status::Idle => 0.35 * (thread * 0.5 + phase).sin(),
            _ => 0.4,
        };
    }

    match text {
        Status::Thinking => output.jitter = frame / 3,
        Status::Blocked if (thread * 0.8 + phase).fract() < 0.25 => view.yaw += (thread * 45.0).sin() * 0.12,
        Status::Done => output.gain = 0.55,
        _ => {}
    }

    (view, output)
}

/// A spawn under way: animation ANIM of avatar A, started at START (`now()`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spawn {
    pub anim: usize,
    pub start: f32,
}

/// Seconds of the text decode after a spawn.
pub(crate) const REVEAL: f32 = 0.35;

pub(crate) const SPAWN_W: usize = spawn::CW;
pub(crate) const SPAWN_H: usize = spawn::CH;
/// Cell of the 7x4 sprite in the spawn canvas.
pub(crate) const SPAWN_AT: (usize, usize) = (3, 1);

impl Spawn {
    /// A random animation none of TAKEN (the ones playing), starting now.
    pub(crate) fn random_except(taken: &[usize]) -> Spawn {
        let count_value = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |directory| u64::from(directory.subsec_nanos()));
        let count = spawn::count();
        let mut anim = (regi::hash(count_value) * count as f32) as usize % count;

        // the next free one (all taken: any)
        find_unused_animation(taken, count, &mut anim);

        // tests read the screens: a spawn over them would hide the cards
        let start = if cfg!(test) { now() - 1000.0 } else { now() };

        Spawn { anim, start }
    }

    /// The flash after the snap included.
    pub(crate) fn done(&self) -> bool {
        now() - self.start > self.end()
    }

    /// Seconds from START to the end of the flash.
    fn end(&self) -> f32 {
        spawn::timing(self.anim).2 + 0.15
    }

    /// Progress 0..1 of the text decode after the spawn (0: spawn under way, 1: text shown).
    pub(crate) fn reveal(&self) -> f32 {
        ((now() - self.start - self.end()) / REVEAL).clamp(0.0, 1.0)
    }

    /// Spawn and text decode over.
    pub(crate) fn over(&self) -> bool {
        self.reveal() >= 1.0
    }

    /// The shape the goo forms: the frame of status S at the snap (the 1st frame the status
    /// animation shows after the spawn: 0 jump at the snap).
    fn target(&self, agent: &Avatar, text: Status) -> Vec<glyph::Cell> {
        agent.cells(text, self.start + spawn::timing(self.anim).2)
    }

    /// The 13x6 canvas rows of A (the sprite at `SPAWN_AT`), status S after the snap. None: over.
    pub(crate) fn rows(&self, agent: &Avatar, text: Status) -> Option<[String; SPAWN_H]> {
        if self.done() {
            return None;
        }

        let live = |_: f32| agent.cells(text, now());
        let cells = spawn::render(self.anim, &self.target(agent, text), &live, now() - self.start);

        Some(std::array::from_fn(|result| {
            cells[result * SPAWN_W..(result + 1) * SPAWN_W]
                .iter()
                .map(|character| character.ch)
                .collect()
        }))
    }
}

fn find_unused_animation(taken: &[usize], count: usize, anim: &mut usize) {
    for _ in 0..count {
        if let ControlFlow::Break(()) = skip_taken_animation(taken, *anim) {
            break;
        }

        *anim = (*anim + 1) % count;
    }
}

fn skip_taken_animation(taken: &[usize], anim: usize) -> ControlFlow<()> {
    if !taken.contains(&anim) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

#[cfg(test)]
#[path = "avatar_tests.rs"]
mod tests;
