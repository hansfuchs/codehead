//! Samples → terminal cells, 5 ways:
//! - ramp: 1 sample per cell, a density ramp (classic donut.c)
//! - shape: 15 samples per cell (3x5), the printable ASCII glyph whose ink layout, measured
//!   on JetBrains Mono (glyphs.rs), matches best: silhouettes and shadow edges come out as the
//!   glyphs that have ink on that side of the cell
//! - edge: 2x2 samples, silhouette and depth breaks as / \ | _ lines, ramp inside
//! - braille: 2x4 dots per cell, each dot on above its 4x4 Bayer threshold (ordered dither)
//! - half: 2 truecolor pixels per cell (▀ fg top, bg bottom)

use super::glyphs::{self, GLYPHS};
use super::sdf::{EYE, Px, View};

pub type Rgb = (u8, u8, u8);

pub const BG: Rgb = (21, 7, 0);
pub const INK: Rgb = (227, 210, 190);
pub const CORAL: Rgb = (255, 117, 101);
pub const MUTED: Rgb = (150, 124, 104);
pub const WARN: Rgb = (224, 170, 60);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
}

impl Cell {
    pub const BLANK: Cell = Cell { ch: ' ', fg: INK, bg: None };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Ramp,
    Shape,
    Edge,
    Braille,
    Half,
    /// Shape glyphs on the edges, braille dither inside.
    ShapeBrailleFill,
    /// Shape glyphs in the light, braille dither in the shadow.
    ShapeBrailleShade,
    /// Braille on the silhouette (sub-cell outline), shape glyphs inside.
    BrailleEdgeShape,
    /// Per cell: the shape glyph, or braille where no glyph fits well.
    BestFit,
}

pub const MODES: [Mode; 9] = [
    Mode::Edge,
    Mode::Shape,
    Mode::ShapeBrailleFill,
    Mode::ShapeBrailleShade,
    Mode::BrailleEdgeShape,
    Mode::BestFit,
    Mode::Braille,
    Mode::Half,
    Mode::Ramp,
];

impl Mode {
    /// Modes of the shape pass.
    pub fn shaped(self) -> bool {
        matches!(self, Mode::Shape | Mode::ShapeBrailleFill | Mode::ShapeBrailleShade | Mode::BrailleEdgeShape | Mode::BestFit)
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Ramp => "ramp",
            Mode::Shape => "shape",
            Mode::Edge => "edge",
            Mode::Braille => "braille",
            Mode::Half => "half-block",
            Mode::ShapeBrailleFill => "shape + braille fill",
            Mode::ShapeBrailleShade => "shape + braille shade",
            Mode::BrailleEdgeShape => "braille edge + shape",
            Mode::BestFit => "best fit",
        }
    }
    pub fn about(self) -> &'static str {
        match self {
            Mode::Ramp => "1 ray per cell, brightness → \" .,:;-=+*#%@\"",
            Mode::Shape => "15 rays per cell, the ASCII glyph with the closest ink layout",
            Mode::Edge => "silhouette + depth breaks as / \\ | _, ramp inside",
            Mode::Braille => "8 dots per cell, 4x4 Bayer ordered dither",
            Mode::Half => "2 truecolor pixels per cell, ▀ fg + bg",
            Mode::ShapeBrailleFill => "glyphs carve the outline and folds, dithered braille dots fill the stone",
            Mode::ShapeBrailleShade => "glyphs where light falls, braille dots where shadow falls",
            Mode::BrailleEdgeShape => "the outline in braille (8 dots per cell), glyphs inside",
            Mode::BestFit => "the glyph if 1 fits the cell well, else its 8 braille dots",
        }
    }
}

/// Stone tint: the ink, or the accent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Ink,
    Coral,
    Mono(Rgb),
}

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// Color of a lit value.
pub fn color(tint: Tint, px: &Px) -> Rgb {
    if px.mat == EYE {
        return mix(BG, CORAL, 0.25 + 0.75 * px.lum);
    }
    let l = px.lum.powf(0.8);
    match tint {
        Tint::Ink => mix((70, 42, 30), INK, l),
        Tint::Coral => {
            if l < 0.55 { mix((60, 22, 14), CORAL, l / 0.55) } else { mix(CORAL, (255, 222, 206), (l - 0.55) / 0.45) }
        }
        Tint::Mono(c) => mix(mix(BG, c, 0.3), c, l),
    }
}

pub const BAYER4: [[f32; 4]; 4] = [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];

/// Bayer threshold of dot (X, Y), 0..1.
pub fn bayer(x: usize, y: usize) -> f32 {
    (BAYER4[y % 4][x % 4] + 0.5) / 16.0
}

const RAMP: &[char] = &[' ', '.', ',', ':', ';', '-', '=', '+', '*', '#', '%', '@'];

fn ramp(l: f32) -> char {
    RAMP[((l.clamp(0.0, 1.0) * (RAMP.len() - 1) as f32).round() as usize).min(RAMP.len() - 1)]
}

/// Glyph table normalized per region: each region divided by its largest value over all glyphs,
/// so a fully inked region asks for the inkiest glyph there (Alex Harri's shape vectors).
fn table() -> &'static Vec<(char, [f32; N])> {
    static T: std::sync::OnceLock<Vec<(char, [f32; N])>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut max = [0.0f32; N];
        for (_, v) in GLYPHS.iter() {
            for i in 0..N {
                max[i] = max[i].max(v[i]);
            }
        }
        GLYPHS
            .iter()
            .filter(|(c, _)| ALPHABET.contains(*c))
            .map(|(c, v)| (*c, std::array::from_fn(|i| v[i] / max[i].max(1e-3))))
            .collect()
    })
}

const N: usize = glyphs::COLS * glyphs::ROWS;

/// Glyphs the shape mode may use: printable ASCII minus the busy ones (@ $ & { } ? Q g O, digits but 0),
/// so fills read as carved stone, not noise.
const ALPHABET: &str = " .,:;'\"`^-_~=+*!|/\\()[]<>ilrtfJLYTVxXznmwvuoaecsbdpqGhkKNMWBRD0#%";

/// The glyph whose ink layout is closest to the sampled ink V (0..1 per region). Contrast K > 1
/// pushes the regions of a cell apart, relative to its inkiest one: sharper edges.
fn shape_glyph(v: &[f32; N], k: f32) -> (char, f32) {
    let max = v.iter().cloned().fold(0.0f32, f32::max);
    let v: [f32; N] = if max > 0.0 { v.map(|x| (x / max).powf(k) * max) } else { *v };
    let mut best = (' ', f32::MAX);
    for (c, g) in table() {
        let d: f32 = (0..N).map(|i| (v[i] - g[i]) * (v[i] - g[i])).sum();
        if d < best.1 {
            best = (*c, d);
        }
    }
    best
}

/// Ink of a sample: stone by its light (soft threshold), eyes full, a miss 0.
fn ink(p: &Px, o: &Opts) -> f32 {
    if !p.hit {
        0.0
    } else if p.mat == EYE {
        1.0
    } else {
        // a floor keeps shadowed stone visible: the silhouette stays whole
        let t = ((p.lum - o.ink.0) / (o.ink.1 - o.ink.0)).clamp(0.0, 1.0);
        o.ink.2 + (1.0 - o.ink.2) * t * t * (3.0 - 2.0 * t)
    }
}

/// Options per render.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    pub mode: Mode,
    pub tint: Tint,
    /// Lit value multiplier: fades, dissolves.
    pub gain: f32,
    /// Shift of the Bayer matrix per frame: shimmer in braille.
    pub jitter: usize,
    /// Lowest lit value of a hit: tiny renders keep their silhouette.
    pub floor: f32,
    /// Shape mode: lit values from .0 (ink .2) to .1 (full ink).
    pub ink: (f32, f32, f32),
    /// Shape mode: contrast inside a cell, 1 = none.
    pub contrast: f32,
    /// Shape mode: fold between neighbor normals that cuts a gap (1 - cos), 0 = off.
    pub crease: f32,
}

impl Opts {
    pub fn new(mode: Mode) -> Opts {
        Opts { mode, tint: Tint::Ink, gain: 1.0, jitter: 0, floor: 0.0, ink: (0.45, 0.65, 0.2), contrast: 1.0, crease: 0.15 }
    }
}

/// The view in a W x H cell grid, row-major. A cell: ~1:2 (w:h), so 1 cell = 1/H of the height
/// in x, 2/H in y.
pub fn render(view: &View, w: usize, h: usize, o: Opts) -> Vec<Cell> {
    let at = |cx: f32, cy: f32| -> Px {
        let u = (cx - w as f32 / 2.0) / h as f32;
        let v = 1.0 - 2.0 * cy / h as f32;
        let mut px = view.sample(u, v);
        if px.hit {
            px.lum = ((o.floor + (1.0 - o.floor) * px.lum) * o.gain).clamp(0.0, 1.0);
        }
        px
    };
    let shape = if o.mode.shaped() { shape_pass(&at, w, h, o) } else { Vec::new() };
    let mut out = vec![Cell::BLANK; w * h];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            out[y * w + x] = match o.mode {
                Mode::Ramp => {
                    let p = at(fx + 0.5, fy + 0.5);
                    if p.hit { Cell { ch: ramp(p.lum.max(0.09)), fg: color(o.tint, &p), bg: None } } else { Cell::BLANK }
                }
                Mode::Shape | Mode::ShapeBrailleFill | Mode::ShapeBrailleShade | Mode::BrailleEdgeShape | Mode::BestFit => shape[y * w + x],
                Mode::Edge => edge_cell(&at, fx, fy, o),
                Mode::Braille => {
                    let mut bits = 0u32;
                    let mut sum = 0.0;
                    let mut n = 0;
                    let mut eye = Px::default();
                    const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
                    for dy in 0..4 {
                        for dx in 0..2 {
                            let p = at(fx + 0.25 + 0.5 * dx as f32, fy + 0.125 + 0.25 * dy as f32);
                            if !p.hit {
                                continue;
                            }
                            if p.mat == EYE {
                                eye = p;
                            }
                            if p.lum > bayer(x * 2 + dx + o.jitter, y * 4 + dy + o.jitter / 2) {
                                bits |= BIT[dy][dx];
                            }
                            sum += p.lum;
                            n += 1;
                        }
                    }
                    if bits == 0 {
                        Cell::BLANK
                    } else {
                        let p = if eye.hit { eye } else { Px { hit: true, lum: 0.35 + 0.65 * sum / n as f32, ..Px::default() } };
                        Cell { ch: char::from_u32(0x2800 + bits).unwrap_or(' '), fg: color(o.tint, &p), bg: None }
                    }
                }
                Mode::Half => {
                    let t = at(fx + 0.5, fy + 0.25);
                    let b = at(fx + 0.5, fy + 0.75);
                    match (t.hit, b.hit) {
                        (false, false) => Cell::BLANK,
                        (true, false) => Cell { ch: '▀', fg: color(o.tint, &t), bg: None },
                        (false, true) => Cell { ch: '▄', fg: color(o.tint, &b), bg: None },
                        (true, true) => Cell { ch: '▀', fg: color(o.tint, &t), bg: Some(color(o.tint, &b)) },
                    }
                }
            };
        }
    }
    out
}

/// Shape mode: 3x5 samples per cell over the whole grid, creases carved out as gaps (a depth
/// step or a fold between 2 neighbor samples: the deeper one loses its ink), then 1 glyph per
/// cell by its ink layout. The gaps read like the letter cuts in a neofetch logo.
fn shape_pass(at: &impl Fn(f32, f32) -> Px, w: usize, h: usize, o: Opts) -> Vec<Cell> {
    let (gc, gr) = (glyphs::COLS, glyphs::ROWS);
    let (sw, sh) = (w * gc, h * gr);
    let px: Vec<Px> = (0..sw * sh).map(|i| at(((i % sw) as f32 + 0.5) / gc as f32, ((i / sw) as f32 + 0.5) / gr as f32)).collect();
    let mut ink: Vec<f32> = px.iter().map(|p| ink(p, &o)).collect();
    if o.crease > 0.0 {
        for y in 0..sh {
            for x in 0..sw {
                let a = &px[y * sw + x];
                if !a.hit || a.mat == EYE {
                    continue;
                }
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    if nx >= sw || ny >= sh {
                        continue;
                    }
                    let b = &px[ny * sw + nx];
                    if !b.hit || b.mat == EYE {
                        continue;
                    }
                    let step = (a.depth - b.depth).abs() > 0.06;
                    let fold = a.n.dot(b.n) < 1.0 - o.crease;
                    if step || fold {
                        let k = if a.depth > b.depth { y * sw + x } else { ny * sw + nx };
                        ink[k] = 0.0;
                    }
                }
            }
        }
    }
    let mut out = vec![Cell::BLANK; w * h];
    for cy in 0..h {
        for cx in 0..w {
            let mut v = [0.0; N];
            let (mut sum, mut n, mut eye) = (0.0, 0, false);
            for i in 0..N {
                let k = (cy * gr + i / gc) * sw + cx * gc + i % gc;
                v[i] = ink[k];
                if px[k].hit {
                    sum += px[k].lum;
                    n += 1;
                    eye |= px[k].mat == EYE;
                }
            }
            if n == 0 {
                continue;
            }
            let (ch, err) = shape_glyph(&v, o.contrast);
            let mean = v.iter().sum::<f32>() / N as f32;
            let lum = sum / n as f32;
            let use_braille = match o.mode {
                Mode::ShapeBrailleFill => n == N && v.iter().all(|&x| x > 0.15) && !eye,
                Mode::ShapeBrailleShade => mean < 0.45 && !eye,
                Mode::BrailleEdgeShape => n < N && !eye,
                Mode::BestFit => err > 0.9 && !eye,
                _ => false,
            };
            let p = Px { hit: true, lum: 0.3 + 0.7 * lum, mat: if eye { EYE } else { 0 }, ..Px::default() };
            if use_braille {
                let bits = braille_bits(at, cx, cy, o);
                if bits != 0 {
                    out[cy * w + cx] = Cell { ch: char::from_u32(0x2800 + bits).unwrap_or(' '), fg: color(o.tint, &p), bg: None };
                }
            } else if ch != ' ' {
                out[cy * w + cx] = Cell { ch, fg: color(o.tint, &p), bg: None };
            }
        }
    }
    out
}

/// 8 braille dots of cell (CX, CY): a dot is on when its ink beats its Bayer threshold; on the
/// silhouette (edge mode) a dot is on when its ray hits.
fn braille_bits(at: &impl Fn(f32, f32) -> Px, cx: usize, cy: usize, o: Opts) -> u32 {
    const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
    let mut bits = 0;
    for dy in 0..4 {
        for dx in 0..2 {
            let p = at(cx as f32 + 0.25 + 0.5 * dx as f32, cy as f32 + 0.125 + 0.25 * dy as f32);
            let on = match o.mode {
                Mode::BrailleEdgeShape => p.hit,
                Mode::ShapeBrailleShade => p.hit && 0.25 + 1.4 * p.lum > bayer(cx * 2 + dx, cy * 4 + dy),
                _ => p.hit && 0.15 + 0.85 * p.lum > bayer(cx * 2 + dx, cy * 4 + dy),
            };
            if on {
                bits |= BIT[dy][dx];
            }
        }
    }
    bits
}

fn edge_cell(at: &impl Fn(f32, f32) -> Px, fx: f32, fy: f32, o: Opts) -> Cell {
    // tl tr bl br
    let s = [at(fx + 0.25, fy + 0.25), at(fx + 0.75, fy + 0.25), at(fx + 0.25, fy + 0.75), at(fx + 0.75, fy + 0.75)];
    let hits = s.iter().filter(|p| p.hit).count();
    if hits == 0 {
        return Cell::BLANK;
    }
    // presence (silhouette) or nearness (depth break): high = solid
    let silhouette = hits < 4;
    let dmin = s.iter().filter(|p| p.hit).map(|p| p.depth).fold(f32::MAX, f32::min);
    let dmax = s.iter().filter(|p| p.hit).map(|p| p.depth).fold(0.0, f32::max);
    let crease = !silhouette && dmax - dmin > 0.12;
    let best = *s.iter().filter(|p| p.hit).max_by(|a, b| a.lum.total_cmp(&b.lum)).unwrap_or(&s[0]);
    let eye = s.iter().find(|p| p.hit && p.mat == EYE);
    if (silhouette || crease) && eye.is_none() {
        let val = |p: &Px| if !p.hit { 0.0 } else if silhouette { 1.0 } else { dmax - p.depth };
        let v = s.map(|p| val(&p));
        let gx = (v[1] + v[3]) - (v[0] + v[2]);
        let gy = (v[2] + v[3]) - (v[0] + v[1]);
        let ch = if gx.abs() > 2.0 * gy.abs() {
            '|'
        } else if gy.abs() > 2.0 * gx.abs() {
            if gy > 0.0 { '_' } else { '-' }
        } else if gx * gy > 0.0 {
            '/'
        } else {
            '\\'
        };
        let p = Px { lum: 0.5 + 0.5 * best.lum, ..best };
        return Cell { ch, fg: color(o.tint, &p), bg: None };
    }
    let p = eye.copied().unwrap_or(best);
    let mean = s.iter().map(|p| p.lum).sum::<f32>() / 4.0;
    let ch = if p.mat == EYE { '@' } else { ramp(mean.powf(0.85).max(0.09)) };
    Cell { ch, fg: color(o.tint, &p), bg: None }
}

/// Braille outline of the view in a W x H cell grid (2x4 dots per cell): only the edges on, the
/// areas empty (the regi look). A dot is on when it hits and: a 4-neighbor misses (the
/// silhouette), or a neighbor sits across a fold (normals apart by more than O.crease, min.
/// 0.2; the nearer of the 2 on), or it is the near side of a depth step. Eye dots: all on. Then lone dots and 1-dot spurs out.
pub fn outline(view: &View, w: usize, h: usize, o: Opts) -> Vec<Cell> {
    let (dw, dh) = (w * 2, h * 4);
    let px: Vec<Px> = (0..dw * dh)
        .map(|i| {
            let (x, y) = ((i % dw) as f32, (i / dw) as f32);
            let u = ((x + 0.5) / 2.0 - w as f32 / 2.0) / h as f32;
            let v = 1.0 - 2.0 * ((y + 0.5) / 4.0) / h as f32;
            view.sample(u, v)
        })
        .collect();
    let hit = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < dw && (y as usize) < dh && px[y as usize * dw + x as usize].hit;
    let mut dots = vec![0u8; dw * dh];
    let fold = 1.0 - o.crease.max(0.2);
    for y in 0..dh as i32 {
        for x in 0..dw as i32 {
            let i = y as usize * dw + x as usize;
            let p = &px[i];
            if !p.hit {
                continue;
            }
            if p.mat == EYE {
                dots[i] = 2;
                continue;
            }
            if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(ox, oy)| !hit(x + ox, y + oy)) {
                dots[i] = 1;
            }
            for (ox, oy) in [(1, 0), (0, 1)] {
                if !hit(x + ox, y + oy) {
                    continue;
                }
                let j = (y + oy) as usize * dw + (x + ox) as usize;
                let q = &px[j];
                if q.mat == EYE {
                    continue;
                }
                if p.n.dot(q.n) < fold {
                    let k = if p.depth <= q.depth { i } else { j };
                    dots[k] = dots[k].max(1);
                }
            }
            // a depth step: the depth bends (2nd difference) toward the viewer here; a plane
            // seen at a slant keeps a straight depth (0 dots on it), the near side of a step on
            for (ox, oy) in [(1, 0), (0, 1)] {
                if hit(x - ox, y - oy) && hit(x + ox, y + oy) {
                    let d = |k: i32| px[(y + oy * k) as usize * dw + (x + ox * k) as usize].depth;
                    if d(-1) + d(1) - 2.0 * p.depth > 0.12 {
                        dots[i] = 1;
                    }
                }
            }
        }
    }
    super::regi::clean_in(&mut dots, dw, dh, true);
    const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
    let mut out = vec![Cell::BLANK; w * h];
    for cy in 0..h {
        for cx in 0..w {
            let (mut bits, mut eye, mut sum, mut n) = (0u32, false, 0.0, 0);
            for dy in 0..4 {
                for dx in 0..2 {
                    let i = (cy * 4 + dy) * dw + cx * 2 + dx;
                    if dots[i] != 0 {
                        bits |= BIT[dy][dx];
                        eye |= dots[i] == 2;
                        sum += px[i].lum;
                        n += 1;
                    }
                }
            }
            if bits != 0 {
                let p = Px { hit: true, lum: 0.45 + 0.55 * sum / n as f32, mat: if eye { EYE } else { 0 }, ..Px::default() };
                out[cy * w + cx] = Cell { ch: char::from_u32(0x2800 + bits).unwrap_or(' '), fg: color(o.tint, &p), bg: None };
            }
        }
    }
    out
}
