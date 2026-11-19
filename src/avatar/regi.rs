//! Successors of the regi sprites (sprites/regi.sprites): the 3 eye patterns of the regis, 7 dots
//! each, in 2 kinds of avatar:
//! - flat: a 1-dot braille frame (square, round, dotted, dashed, sparse), the 7 dots inside
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

/// The talk of the working status: 1 rhythm per avatar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Talk {
    /// In reading order on, then all off.
    Sweep,
    /// Syllables: a random set per beat.
    Chatter,
    /// A wave from the center out.
    CenterOut,
    /// 2 dots run around the ring, the center on.
    Chase,
    /// All on / off in a morse rhythm.
    Morse,
    /// 1 row at a time, top to bottom.
    Rows,
    /// Left and right in turn, the center on.
    Halves,
    /// All on, 2 quick blinks per beat.
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

/// Dot K of PAT on at time T, status S (0 working, 1 thinking, 2 idle, 3 blocked, 4 done).
/// BLINK: the idle blink style (0 single, 1 double, 2 row by row).
pub fn lit(pat: Pattern, talk: Talk, blink: usize, s: usize, t: f32, k: usize) -> bool {
    let d = pat.dots();
    let (x, y) = d[k];
    let center = x == 0.0 && y == 0.0;
    // the ring: the dots that are not the center, by angle
    let n = d.len();
    let ring: Vec<usize> = {
        let mut r: Vec<usize> = (0..n).filter(|&i| d[i] != (0.0, 0.0)).collect();
        r.sort_by(|&a, &b| d[a].1.atan2(d[a].0).total_cmp(&d[b].1.atan2(d[b].0)));
        r
    };
    let ring_at = ring.iter().position(|&i| i == k);
    match s {
        0 => match talk {
            Talk::Sweep => {
                let step = (t * 11.0) as usize % (n + 3);
                k <= step && step < n
            }
            Talk::Chatter => hash((t * 8.0) as u64 * 13 + k as u64) < 0.55,
            Talk::CenterOut => {
                let r = if center { 0 } else if x.abs() <= 0.5 && y.abs() < 1.0 { 1 } else { 2 };
                (t * 7.0) as usize % 4 == r
            }
            Talk::Chase => center || ring_at.is_some_and(|i| ((t * 10.0) as usize + ring.len() - i) % ring.len() < 2),
            Talk::Morse => b"1101011100"[(t * 9.0) as usize % 10] == b'1',
            Talk::Rows => (t * 7.0) as usize % 5 == (y + 1.5) as usize,
            Talk::Halves => center || ((t * 5.0) as usize).is_multiple_of(2) == (x < 0.0),
            Talk::Stutter => !matches!(((t * 12.0) as usize) % 8, 1 | 3),
        },
        1 => center || ring_at == Some((t * 3.5) as usize % ring.len().max(1)),
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

// ─── flat ────────────────────────────────────────────────────────────────────

/// Braille frames: 1 dot wide, the rectangle x 1..13, y 2..14 of the 14x16 dots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame {
    Square,
    Round,
}

impl Frame {
    pub fn name(self) -> &'static str {
        match self {
            Frame::Square => "square",
            Frame::Round => "round",
        }
    }

    /// Dot (X, Y) of the frame line on.
    fn on(self, x: usize, y: usize) -> bool {
        let (x0, x1, y0, y1) = (1, 13, 2, 14);
        let edge_x = (x == x0 || x == x1) && (y0..=y1).contains(&y);
        let edge_y = (y == y0 || y == y1) && (x0..=x1).contains(&x);
        match self {
            Frame::Square => edge_x || edge_y,
            Frame::Round => (edge_x || edge_y) && !(edge_x && edge_y),
        }
    }
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Flat(Frame),
    Solid(Solid, Motion),
}

pub struct Regi {
    pub kind: Kind,
    pub pattern: Pattern,
    /// Flat: rows of the eyes 3 dots apart, not 2.
    pub tall: bool,
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
            Kind::Flat(f) => format!("flat · {}{}", f.name(), if self.tall { " · tall" } else { "" }),
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

/// The kept ones of the 1st sets (by their old number), then the new angular solids.
pub fn catalog() -> Vec<Regi> {
    let mut out = Vec::new();
    let mut k = 0u64;
    // flat: square and round, the others dropped
    for (p, &pattern) in PATTERNS[..3].iter().enumerate() {
        for tall in [false, true] {
            // steel: only the tall ones stay
            if p == 2 && !tall {
                continue;
            }
            for (f, frame) in [Frame::Square, Frame::Round].into_iter().enumerate() {
                out.push(Regi { kind: Kind::Flat(frame), pattern, tall, talk: TALKS[(f + p * 3 + tall as usize * 5) % 8], blink: (f + p) % 3, phase: hash(k) * 10.0, yaw: 0.0, star: false, new: false });
                k += 1;
            }
        }
    }
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
            let kind = Kind::Solid(solid, MOTIONS[(si + pi * 2) % 5]);
            out.push(Regi { kind, pattern, tall: false, talk: TALKS[(idx * 5 + si) % 8], blink: idx % 3, phase: hash(k) * 10.0, yaw: YAWS[(si + pi) % 4], star: false, new: false });
            k += 1;
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
            out.push(Regi { kind, pattern, tall: false, talk: TALKS[(idx * 3 + j) % 8], blink: idx % 3, phase: hash(k) * 10.0, yaw: YAWS[(si + j) % 4], star: false, new: false });
            k += 1;
        }
    }
    out
}

/// The pick: 074 of the 1st set, as it was (the spawn page's agent).
pub fn star() -> Regi {
    Regi { kind: Kind::Solid(Solid::Disk, Motion::Nod), pattern: Pattern::Ice, tall: false, talk: Talk::Stutter, blink: 1, phase: hash(73) * 10.0, yaw: 0.35, star: true, new: false }
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

/// R at rest: every dot on, idle colors, the pose at T 0.
pub fn rest(r: &Regi) -> Vec<Cell> {
    render_with(r, r.pattern, 2, 0.0, 1.0, |_| true)
}

fn render_with(r: &Regi, pat: Pattern, s: usize, t: f32, speed: f32, on: impl Fn(usize) -> bool) -> Vec<Cell> {
    let (eye_c, frame_c) = colors(s);
    let dots = match r.kind {
        Kind::Flat(f) => flat_dots(r, f, pat, on),
        Kind::Solid(solid, motion) => {
            let t = t + r.phase;
            let (pitch, roll, bob) = pose(motion, s, t * speed);
            // blocked: a shake now and then
            let yaw = if s == 3 && (t * 0.8).fract() < 0.2 { r.yaw + (t * 50.0).sin() * 0.1 } else { r.yaw };
            let (mesh, eye, ey) = solid.mesh();
            let marks: Vec<(f32, f32, u8)> = pat.dots().iter().enumerate().filter(|(k, _)| on(*k)).map(|(_, &(x, y))| (x * eye.0, ey - y * eye.1, 2)).collect();
            draw3d(&mesh, &[], &marks, (yaw, pitch, roll), bob)
        }
    };
    cells(&dots, eye_c, frame_c)
}

pub const DW: usize = 14;
pub const DH: usize = 16;

/// 14x16 dots (0 off, 1 frame, 2 eye) → 7x4 cells; a cell with an eye dot: the eye color.
pub fn cells(dots: &[u8], eye_c: super::glyph::Rgb, frame_c: super::glyph::Rgb) -> Vec<Cell> {
    let mut out = Vec::with_capacity(28);
    for cy in 0..4 {
        for cx in 0..7 {
            let (mut bits, mut eye) = (0, false);
            for dy in 0..4 {
                for dx in 0..2 {
                    let v = dots[(cy * 4 + dy) * DW + cx * 2 + dx];
                    if v > 0 {
                        bits |= BIT[dy][dx];
                    }
                    eye |= v == 2;
                }
            }
            out.push(Cell { ch: braille(bits), fg: if eye { eye_c } else { frame_c }, bg: None });
        }
    }
    out
}

fn flat_dots(r: &Regi, f: Frame, pat: Pattern, on: impl Fn(usize) -> bool) -> Vec<u8> {
    let mut dots = vec![0u8; DW * DH];
    for y in 0..DH {
        for x in 0..DW {
            if f.on(x, y) {
                dots[y * DW + x] = 1;
            }
        }
    }
    // the eyes around (7, 8): x 3..11, rows 2 (or 3) dots apart
    let ay = if r.tall { 3.0 } else { 2.0 };
    for (k, (x, y)) in pat.dots().iter().enumerate() {
        if on(k) {
            let (dx, dy) = ((7.0 + x * 4.0).round() as usize, (8.0 + y * ay).round() as usize);
            dots[dy.min(DH - 1) * DW + dx.min(DW - 1)] = 2;
        }
    }
    dots
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
pub fn draw3d(mesh: &Mesh, wires: &[(P3, P3)], marks: &[(f32, f32, u8)], (yaw, pitch, roll): (f32, f32, f32), bob: f32) -> Vec<u8> {
    let vr: Vec<P3> = mesh.verts.iter().map(|&p| rot(p, yaw, pitch, roll)).collect();
    // perspective: a camera 40 dots away
    let proj = |p: P3| -> (f32, f32) {
        let k = 40.0 / (40.0 - p.2);
        (6.6 + p.0 * k, 7.6 - (p.1 + bob) * k)
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
    let line = |a: (f32, f32), b: (f32, f32), set: &mut dyn FnMut(f32, f32, u8)| {
        let n = ((b.0 - a.0).hypot(b.1 - a.1) * 1.6).ceil().max(1.0) as usize;
        for i in 0..=n {
            let u = i as f32 / n as f32;
            set(a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u, 1);
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
    if norms[0].2 > 0.2 {
        for &(x, y, v) in marks {
            let (px, py) = proj(rot((x, y, mesh.front_z(x, y)), yaw, pitch, roll));
            set(px, py, v);
        }
    }
    dots
}
