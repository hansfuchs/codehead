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
pub enum Pattern {
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

pub const PATTERNS: [Pattern; 12] =
    [Pattern::Rock, Pattern::Ice, Pattern::Steel, Pattern::Cross4, Pattern::Tee, Pattern::Vee, Pattern::Ay, Pattern::Bar, Pattern::Columns, Pattern::Pillars, Pattern::Center, Pattern::InvCross];

impl Pattern {
    pub fn dots(self) -> &'static [(f32, f32)] {
        match self {
            // as tight as the rows: a 3x3 square, not a wide H
            Pattern::Rock => &[(-0.5, -1.0), (0.5, -1.0), (-0.5, 0.0), (0.0, 0.0), (0.5, 0.0), (-0.5, 1.0), (0.5, 1.0)],
            Pattern::Ice => &[(0.0, -1.0), (-1.0, 0.0), (-0.5, 0.0), (0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (0.0, 1.0)],
            Pattern::Steel => &[(-0.5, -1.0), (0.5, -1.0), (-1.0, 0.0), (0.0, 0.0), (1.0, 0.0), (-0.5, 1.0), (0.5, 1.0)],
            Pattern::Cross4 => &[(0.0, -1.5), (-1.0, -0.5), (-0.5, -0.5), (0.0, -0.5), (0.5, -0.5), (1.0, -0.5), (0.0, 0.5), (0.0, 1.5)],
            Pattern::Tee => &[(-1.0, -1.0), (-0.5, -1.0), (0.0, -1.0), (0.5, -1.0), (1.0, -1.0), (0.0, 0.0), (0.0, 1.0)],
            Pattern::Vee => &[(-1.0, -1.0), (1.0, -1.0), (-0.5, 0.0), (0.5, 0.0), (0.0, 1.0)],
            Pattern::Ay => &[(0.0, -1.0), (-0.5, 0.0), (0.5, 0.0), (-1.0, 1.0), (1.0, 1.0)],
            Pattern::Bar => &[(-1.0, 0.0), (-0.5, 0.0), (0.0, 0.0), (0.5, 0.0), (1.0, 0.0)],
            Pattern::Columns => &[(-1.0, -1.5), (1.0, -1.5), (-1.0, -0.5), (0.0, -0.5), (1.0, -0.5), (-1.0, 0.5), (0.0, 0.5), (1.0, 0.5), (-1.0, 1.5), (1.0, 1.5)],
            Pattern::Pillars => &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (1.0, 1.0)],
            Pattern::Center => &[(0.0, -1.5), (0.0, -0.5), (0.0, 0.5), (-1.0, 1.5), (0.0, 1.5), (1.0, 1.5)],
            Pattern::InvCross => &[(0.0, -1.5), (0.0, -0.5), (-1.0, 0.5), (-0.5, 0.5), (0.0, 0.5), (0.5, 0.5), (1.0, 0.5), (0.0, 1.5)],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Pattern::Rock => "rock H",
            Pattern::Ice => "ice cross",
            Pattern::Steel => "steel hexagon",
            Pattern::Cross4 => "cross 4 rows (008)",
            Pattern::Tee => "T (014)",
            Pattern::Vee => "V (016)",
            Pattern::Ay => "A (017)",
            Pattern::Bar => "bar 5 (020)",
            Pattern::Columns => "columns (022)",
            Pattern::Pillars => "twin pillars (034)",
            Pattern::Center => "center column (076)",
            Pattern::InvCross => "inverted cross",
        }
    }
}

/// The talk of the working status: 1 blink rhythm per avatar (tempo, how often, how short; see
/// `blinks`). The names stay from the older animations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Talk {
    Sweep,
    Chatter,
    CenterOut,
    Chase,
    Morse,
    Rows,
    Halves,
    Stutter,
}

pub const TALKS: [Talk; 8] = [Talk::Sweep, Talk::Chatter, Talk::CenterOut, Talk::Chase, Talk::Morse, Talk::Rows, Talk::Halves, Talk::Stutter];

impl Talk {
    pub fn name(self) -> &'static str {
        match self {
            Talk::Sweep => "sweep",
            Talk::Chatter => "chatter",
            Talk::CenterOut => "center out",
            Talk::Chase => "chase",
            Talk::Morse => "morse",
            Talk::Rows => "rows",
            Talk::Halves => "halves",
            Talk::Stutter => "stutter",
        }
    }
}

pub fn hash(n: u64) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Working: dot K of TALK off at time T. Every dot blinks on its own: per slot (its own length
/// and phase) a chance of 1 short blink at a random point, so the pattern stays in view and single
/// dots wink in a pseudo random rhythm. TALK sets the tempo and how often.
pub fn blinks(talk: Talk, k: usize, t: f32) -> bool {
    // (slot seconds, blink chance per slot, blink seconds)
    let (slot, chance, dur) = match talk {
        Talk::Sweep => (0.55, 0.16, 0.11),
        Talk::Chatter => (0.38, 0.14, 0.08),
        Talk::CenterOut => (0.6, 0.15, 0.12),
        Talk::Chase => (0.45, 0.16, 0.09),
        Talk::Morse => (0.5, 0.2, 0.07),
        Talk::Rows => (0.7, 0.16, 0.13),
        Talk::Halves => (0.48, 0.15, 0.1),
        Talk::Stutter => (0.42, 0.22, 0.06),
    };
    blink((talk as u64 + 1) * 7919 + k as u64 * 104_729, slot, chance, dur, t)
}

/// Thinking: dot K of TALK off at time T. As `blinks`, slower: longer slots, longer blinks
/// (a slow, heavy-lidded wink per dot), own phases per talk.
pub fn blinks_slow(talk: Talk, k: usize, t: f32) -> bool {
    blink((talk as u64 + 41) * 7919 + k as u64 * 104_729, 1.5, 0.3, 0.26, t)
}

/// 1 dot of seed SEED off at time T: per slot (SLOT s, ±30 % per dot, its own phase) CHANCE of 1
/// blink of DUR s at a random point of the slot.
fn blink(seed: u64, slot: f32, chance: f32, dur: f32, t: f32) -> bool {
    let len = slot * (0.7 + 0.6 * hash(seed));
    let u = t + hash(seed + 1) * len;
    let j = (u / len).floor() as u64;
    let at = u - j as f32 * len;
    let s = seed.wrapping_mul(31).wrapping_add(j);
    if hash(s * 3 + 2) >= chance {
        return false;
    }
    let start = hash(s * 3 + 3) * (len - dur).max(0.0);
    (start..start + dur).contains(&at)
}

/// Dot K of PAT on at time T, status S (0 working, 1 thinking, 2 idle, 3 blocked, 4 done).
/// BLINK: the idle blink style (0 single, 1 double, 2 row by row).
pub fn lit(pat: Pattern, talk: Talk, blink: usize, s: usize, t: f32, k: usize) -> bool {
    let d = pat.dots();
    let (x, y) = d[k];
    let center = x == 0.0 && y == 0.0;
    match s {
        0 => !blinks(talk, k, t),
        1 => !blinks_slow(talk, k, t),
        2 => {
            let p = 2.6 + (blink as f32) * 0.7;
            let u = t.rem_euclid(p);
            match blink % 3 {
                0 => u > 0.13,
                1 => !(u < 0.1 || (0.22..0.32).contains(&u)),
                _ => !(u < 0.48 && (u / 0.12) as usize == (y + 1.5) as usize),
            }
        }
        3 => center || ((t * 4.0) as usize).is_multiple_of(2) == (x <= 0.0),
        // done: the center (a pattern without one: its middle column, else its 1st dot)
        _ => center || (!d.contains(&(0.0, 0.0)) && if d.iter().any(|p| p.0 == 0.0) { x == 0.0 } else { k == 0 }),
    }
}

/// Eye, frame color of status S.
pub fn colors(s: usize) -> (super::glyph::Rgb, super::glyph::Rgb) {
    match s {
        0 => (CORAL, mix(MUTED, CORAL, 0.5)),
        3 => (WARN, WARN),
        4 => (MUTED, mix(BG, MUTED, 0.7)),
        _ => (INK, MUTED),
    }
}

const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

fn braille(bits: u32) -> char {
    if bits == 0 { ' ' } else { char::from_u32(0x2800 + bits).unwrap_or(' ') }
}

// ─── 3d ──────────────────────────────────────────────────────────────────────

/// Wireframe solids, in dot units (the 7x4 cells: 14x16 dots). The front face looks at +z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Solid {
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

pub const SOLIDS: [Solid; 19] = [
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
pub const MORE_SOLIDS: [Solid; 3] = [Solid::Star4, Solid::Ellipse, Solid::WideTrapezoid];

pub const NEW_SOLIDS: [Solid; 12] = [
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
pub enum Motion {
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

pub const MOTIONS: [Motion; 5] = [Motion::Nod, Motion::Float, Motion::Hover, Motion::Drift, Motion::Bob];

pub type P3 = (f32, f32, f32);

/// A wireframe: vertices, faces (vertex loops, outward; face 0: the front).
pub struct Mesh {
    pub verts: Vec<P3>,
    pub faces: Vec<Vec<usize>>,
}

impl Mesh {
    /// The solid between the outlines FRONT and BACK (same count, ccw seen from the front).
    pub fn loft(front: &[P3], back: &[P3]) -> Mesh {
        let n = front.len();
        let mut verts = front.to_vec();
        verts.extend_from_slice(back);
        let mut faces = vec![(0..n).collect::<Vec<_>>(), (0..n).rev().map(|i| i + n).collect()];
        for i in 0..n {
            let j = (i + 1) % n;
            faces.push(vec![j, i, i + n, j + n]);
        }
        Mesh { verts, faces }
    }

    /// Prism of the outline OUT, depth D; the back outline scaled by BACK, shifted by OFF,
    /// turned by TURN (rad).
    pub fn prism(out: &[(f32, f32)], d: f32, back: f32, off: (f32, f32), turn: f32) -> Mesh {
        let (s, c) = turn.sin_cos();
        let front: Vec<P3> = out.iter().map(|&(x, y)| (x, y, d / 2.0)).collect();
        let rear: Vec<P3> = out.iter().map(|&(x, y)| ((c * x - s * y) * back + off.0, (s * x + c * y) * back + off.1, -d / 2.0)).collect();
        Mesh::loft(&front, &rear)
    }

    /// Z on the plane of the front face at (X, Y).
    pub fn front_z(&self, x: f32, y: f32) -> f32 {
        let f = &self.faces[0];
        let (a, b, c) = (self.verts[f[0]], self.verts[f[1]], self.verts[f[2]]);
        let n = cross(sub(b, a), sub(c, a));
        if n.2.abs() < 1e-4 { a.2 } else { a.2 - (n.0 * (x - a.0) + n.1 * (y - a.1)) / n.2 }
    }
}

pub fn ngon(n: usize, rx: f32, ry: f32, rot: f32) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let a = rot + std::f32::consts::TAU * i as f32 / n as f32;
            (rx * a.cos(), ry * a.sin())
        })
        .collect()
}

/// Half circle of radius R around (0, CY) from angle A0, ccw, N steps.
fn arc(r: f32, cy: f32, a0: f32, n: usize) -> impl Iterator<Item = (f32, f32)> {
    (0..=n).map(move |i| {
        let a = a0 + std::f32::consts::PI * i as f32 / n as f32;
        (r * a.cos(), cy + r * a.sin())
    })
}

pub fn rect(w: f32, h: f32) -> Vec<(f32, f32)> {
    vec![(w, h), (-w, h), (-w, -h), (w, -h)]
}

impl Solid {
    pub fn name(self) -> &'static str {
        match self {
            Solid::Cube => "cube",
            Solid::Slab => "slab",
            Solid::Tall => "tall slab",
            Solid::Monolith => "monolith",
            Solid::Obelisk => "obelisk",
            Solid::Stele => "stele",
            Solid::Trapezoid => "trapezoid",
            Solid::Screen => "screen",
            Solid::Wedge => "wedge",
            Solid::Arch => "arch",
            Solid::Capsule => "capsule",
            Solid::Shield => "shield",
            Solid::Lozenge => "lozenge",
            Solid::Diamond => "diamond",
            Solid::Pentagon => "pentagon",
            Solid::Hex => "hex disk",
            Solid::Heptagon => "heptagon",
            Solid::Oct => "octagon",
            Solid::Disk => "disk",
            Solid::TrapPrism => "trapezoidal prism",
            Solid::TrapPrismInv => "trapezoidal prism, inverted",
            Solid::Skew => "skewed box",
            Solid::Notch => "notched block",
            Solid::HexFrustum => "hex frustum",
            Solid::Pyramid => "pyramid",
            Solid::CutBox => "cut cube",
            Solid::Twist => "twisted box",
            Solid::Rhomb => "rhombic prism",
            Solid::Crystal => "crystal",
            Solid::Kite => "kite crystal",
            Solid::Gem => "gem",
            Solid::Star4 => "4-point star",
            Solid::Ellipse => "ellipse",
            Solid::WideTrapezoid => "wide trapezoid",
        }
    }

    /// The mesh, the eye scale (x, y) in dots, the eye center y.
    pub fn mesh(self) -> (Mesh, (f32, f32), f32) {
        use std::f32::consts::{FRAC_PI_2, PI};
        let p = |out: Vec<(f32, f32)>, d: f32, back: f32| Mesh::prism(&out, d, back, (0.0, 0.0), 0.0);
        let e = (4.0, 2.0);
        match self {
            Solid::Cube => (p(rect(5.5, 5.5), 6.0, 1.0), e, 0.0),
            Solid::Slab => (p(rect(6.0, 4.5), 3.0, 1.0), e, 0.0),
            Solid::Tall => (p(rect(5.0, 6.5), 2.5, 1.0), (4.0, 2.5), 1.0),
            Solid::Monolith => (p(rect(5.0, 6.5), 1.5, 1.0), e, 1.5),
            Solid::Obelisk => (p(vec![(4.6, -6.8), (4.6, 3.2), (0.0, 7.2), (-4.6, 3.2), (-4.6, -6.8)], 3.0, 1.0), (3.5, 2.0), 0.0),
            Solid::Stele => (p(vec![(5.5, -6.5), (5.5, 4.5), (-5.5, 6.5), (-5.5, -6.5)], 2.5, 1.0), e, 0.5),
            Solid::Trapezoid => (p(vec![(6.5, -6.5), (4.2, 6.0), (-4.2, 6.0), (-6.5, -6.5)], 3.0, 1.0), e, 0.0),
            Solid::Screen => (p(rect(6.0, 5.0), 5.0, 0.55), e, 0.0),
            Solid::Wedge => {
                let mut m = p(rect(5.5, 5.5), 6.0, 1.0);
                // the top of the back edge pulled down: a wedge leaning back
                for v in m.verts.iter_mut().skip(4).take(2) {
                    v.1 -= 4.0;
                }
                (m, e, 0.0)
            }
            Solid::Arch => {
                let mut o = vec![(6.0, -6.0)];
                o.extend(arc(6.0, 1.0, 0.0, 8));
                o.push((-6.0, -6.0));
                (p(o, 3.0, 1.0), e, 0.0)
            }
            Solid::Capsule => {
                let mut o: Vec<(f32, f32)> = arc(5.6, 1.4, 0.0, 8).collect();
                o.extend(arc(5.6, -1.4, PI, 8));
                (p(o, 2.5, 1.0), e, 0.0)
            }
            Solid::Shield => (p(vec![(6.0, 5.5), (-6.0, 5.5), (-6.0, -0.5), (0.0, -7.0), (6.0, -0.5)], 2.5, 1.0), e, 1.5),
            Solid::Lozenge => (p(vec![(0.0, 7.5), (-5.6, 3.5), (-5.6, -3.5), (0.0, -7.5), (5.6, -3.5), (5.6, 3.5)], 2.5, 1.0), e, 0.0),
            Solid::Diamond => (p(vec![(7.0, 0.0), (0.0, 7.5), (-7.0, 0.0), (0.0, -7.5)], 3.0, 1.0), (3.5, 2.0), 0.0),
            Solid::Pentagon => (p(ngon(5, 7.2, 7.2, FRAC_PI_2), 2.5, 1.0), e, -0.5),
            Solid::Hex => (p(ngon(6, 6.6, 6.6, 0.0), 3.0, 1.0), e, 0.0),
            Solid::Heptagon => (p(ngon(7, 6.8, 6.8, FRAC_PI_2), 2.5, 1.0), e, 0.0),
            Solid::Oct => (p(ngon(8, 6.4, 6.4, PI / 8.0), 3.0, 1.0), e, 0.0),
            Solid::Disk => (p(ngon(16, 6.4, 6.4, 0.0), 2.5, 1.0), e, 0.0),
            Solid::TrapPrism | Solid::TrapPrismInv => {
                // the front leans: its top (or bottom) set back
                let (zt, zb) = if self == Solid::TrapPrism { (0.0, 2.8) } else { (2.8, 0.0) };
                let front = [(5.5, 5.5, zt), (-5.5, 5.5, zt), (-5.5, -5.5, zb), (5.5, -5.5, zb)];
                let back = [(5.5, 5.5, -2.8), (-5.5, 5.5, -2.8), (-5.5, -5.5, -2.8), (5.5, -5.5, -2.8)];
                (Mesh::loft(&front, &back), e, 0.0)
            }
            Solid::Skew => (Mesh::prism(&rect(5.0, 5.0), 4.0, 1.0, (2.4, 1.8), 0.0), e, 0.0),
            Solid::Notch => (p(vec![(5.5, 5.5), (-5.5, 5.5), (-5.5, -5.5), (1.5, -5.5), (1.5, -2.0), (5.5, -2.0)], 4.0, 1.0), e, 1.5),
            Solid::HexFrustum => (p(ngon(6, 6.8, 6.8, 0.0), 4.0, 0.55), e, 0.0),
            Solid::Pyramid => (p(rect(6.0, 6.0), 6.0, 0.03), e, 0.0),
            Solid::CutBox => {
                let front: Vec<P3> = [(5.5, 1.5), (1.5, 5.5), (-5.5, 5.5), (-5.5, -5.5), (5.5, -5.5)].iter().map(|&(x, y)| (x, y, 2.8)).collect();
                let back: Vec<P3> = [(5.5, 4.8), (4.8, 5.5), (-5.5, 5.5), (-5.5, -5.5), (5.5, -5.5)].iter().map(|&(x, y)| (x, y, -2.8)).collect();
                (Mesh::loft(&front, &back), e, -0.5)
            }
            Solid::Twist => (Mesh::prism(&rect(5.5, 5.5), 5.0, 0.95, (0.0, 0.0), 0.45), e, 0.0),
            Solid::Rhomb => (p(vec![(6.8, 5.0), (-3.2, 5.0), (-6.8, -5.0), (3.2, -5.0)], 3.0, 1.0), e, 0.0),
            Solid::Crystal => {
                let out = vec![(6.2, 1.0), (3.0, 6.8), (-3.6, 5.8), (-6.6, -0.5), (-3.0, -6.8), (3.6, -5.8)];
                (Mesh::prism(&out, 4.0, 0.7, (1.2, -0.8), 0.25), e, 0.0)
            }
            Solid::Kite => (Mesh::prism(&[(0.0, 7.6), (-6.4, 1.5), (0.0, -7.6), (6.4, 1.5)], 4.0, 0.55, (0.0, 0.8), 0.0), (3.5, 2.0), 0.8),
            Solid::Gem => (p(ngon(8, 5.4, 5.4, PI / 8.0), 3.0, 1.3), e, 0.0),
            Solid::Star4 => {
                // 4 points (up, down, left, right), the inner corners on the diagonals; the
                // loop starts on an inner corner: the 2nd vertex convex (the front normal +z)
                let o: Vec<(f32, f32)> = (0..8)
                    .map(|i| {
                        let a = -PI / 4.0 + PI / 4.0 * i as f32;
                        let (rx, ry) = if i % 2 == 1 { (6.8, 7.6) } else { (3.9, 3.9) };
                        (rx * a.cos(), ry * a.sin())
                    })
                    .collect();
                (p(o, 1.8, 1.0), (2.0, 1.5), 0.0)
            }
            Solid::Ellipse => (p(ngon(20, 7.0, 5.2, 0.0), 3.0, 1.0), e, 0.0),
            Solid::WideTrapezoid => (p(vec![(7.0, -4.8), (3.6, 4.8), (-3.6, 4.8), (-7.0, -4.8)], 3.0, 1.0), e, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Solid(Solid, Motion),
}

pub struct Regi {
    pub kind: Kind,
    pub pattern: Pattern,
    pub talk: Talk,
    pub blink: usize,
    pub phase: f32,
    /// 3d: the turn it holds (rad).
    pub yaw: f32,
    /// The owner's pick (074 of the 1st set).
    pub star: bool,
    /// Added in the last round: drawn after a gap.
    pub new: bool,
}

impl Regi {
    pub fn name(&self, look: &Look) -> String {
        let body = match self.kind {
            Kind::Solid(s, m) => format!("3d · {} · {:?}", s.name(), m).to_lowercase(),
        };
        let pat = look.pattern.unwrap_or(self.pattern);
        format!("{}{body} · {} · talk: {}", if self.star { "★ " } else { "" }, pat.name(), self.talk.name())
    }
}

/// How the regi page draws them all: an eye pattern for all (None: each its own), the speed of
/// the float.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub pattern: Option<Pattern>,
    pub speed: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look { pattern: None, speed: 1.0 }
    }
}

const YAWS: [f32; 4] = [0.35, -0.35, 0.15, -0.5];

/// The kept ones of the 1st sets (by their old number), then the new angular solids, then the
/// 3rd round. Out: the flat frames, the obelisks, the arches.
pub fn catalog() -> Vec<Regi> {
    let mut out = Vec::new();
    // k: the phase seed, as before the flat frames went out (the kept ones keep their phase)
    let mut k = 10u64;
    // 3d: 1 per solid and pattern of the regis, the dropped ones out (old 032, 039-044, 051-053)
    const DROP: [usize; 10] = [0, 7, 8, 9, 10, 11, 12, 19, 20, 21];
    let mut n = 0;
    for (si, &solid) in SOLIDS.iter().enumerate() {
        for (pi, &pattern) in PATTERNS[..3].iter().enumerate() {
            if solid == Solid::Disk && pattern == Pattern::Ice {
                continue;
            }
            let idx = si * 3 + pi;
            n += 1;
            if DROP.contains(&(n - 1)) {
                continue;
            }
            k += 1;
            if matches!(solid, Solid::Obelisk | Solid::Arch) {
                continue;
            }
            let kind = Kind::Solid(solid, MOTIONS[(si + pi * 2) % 5]);
            out.push(Regi { kind, pattern, talk: TALKS[(idx * 5 + si) % 8], blink: idx % 3, phase: hash(k - 1) * 10.0, yaw: YAWS[(si + pi) % 4], star: false, new: false });
        }
    }
    // new: 2 per angular solid, the sprite patterns spread over them; kept: old 066, 076-080
    for (si, &solid) in NEW_SOLIDS.iter().enumerate() {
        for j in 0..2 {
            let idx = si * 2 + j;
            if ![9, 19, 20, 21, 22, 23].contains(&idx) {
                continue;
            }
            let kind = Kind::Solid(solid, MOTIONS[(si + j * 3 + 1) % 5]);
            let pattern = PATTERNS[(idx * 7 + 3) % 11];
            out.push(Regi { kind, pattern, talk: TALKS[(idx * 3 + j) % 8], blink: idx % 3, phase: hash(k) * 10.0, yaw: YAWS[(si + j) % 4], star: false, new: false });
            k += 1;
        }
    }
    // 3rd round: a star, an ellipse, a wide trapezoid
    // (eyes, motion, talk, turn)
    let more = [(Pattern::Steel, Motion::Nod, Talk::Chase, 0.3), (Pattern::Steel, Motion::Hover, Talk::Morse, 0.35), (Pattern::Rock, Motion::Nod, Talk::Halves, 0.0)];
    for (si, (&solid, (pattern, motion, talk, yaw))) in MORE_SOLIDS.iter().zip(more).enumerate() {
        out.push(Regi { kind: Kind::Solid(solid, motion), pattern, talk, blink: si % 3, phase: hash(k) * 10.0, yaw, star: false, new: true });
        k += 1;
    }
    out
}

/// The pick: 074 of the 1st set, as it was (the spawn page's agent).
pub fn star() -> Regi {
    Regi { kind: Kind::Solid(Solid::Disk, Motion::Nod), pattern: Pattern::Ice, talk: Talk::Stutter, blink: 1, phase: hash(73) * 10.0, yaw: 0.35, star: true, new: false }
}

pub fn all() -> &'static [Regi] {
    static C: std::sync::OnceLock<Vec<Regi>> = std::sync::OnceLock::new();
    C.get_or_init(catalog)
}

/// 7x4 cells of R at time T for status S (0 working, 1 thinking, 2 idle, 3 blocked, 4 done).
pub fn render(r: &Regi, s: usize, t: f32, look: &Look) -> Vec<Cell> {
    let pat = look.pattern.unwrap_or(r.pattern);
    render_with(r, pat, s, t, look.speed, |k| lit(pat, r.talk, r.blink, s, t + r.phase, k))
}

fn render_with(r: &Regi, pat: Pattern, s: usize, t: f32, speed: f32, on: impl Fn(usize) -> bool) -> Vec<Cell> {
    let (eye_c, frame_c) = colors(s);
    cells(&dots_with(r, pat, s, t, speed, on), eye_c, frame_c)
}

/// The 14x16 dots of R (0 off, 1 frame, 2 eye). The eyes: 1 rigid pattern, whatever the pose
/// (only its anchor moves), a blink only takes dots out of it.
fn dots_with(r: &Regi, pat: Pattern, s: usize, t: f32, speed: f32, on: impl Fn(usize) -> bool) -> Vec<u8> {
    match r.kind {
        Kind::Solid(solid, motion) => {
            let t = t + r.phase;
            let (pitch, roll, bob) = pose(motion, s, t * speed);
            // blocked: a shake now and then
            let yaw = if s == 3 && (t * 0.8).fract() < 0.2 { r.yaw + (t * 50.0).sin() * 0.1 } else { r.yaw };
            let (mesh, eye, ey) = solid.mesh();
            // offsets in whole dots from the anchor (the pattern's center on the front face)
            let marks: Vec<(i32, i32, u8)> = pat.dots().iter().enumerate().filter(|(k, _)| on(*k)).map(|(_, &(x, y))| ((x * eye.0).round() as i32, (y * eye.1).round() as i32, 2)).collect();
            let fit = fit(&mesh, motion, r.yaw);
            draw3d_in(&mesh, &[], (0.0, ey), &marks, (yaw, pitch, roll), bob, fit)
        }
    }
}

pub const DW: usize = 14;
pub const DH: usize = 16;

/// 14x16 dots (0 off, 1 frame, 2 eye) → 7x4 cells, every dot drawn (0
/// gaps in the shape). 1 color per cell: the eye color only for eye dots alone; a cell of eye and
/// frame dots takes the frame color (a frame dot in the eye color: a fake eye).
pub fn cells(dots: &[u8], eye_c: super::glyph::Rgb, frame_c: super::glyph::Rgb) -> Vec<Cell> {
    let mut out = Vec::with_capacity(28);
    for cy in 0..4 {
        for cx in 0..7 {
            let (mut frame, mut eyes) = (0, 0);
            for dy in 0..4 {
                for dx in 0..2 {
                    match dots[(cy * 4 + dy) * DW + cx * 2 + dx] {
                        1 => frame |= BIT[dy][dx],
                        2 => eyes |= BIT[dy][dx],
                        _ => {}
                    }
                }
            }
            let eye = eyes != 0 && frame == 0;
            out.push(Cell { ch: braille(frame | eyes), fg: if eye { eye_c } else { frame_c }, bg: None });
        }
    }
    out
}

fn rot(p: P3, yaw: f32, pitch: f32, roll: f32) -> P3 {
    let (s, c) = yaw.sin_cos();
    let (x, y, z) = (c * p.0 + s * p.2, p.1, -s * p.0 + c * p.2);
    let (s, c) = pitch.sin_cos();
    let (y, z) = (c * y - s * z, s * y + c * z);
    let (s, c) = roll.sin_cos();
    (c * x - s * y, s * x + c * y, z)
}

fn sub(a: P3, b: P3) -> P3 {
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

fn cross(a: P3, b: P3) -> P3 {
    (a.1 * b.2 - a.2 * b.1, a.2 * b.0 - a.0 * b.2, a.0 * b.1 - a.1 * b.0)
}

fn unit(a: P3) -> P3 {
    let l = (a.0 * a.0 + a.1 * a.1 + a.2 * a.2).sqrt().max(1e-6);
    (a.0 / l, a.1 / l, a.2 / l)
}

/// The pose of MOTION at T, slowed or stopped by status S: pitch, roll, bob (dots).
pub fn pose(m: Motion, s: usize, t: f32) -> (f32, f32, f32) {
    let t = match s {
        0 => t,
        1 => t * 0.6,
        2 => t * 0.3,
        _ => 1.3,
    };
    match m {
        Motion::Nod => (0.3 + 0.25 * (t * 1.6).sin(), 0.0, 0.0),
        Motion::Float => (0.25, 0.0, 0.9 * (t * 1.8).sin()),
        Motion::Hover => (0.25 + 0.06 * (t * 0.9 + 1.0).sin(), 0.0, 1.5 * (t * 0.9).sin()),
        Motion::Drift => (0.25, 0.07 * (t * 0.8).sin(), 0.7 * (t * 1.3).sin()),
        Motion::Bob => (0.25, 0.0, 0.45 * (t * 3.6).sin()),
    }
}

/// MESH as a 1-dot wireframe in 14x16 dots: the silhouette and the creases in view. WIRES:
/// extra 3d segments (horns, pins), always drawn. MARKS: points on the front face plane
/// (x, y, value: 1 line, 2 eye), drawn while the front looks at the viewer.
/// MARKS: dot offsets (x right, y down) from ANCHOR (x, y on the front face), projected as 1 rigid
/// group: a pose moves them together, 0 rounding per mark.
pub fn draw3d(mesh: &Mesh, wires: &[(P3, P3)], anchor: (f32, f32), marks: &[(i32, i32, u8)], pose: (f32, f32, f32), bob: f32) -> Vec<u8> {
    draw3d_in(mesh, wires, anchor, marks, pose, bob, Fit::FREE)
}

/// Where a solid sits on the 14x16 dots: its scale, the canvas point of its center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub cx: f32,
    pub cy: f32,
}

impl Fit {
    /// 0 fit: scale 1, the center a little right of and below the middle.
    pub const FREE: Fit = Fit { scale: 1.0, cx: 6.6, cy: 7.6 };
}

/// Perspective: a camera 40 dots away. P relative to the canvas center, y up → (x, y) right, down.
fn persp(p: P3, bob: f32) -> (f32, f32) {
    let k = 40.0 / (40.0 - p.2);
    (p.0 * k, -(p.1 + bob) * k)
}

/// The fit of MESH in MOTION at turn YAW: every vertex over 1 whole motion (pitch, roll, bob,
/// the blocked shake) inside the canvas, the swept area centered; a smaller scale only when that
/// area is larger than the canvas. 0 dots cut off at the canvas edge while it floats.
pub fn fit(mesh: &Mesh, motion: Motion, yaw: f32) -> Fit {
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    let mut take = |yaw: f32, (pitch, roll, bob): (f32, f32, f32)| {
        for &v in &mesh.verts {
            let (x, y) = persp(rot(v, yaw, pitch, roll), bob);
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
    };
    // 32 s in 1/8 s steps: every motion's period (max. 2π / 0.8 ≈ 7.9 s) several times
    for i in 0..256 {
        take(yaw, pose(motion, 0, i as f32 / 8.0));
    }
    // blocked: its fixed pose, the shake ±0.1 rad
    for d in [-0.1, 0.0, 0.1] {
        take(yaw + d, pose(motion, 3, 0.0));
    }
    let (w, h) = ((DW - 1) as f32, (DH - 1) as f32);
    let scale = (w / (x1 - x0).max(1e-3)).min(h / (y1 - y0).max(1e-3)).min(1.0);
    Fit { scale, cx: w / 2.0 - (x0 + x1) / 2.0 * scale, cy: h / 2.0 - (y0 + y1) / 2.0 * scale }
}

/// [`draw3d`] placed by FIT.
pub fn draw3d_in(mesh: &Mesh, wires: &[(P3, P3)], anchor: (f32, f32), marks: &[(i32, i32, u8)], (yaw, pitch, roll): (f32, f32, f32), bob: f32, fit: Fit) -> Vec<u8> {
    let vr: Vec<P3> = mesh.verts.iter().map(|&p| rot(p, yaw, pitch, roll)).collect();
    let proj = |p: P3| -> (f32, f32) {
        let (x, y) = persp(p, bob);
        (fit.cx + x * fit.scale, fit.cy + y * fit.scale)
    };
    let normal = |f: &[usize]| unit(cross(sub(vr[f[1]], vr[f[0]]), sub(vr[f[2]], vr[f[0]])));
    let norms: Vec<P3> = mesh.faces.iter().map(|f| normal(f)).collect();
    let vis: Vec<bool> = norms.iter().map(|n| n.2 > 0.0).collect();
    let mut dots = vec![0u8; DW * DH];
    let mut set = |x: f32, y: f32, v: u8| {
        let (x, y) = (x.round(), y.round());
        if x >= 0.0 && y >= 0.0 && (x as usize) < DW && (y as usize) < DH {
            let i = y as usize * DW + x as usize;
            dots[i] = dots[i].max(v);
        }
    };
    // Bresenham between the rounded ends: 1 dot per step, 0 doubled dots at the steps, and the
    // edges of 1 vertex all end on the same dot (sharp corners)
    let line = |a: (f32, f32), b: (f32, f32), set: &mut dyn FnMut(f32, f32, u8)| {
        let (mut x, mut y) = (a.0.round() as i32, a.1.round() as i32);
        let (x1, y1) = (b.0.round() as i32, b.1.round() as i32);
        let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
        let (sx, sy) = ((x1 - x).signum(), (y1 - y).signum());
        let mut err = dx + dy;
        loop {
            set(x as f32, y as f32, 1);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    };
    let mut edges: Vec<(usize, usize, usize)> = Vec::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for i in 0..f.len() {
            edges.push((f[i].min(f[(i + 1) % f.len()]), f[i].max(f[(i + 1) % f.len()]), fi));
        }
    }
    edges.sort();
    for pair in edges.chunks(2) {
        let [(a, b, fa), (_, _, fb)] = [pair[0], *pair.get(1).unwrap_or(&pair[0])];
        let (na, nb) = (norms[fa], norms[fb]);
        let crease = na.0 * nb.0 + na.1 * nb.1 + na.2 * nb.2 < 0.8;
        if !(vis[fa] || vis[fb]) || (vis[fa] && vis[fb] && !crease) {
            continue;
        }
        line(proj(vr[a]), proj(vr[b]), &mut set);
    }
    for &(a, b) in wires {
        line(proj(rot(a, yaw, pitch, roll)), proj(rot(b, yaw, pitch, roll)), &mut set);
    }
    clean(&mut dots, wires.is_empty());
    if norms[0].2 > 0.2 {
        let (ax, ay) = proj(rot((anchor.0, anchor.1, mesh.front_z(anchor.0, anchor.1)), yaw, pitch, roll));
        let (ax, ay) = (ax.round() as i32, ay.round() as i32);
        for &(x, y, v) in marks {
            let (x, y) = (ax + x, ay + y);
            if x >= 0 && y >= 0 && (x as usize) < DW && (y as usize) < DH {
                let i = y as usize * DW + x as usize;
                dots[i] = dots[i].max(v);
            }
        }
    }
    dots
}

/// The line dots (1) of DOTS, cleaned: lone dots out, then (SPURS) 1-dot spurs off a line
/// (a dot with 1 neighbor that has 4+ neighbors).
pub fn clean(dots: &mut [u8], spurs: bool) {
    clean_in(dots, DW, DH, spurs);
}

/// [`clean`] on a DW x DH grid of dots.
pub fn clean_in(dots: &mut [u8], dw: usize, dh: usize, spurs: bool) {
    let on = |d: &[u8], x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < dw && (y as usize) < dh && d[y as usize * dw + x as usize] == 1;
    let around = |d: &[u8], x: i32, y: i32| -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for (ox, oy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            if on(d, x + ox, y + oy) {
                v.push((x + ox, y + oy));
            }
        }
        v
    };
    for _ in 0..3 {
        let mut changed = false;
        for y in 0..dh as i32 {
            for x in 0..dw as i32 {
                if !on(dots, x, y) {
                    continue;
                }
                let n = around(dots, x, y);
                let lone = n.is_empty();
                let spur = spurs && n.len() == 1 && around(dots, n[0].0, n[0].1).len() >= 4;
                if lone || spur {
                    dots[y as usize * dw + x as usize] = 0;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_solids_stay_inside_the_canvas() {
        // every solid, every motion, a range of turns: each vertex of each frame on a dot of the canvas
        for solid in SOLIDS.iter().chain(&NEW_SOLIDS).chain(&MORE_SOLIDS) {
            let (mesh, _, _) = solid.mesh();
            for motion in MOTIONS {
                for yaw in [-0.5f32, -0.2, 0.0, 0.3, 0.6] {
                    let f = fit(&mesh, motion, yaw);
                    assert!(f.scale > 0.75, "{solid:?} {motion:?}: scale {}", f.scale);
                    for s in 0..4 {
                        for i in 0..400 {
                            let (pitch, roll, bob) = pose(motion, s, i as f32 * 0.05);
                            for &v in &mesh.verts {
                                let (x, y) = persp(rot(v, yaw, pitch, roll), bob);
                                let (x, y) = (f.cx + x * f.scale, f.cy + y * f.scale);
                                assert!((-0.5..DW as f32 - 0.5).contains(&x) && (-0.5..DH as f32 - 0.5).contains(&y), "{solid:?} {motion:?} yaw {yaw} s {s}: ({x}, {y})");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn lines_have_no_loose_ends() {
        // every regi, every status, 20 s at 10 fps: each line dot touches 2+ dots (a closed
        // wireframe: 0 lone dots, 0 spurs off a corner)
        for (i, r) in all().iter().enumerate() {
            for s in 0..5 {
                for f in 0..200 {
                    let t = f as f32 / 10.0;
                    let d = dots_with(r, r.pattern, s, t, 1.0, |_| true);
                    for y in 0..DH as i32 {
                        for x in 0..DW as i32 {
                            if d[y as usize * DW + x as usize] != 1 {
                                continue;
                            }
                            let n = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
                                .iter()
                                .filter(|&&(ox, oy)| {
                                    let (a, b) = (x + ox, y + oy);
                                    a >= 0 && b >= 0 && (a as usize) < DW && (b as usize) < DH && d[b as usize * DW + a as usize] != 0
                                })
                                .count();
                            assert!(n >= 2, "regi {i} s{s} t{t}: a loose dot at ({x}, {y})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn working_eyes_blink_one_by_one() {
        // over 40 s at 60 fps: each dot blinks, mostly 0 or 1 dot off, never half of them
        for talk in TALKS {
            let n = 9;
            let (mut off_any, mut multi) = (vec![false; n], 0);
            for f in 0..2400 {
                let t = f as f32 / 60.0;
                let off = (0..n).filter(|&k| blinks(talk, k, t)).count();
                assert!(off <= n / 2, "{talk:?} {t}: {off} off");
                multi += usize::from(off > 1);
                (0..n).for_each(|k| off_any[k] |= blinks(talk, k, t));
            }
            assert!(off_any.iter().all(|&b| b), "{talk:?}: a dot never blinks");
            assert!(multi < 2400 / 20, "{talk:?}: {multi} frames with 2+ off");
        }
    }

    #[test]
    fn eye_patterns_stay_rigid() {
        // every regi, its own pattern and each override, every status, 10 s at 10 fps: the eyes
        // all on: 1 shape (moved, never changed); a blink: a subset of it, 0 dot elsewhere
        let eyes = |d: &[u8]| -> Vec<(i32, i32)> { (0..DW * DH).filter(|&i| d[i] == 2).map(|i| ((i % DW) as i32, (i / DW) as i32)).collect() };
        let norm = |e: &[(i32, i32)]| -> Vec<(i32, i32)> {
            let (mx, my) = (e.iter().map(|p| p.0).min().unwrap_or(0), e.iter().map(|p| p.1).min().unwrap_or(0));
            let mut v: Vec<(i32, i32)> = e.iter().map(|p| (p.0 - mx, p.1 - my)).collect();
            v.sort();
            v
        };
        for (i, r) in all().iter().enumerate() {
            for pat in std::iter::once(r.pattern).chain(PATTERNS) {
                let n = pat.dots().len();
                let mut shape: Option<Vec<(i32, i32)>> = None;
                for s in 0..5 {
                    for f in 0..100 {
                        let t = f as f32 / 10.0;
                        let full = eyes(&dots_with(r, pat, s, t, 1.0, |_| true));
                        if full.is_empty() {
                            continue; // the face turned away: 0 eyes
                        }
                        assert_eq!(full.len(), n, "regi {i} {pat:?} s{s} t{t}: {} of {n} eyes", full.len());
                        let sh = norm(&full);
                        match &shape {
                            None => shape = Some(sh),
                            Some(a) => assert_eq!(a, &sh, "regi {i} {pat:?} s{s} t{t}: the eye pattern changed"),
                        }
                        let d = dots_with(r, pat, s, t, 1.0, |k| lit(pat, r.talk, r.blink, s, t + r.phase, k));
                        assert!(eyes(&d).iter().all(|p| full.contains(p)), "regi {i} {pat:?} s{s} t{t}: a lit eye off the pattern");
                        // drawn: every dot of the shape (0 gaps); the eye color only on eye dots
                        let (ec, fc) = colors(s);
                        let drawn = cells(&d, ec, fc);
                        for (c, cell) in drawn.iter().enumerate() {
                            let bits = (cell.ch as u32).saturating_sub(0x2800) & 0xFF;
                            let (mut frame, mut eye) = (0, 0);
                            for j in 0..8 {
                                match d[((c / 7) * 4 + j / 2) * DW + (c % 7) * 2 + j % 2] {
                                    1 => frame |= BIT[j / 2][j % 2],
                                    2 => eye |= BIT[j / 2][j % 2],
                                    _ => {}
                                }
                            }
                            assert_eq!(bits, frame | eye, "regi {i} {pat:?} s{s} t{t}: cell {c} a gap");
                            if ec != fc && cell.fg == ec {
                                assert_eq!(frame, 0, "regi {i} {pat:?} s{s} t{t}: cell {c} a frame dot in the eye color");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn thinking_eyes_blink_slower() {
        // over 60 s at 60 fps: each dot blinks, blinks last longer, fewer blinks than working
        let n = 9;
        for talk in TALKS {
            let count = |off: &dyn Fn(usize, f32) -> bool| {
                let (mut starts, mut frames, mut seen) = (0, 0, vec![false; n]);
                for k in 0..n {
                    let mut was = false;
                    for f in 0..3600 {
                        let o = off(k, f as f32 / 60.0);
                        starts += usize::from(o && !was);
                        frames += usize::from(o);
                        seen[k] |= o;
                        was = o;
                    }
                }
                (starts, frames as f32 / starts.max(1) as f32, seen.iter().all(|&b| b))
            };
            let (ws, wlen, _) = count(&|k, t| blinks(talk, k, t));
            let (ts, tlen, all) = count(&|k, t| blinks_slow(talk, k, t));
            assert!(all, "{talk:?}: a dot never blinks");
            assert!(ts < ws && tlen > wlen, "{talk:?}: thinking {ts} blinks of {tlen} frames, working {ws} of {wlen}");
            for f in 0..3600 {
                let off = (0..n).filter(|&k| blinks_slow(talk, k, f as f32 / 60.0)).count();
                assert!(off <= n / 2, "{talk:?}: {off} off");
            }
        }
    }
}
