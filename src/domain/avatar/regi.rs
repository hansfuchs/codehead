//! Successors of the regi sprites (sprites/regi.sprites): the 3 eye patterns of the regis, 7 dots
//! each, in 2 kinds of avatar:
//! - 3d: a 1-dot braille wireframe solid that floats, hovers or nods, the 7 dots on its front
//!
//! The dots talk as the regis do: they blink in patterns and rhythms, 1 rhythm per avatar.
//! - working: the talk (sweep, chatter, center out, chase, morse, rows, halves, stutter)
//! - thinking: 1 dot runs around the ring, the center on
//! - idle: all on, a blink now and then (single, double, or row by row)
//! - blocked: amber, the halves flash in turn
//! - done: only the center dot, dim

use super::glyph::{BG, CORAL, Cell, INK, MUTED, WARN, mix};

/// Eye patterns in a -1..1 square (x right, y down, rows 0.5 apart = 1 dot row): the 3 regis,
/// then 8 patterns of the regi sprites (sprites/regi.sprites, by number).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pattern {
    /// Regirock: 2 columns of 3, 1 dot between (an H).
    Rock,
    /// Regice: a row of 5, 1 dot above and below the center (a cross).
    Ice,
    /// Registeel: 2 over 3 over 2 (a hexagon with its center).
    Steel,
    /// regi-008: a cross of 4 rows.
    Cross4,
    /// regi-014: a T.
    Tee,
    /// regi-016: a V.
    Vee,
    /// regi-017: an A.
    Ay,
    /// regi-020: a bar of 5.
    Bar,
    /// regi-022: 3 columns of 4 rows (the middle 2).
    Columns,
    /// regi-034: twin pillars.
    Pillars,
    /// regi-076: a center column on a base.
    Center,
    /// An inverted cross: the bar low on a long column.
    InvCross,
}

pub(crate) const PATTERNS: [Pattern; 12] = [
    Pattern::Rock,
    Pattern::Ice,
    Pattern::Steel,
    Pattern::Cross4,
    Pattern::Tee,
    Pattern::Vee,
    Pattern::Ay,
    Pattern::Bar,
    Pattern::Columns,
    Pattern::Pillars,
    Pattern::Center,
    Pattern::InvCross,
];

/// The talk of the working status: 1 blink rhythm per avatar (tempo, how often, how short; see
/// `blinks`). The names stay from the older animations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Talk {
    Sweep,
    Chatter,
    CenterOut,
    Chase,
    Morse,
    Rows,
    Halves,
    Stutter,
}

pub(crate) const TALKS: [Talk; 8] = [
    Talk::Sweep,
    Talk::Chatter,
    Talk::CenterOut,
    Talk::Chase,
    Talk::Morse,
    Talk::Rows,
    Talk::Halves,
    Talk::Stutter,
];

const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

// ─── 3d ──────────────────────────────────────────────────────────────────────

/// Wireframe solids, in dot units (the 7x4 cells: 14x16 dots). The front face looks at +z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Solid {
    Cube,
    Slab,
    Tall,
    Monolith,
    Obelisk,
    Stele,
    Trapezoid,
    Screen,
    Wedge,
    Arch,
    Capsule,
    Shield,
    Lozenge,
    Diamond,
    Pentagon,
    Hex,
    Heptagon,
    Oct,
    Disk,
    // the new angular ones
    TrapPrism,
    TrapPrismInv,
    Skew,
    Notch,
    HexFrustum,
    Pyramid,
    CutBox,
    Twist,
    Rhomb,
    Crystal,
    Kite,
    Gem,
    // the 3rd round
    Star4,
    Ellipse,
    WideTrapezoid,
}

pub(crate) const SOLIDS: [Solid; 19] = [
    Solid::Cube,
    Solid::Slab,
    Solid::Tall,
    Solid::Monolith,
    Solid::Obelisk,
    Solid::Stele,
    Solid::Trapezoid,
    Solid::Screen,
    Solid::Wedge,
    Solid::Arch,
    Solid::Capsule,
    Solid::Shield,
    Solid::Lozenge,
    Solid::Diamond,
    Solid::Pentagon,
    Solid::Hex,
    Solid::Heptagon,
    Solid::Oct,
    Solid::Disk,
];

/// The 3rd round: 1 avatar each, after the kept ones.
pub(crate) const MORE_SOLIDS: [Solid; 3] = [Solid::Star4, Solid::Ellipse, Solid::WideTrapezoid];

pub(crate) const NEW_SOLIDS: [Solid; 12] = [
    Solid::TrapPrism,
    Solid::TrapPrismInv,
    Solid::Skew,
    Solid::Notch,
    Solid::HexFrustum,
    Solid::Pyramid,
    Solid::CutBox,
    Solid::Twist,
    Solid::Rhomb,
    Solid::Crystal,
    Solid::Kite,
    Solid::Gem,
];

/// The motions: 0 turns, only hovering, floating, nodding, by degree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Motion {
    /// Tips forward and back.
    Nod,
    /// Up and down, 1 dot, calm.
    Float,
    /// Up and down, 1.5 dots, slow, a slight tilt.
    Hover,
    /// Up and down, a slow roll.
    Drift,
    /// Up and down, quick and small.
    Bob,
}

pub(crate) const MOTIONS: [Motion; 5] = [Motion::Nod, Motion::Float, Motion::Hover, Motion::Drift, Motion::Bob];

pub(crate) type P3 = (f32, f32, f32);

/// A wireframe: vertices, faces (vertex loops, outward; face 0: the front).
pub(crate) struct Mesh {
    pub verts: Vec<P3>,
    pub faces: Vec<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Solid(Solid, Motion),
}

pub(crate) struct Regi {
    pub kind: Kind,
    pub pattern: Pattern,
    pub talk: Talk,
    pub blink: usize,
    pub phase: f32,
    /// 3d: the turn it holds (rad).
    pub yaw: f32,
    /// The owner's pick (074 of the 1st set).
    pub star: bool,
}

impl Regi {
    pub(crate) fn name(&self, look: Look) -> String {
        let body = match self.kind {
            Kind::Solid(solid, motion) => format!("3d · {} · {:?}", solid.name(), motion).to_lowercase(),
        };

        let pat = look.pattern.unwrap_or(self.pattern);

        format!(
            "{}{body} · {} · talk: {}",
            if self.star { "★ " } else { "" },
            pat.name(),
            self.talk.name()
        )
    }
}

/// How the regi page draws them all: an eye pattern for all (None: each its own), the speed of
/// the float.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    pub pattern: Option<Pattern>,
    pub speed: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            pattern: None,
            speed: 1.0,
        }
    }
}

const YAWS: [f32; 4] = [0.35, -0.35, 0.15, -0.5];

/// 7x4 cells of R at time T for status S (0 working, 1 thinking, 2 idle, 3 blocked, 4 done).
pub(crate) fn render(regi: &Regi, status: usize, time: f32, look: Look) -> Vec<Cell> {
    render_toned(regi, status, time, look).0
}

/// [`render`] and the tone of each cell (0..1 of its color; see [`tones`]): views that paint the
/// avatar in 1 color of their own (the agent color) dim each cell by it.
pub(crate) fn render_toned(regi: &Regi, status: usize, time: f32, look: Look) -> (Vec<Cell>, Vec<f32>) {
    let pat = look.pattern.unwrap_or(regi.pattern);

    render_with(regi, pat, status, time, look.speed, |dot| {
        lit(pat, regi.talk, regi.blink, status, time + regi.phase, dot)
    })
}

fn render_with(
    regi: &Regi,
    pat: Pattern,
    status: usize,
    time: f32,
    speed: f32,
    on: impl Fn(usize) -> bool,
) -> (Vec<Cell>, Vec<f32>) {
    let (eye_c, frame_c) = colors(status);
    let (dots, cov) = dots_cov(regi, pat, status, time, speed, on);
    let mut out = cells(&dots, eye_c, frame_c);
    let tones = tones(&dots, &cov);

    for (cell, &tone) in out.iter_mut().zip(&tones) {
        if cell.fg == frame_c {
            cell.fg = super::glyph::mix(super::glyph::BG, frame_c, tone);
        }
    }

    (out, tones)
}

/// The 14x16 dots of R (0 off, 1 frame, 2 eye). The eyes: 1 rigid pattern, whatever the pose
/// (only its anchor moves), a blink only takes dots out of it.
#[cfg(test)]
fn dots_with(regi: &Regi, pat: Pattern, status: usize, time: f32, speed: f32, on: impl Fn(usize) -> bool) -> Vec<u8> {
    dots_cov(regi, pat, status, time, speed, on).0
}

/// [`dots_with`] and the line coverage of each dot (0..1, see [`draw3d_cov`]).
fn dots_cov(
    regi: &Regi,
    pat: Pattern,
    status: usize,
    time: f32,
    speed: f32,
    on: impl Fn(usize) -> bool,
) -> (Vec<u8>, Vec<f32>) {
    match regi.kind {
        Kind::Solid(solid, motion) => {
            let local = time + regi.phase;
            let (pitch, roll, bob) = pose(motion, status, local * speed);

            // blocked: a shake now and then
            let yaw = if status == 3 && (local * 0.8).fract() < 0.2 {
                regi.yaw + (local * 50.0).sin() * 0.1
            } else {
                regi.yaw
            };

            let (mesh, eye, ey) = solid.mesh();

            // offsets in whole dots from the anchor (the pattern's center on the front face)
            let marks: Vec<(i32, i32, u8)> = pat
                .dots()
                .iter()
                .enumerate()
                .filter(|(dot, _)| on(*dot))
                .map(|(_, &(x, y))| ((x * eye.0).round() as i32, (y * eye.1).round() as i32, 2))
                .collect();
            let fit = fit(&mesh, motion, regi.yaw);

            draw3d_cov(&mesh, &[], (0.0, ey), &marks, (yaw, pitch, roll), bob, fit)
        }
    }
}

/// The tone of each of the 7x4 cells: the mean coverage COV of its frame dots in DOTS, in 3 tones
/// (0.53, 0.77, 1: a share of the color, the rest the background); 1 for a cell without frame
/// dots. Pixel art's sub-pixel motion: a line that moves by less than 1 dot shows in the color
/// first, the dots follow.
fn tones(dots: &[u8], cov: &[f32]) -> Vec<f32> {
    let mut out = vec![1.0; 28];

    for index in 0..28 {
        let (cx, cy) = (index % 7, index / 7);
        let (mut sum, mut count) = (0.0, 0);

        accumulate_cell_light(dots, cov, cy, cx, &mut sum, &mut count);

        average_cell_light(&mut out, cy, cx, sum, count);
    }

    out
}

fn accumulate_cell_light(dots: &[u8], cov: &[f32], cy: usize, cx: usize, sum: &mut f32, count: &mut i32) {
    for dy in 0..4 {
        for dx in 0..2 {
            let i = (cy * 4 + dy) * DW + cx * 2 + dx;

            accumulate_lit_dot(dots, cov, sum, count, i);
        }
    }
}

fn accumulate_lit_dot(dots: &[u8], cov: &[f32], sum: &mut f32, count: &mut i32, i: usize) {
    if dots[i] == 1 {
        *sum += cov[i];
        *count += 1;
    }
}

fn average_cell_light(out: &mut [f32], cy: usize, cx: usize, sum: f32, count: i32) {
    if count > 0 {
        // the mean coverage, 0.8 and up full; 3 steps: 1/3, 2/3, 1
        let mean = (sum / count as f32 / 0.8).min(1.0);
        let step = (mean * 3.0).round().clamp(1.0, 3.0) / 3.0;

        out[cy * 7 + cx] = 0.3 + 0.7 * step;
    }
}

pub(crate) const DW: usize = 14;
pub(crate) const DH: usize = 16;

/// 14x16 dots (0 off, 1 frame, 2 eye) → 7x4 cells, every dot drawn (0
/// gaps in the shape). 1 color per cell: the eye color only for eye dots alone; a cell of eye and
/// frame dots takes the frame color (a frame dot in the eye color: a fake eye).
pub(crate) fn cells(dots: &[u8], eye_c: super::glyph::Rgb, frame_c: super::glyph::Rgb) -> Vec<Cell> {
    let mut out = Vec::with_capacity(28);

    append_braille_cells(dots, eye_c, frame_c, &mut out);

    out
}

fn append_braille_cells(dots: &[u8], eye_c: (u8, u8, u8), frame_c: (u8, u8, u8), out: &mut Vec<Cell>) {
    for index in 0..28 {
        let (cx, cy) = (index % 7, index / 7);
        let (mut frame, mut eyes) = (0, 0);

        accumulate_cell_bits(dots, cy, cx, &mut frame, &mut eyes);

        let eye = eyes != 0 && frame == 0;

        out.push(Cell {
            ch: braille(frame | eyes),
            fg: if eye { eye_c } else { frame_c },
            bg: None,
        });
    }
}

fn accumulate_cell_bits(dots: &[u8], cy: usize, cx: usize, frame: &mut u32, eyes: &mut u32) {
    for dy in 0..4 {
        accumulate_row_bits(dots, cy, cx, frame, eyes, dy);
    }
}

fn accumulate_row_bits(dots: &[u8], cy: usize, cx: usize, frame: &mut u32, eyes: &mut u32, dy: usize) {
    for dx in 0..2 {
        match dots[(cy * 4 + dy) * DW + cx * 2 + dx] {
            1 => *frame |= BIT[dy][dx],
            2 => *eyes |= BIT[dy][dx],
            _ => {}
        }
    }
}

/// Where a solid sits on the 14x16 dots: its scale, the canvas point of its center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fit {
    pub scale: f32,
    pub cx: f32,
    pub cy: f32,
}

#[cfg(test)]
#[path = "regi_tests.rs"]
mod tests;

#[path = "eyes.rs"]
mod eyes;
pub(crate) use eyes::*;

#[path = "mesh.rs"]
mod mesh;

#[path = "catalog.rs"]
mod catalog;
pub(crate) use catalog::*;

#[path = "raster.rs"]
mod raster;
pub(crate) use raster::*;
