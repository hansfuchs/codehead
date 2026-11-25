//! 50 spawn animations: an agent's 7x4 sprite forms out of braille goo when a thread or codehead
//! opens. 10 families x 5 variants. Each dot of the sprite's first frame is a blob of goo
//! (box lines become dots too): blobs near each other melt into 1 mass (a metaball field), they
//! stick at the start and lag behind (trails), overshoot their place and wobble to rest, then
//! the blob shrinks to 1 dot and the sprite snaps in, alive.
//!
//! The canvas: 13x6 cells (26x24 dots), the sprite at cell (3, 1).

use super::glyph::{BG, CORAL, Cell, INK, mix};
use super::regi::hash;

pub(crate) const CW: usize = 13;
pub(crate) const CH: usize = 6;
const DOT_WIDTH: usize = CW * 2;
const DOT_HEIGHT: usize = CH * 4;
const OX: usize = 3;
const OY: usize = 1;
/// The loop: the formation, the snap, the hold, a blank gap.
pub(crate) const PERIOD: f32 = 4.2;
/// Goo: flights this much slower, the wobble after the arrival this long (s).
const GOO: f32 = 1.35;
const SETTLE: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Family {
    Fountain,
    Splash,
    Rain,
    Vortex,
    Rise,
    Ripple,
    Wave,
    Condense,
    Drip,
    Merge,
}

pub(crate) const FAMILIES: [Family; 10] = [
    Family::Fountain,
    Family::Splash,
    Family::Rain,
    Family::Vortex,
    Family::Rise,
    Family::Ripple,
    Family::Wave,
    Family::Condense,
    Family::Drip,
    Family::Merge,
];

impl Family {}

pub(crate) fn count() -> usize {
    FAMILIES.len() * 5
}

/// Dots of a cell as bits (2x4, the braille layout): braille as is, box lines as dots on the
/// line, inward (COL: the cell's column in the sprite).
fn mask(ch: char, col: usize) -> [[bool; 2]; 4] {
    let mut dots = [[false; 2]; 4];
    let code = ch as u32;

    if (0x2800..=0x28FF).contains(&code) {
        const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

        for (y, row) in BIT.iter().enumerate() {
            mask_braille_row(&mut dots, code, y, *row);
        }

        return dots;
    }

    // up, down, left, right
    let (up, down, left, right) = match ch {
        '─' | '┄' | '┈' | '╌' => (false, false, true, true),
        '│' | '┆' | '┊' | '╎' => (true, true, false, false),
        '╭' | '┌' => (false, true, false, true),
        '╮' | '┐' => (false, true, true, false),
        '╰' | '└' => (true, false, false, true),
        '╯' | '┘' => (true, false, true, false),
        '┴' => (true, false, true, true),
        '┬' => (false, true, true, true),
        '┤' => (true, true, true, false),
        '├' => (true, true, false, true),
        _ => return dots,
    };

    let cx = usize::from(col == 0);

    dots[1][cx] = true;

    if up {
        dots[0][cx] = true;
    }

    if down {
        dots[2][cx] = true;
        dots[3][cx] = true;
    }

    if left || right {
        dots[1][0] = true;
        dots[1][1] = true;
    }

    dots
}

fn mask_braille_row(dots: &mut [[bool; 2]; 4], code: u32, y: usize, row: [u32; 2]) {
    for (x, bit) in row.iter().enumerate() {
        dots[y][x] = (code - 0x2800) & bit != 0;
    }
}

struct Particle {
    /// Target, dots.
    goal: (f32, f32),
    /// 0..1 per particle.
    random: (f32, f32, f32),
    delay: f32,
}

const CENTER: (f32, f32) = ((OX * 2 + 7) as f32, (OY * 4 + 8) as f32);

#[path = "spawn_paths.rs"]
mod spawn_paths;
pub(crate) use spawn_paths::*;

#[path = "spawn_render.rs"]
mod spawn_render;
pub(crate) use spawn_render::*;

#[cfg(test)]
impl Family {
    fn names(self) -> [&'static str; 5] {
        match self {
            Family::Fountain => FOUNTAIN_NAMES,
            Family::Splash => SPLASH_NAMES,
            Family::Rain => RAIN_NAMES,
            Family::Vortex => VORTEX_NAMES,
            Family::Rise => RISE_NAMES,
            Family::Ripple => RIPPLE_NAMES,
            Family::Wave => WAVE_NAMES,
            Family::Condense => CONDENSE_NAMES,
            Family::Drip => DRIP_NAMES,
            Family::Merge => MERGE_NAMES,
        }
    }
}
#[cfg(test)]
pub(crate) fn name(i: usize) -> &'static str {
    FAMILIES[i / 5].names()[i % 5]
}

#[cfg(test)]
const FOUNTAIN_NAMES: [&str; 5] = [
    "fountain",
    "fountain · twin jets",
    "fountain · high arc",
    "fountain · low jet",
    "fountain · from the left",
];
#[cfg(test)]
const SPLASH_NAMES: [&str; 5] = [
    "splash",
    "splash · loose spring",
    "splash · hard snap",
    "splash · 2 drops",
    "splash · up from below",
];
#[cfg(test)]
const RAIN_NAMES: [&str; 5] = [
    "rain",
    "rain · wind",
    "rain · heavy",
    "rain · drizzle",
    "rain · bottom up",
];
#[cfg(test)]
const VORTEX_NAMES: [&str; 5] = [
    "vortex",
    "vortex · tight",
    "vortex · reverse",
    "vortex · outward",
    "vortex · drain",
];
#[cfg(test)]
const RISE_NAMES: [&str; 5] = [
    "rise · from a puddle",
    "rise · elastic",
    "rise · middle out",
    "rise · bottom first",
    "rise · 2 puddles",
];
#[cfg(test)]
const RIPPLE_NAMES: [&str; 5] = [
    "ripple",
    "ripple · 2 rings",
    "ripple · 3 rings",
    "ripple · fast",
    "ripple · from below",
];
#[cfg(test)]
const WAVE_NAMES: [&str; 5] = [
    "wave · from the left",
    "wave · from the right",
    "wave · both sides",
    "wave · from below",
    "wave · tall",
];
#[cfg(test)]
const CONDENSE_NAMES: [&str; 5] = [
    "condense",
    "condense · steam",
    "condense · ring cloud",
    "condense · sparse",
    "condense · hard snap",
];
#[cfg(test)]
const DRIP_NAMES: [&str; 5] = [
    "drip",
    "drip · from the left",
    "drip · 2 taps",
    "drip · fill up",
    "drip · splashing",
];
#[cfg(test)]
const MERGE_NAMES: [&str; 5] = [
    "merge · 2 drops",
    "merge · 3 from above",
    "merge · 4 corners",
    "merge · 1 big drop",
    "merge · bubbles",
];
