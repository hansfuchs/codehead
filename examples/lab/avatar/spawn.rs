//! 50 spawn animations: an agent's 7x4 sprite forms out of braille goo when a thread or codehead
//! opens. 10 families x 5 variants. Each dot of the sprite's first frame is a blob of goo
//! (box lines become dots too): blobs near each other melt into 1 mass (a metaball field), they
//! stick at the start and lag behind (trails), overshoot their place and wobble to rest, then
//! the blob shrinks to 1 dot and the sprite snaps in, alive.
//!
//! The canvas: 13x6 cells (26x24 dots), the sprite at cell (3, 1).

use super::glyph::{BG, CORAL, Cell, INK, mix};
use super::regi::hash;

pub const CW: usize = 13;
pub const CH: usize = 6;
const W: usize = CW * 2;
const H: usize = CH * 4;
const OX: usize = 3;
const OY: usize = 1;
/// The loop: the formation, the snap, the hold, a blank gap.
pub const PERIOD: f32 = 4.2;
/// Goo: flights this much slower, the wobble after the arrival this long (s).
const GOO: f32 = 1.35;
const SETTLE: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Family {
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

pub const FAMILIES: [Family; 10] =
    [Family::Fountain, Family::Splash, Family::Rain, Family::Vortex, Family::Rise, Family::Ripple, Family::Wave, Family::Condense, Family::Drip, Family::Merge];

impl Family {
    fn names(self) -> [&'static str; 5] {
        match self {
            Family::Fountain => ["fountain", "fountain · twin jets", "fountain · high arc", "fountain · low jet", "fountain · from the left"],
            Family::Splash => ["splash", "splash · loose spring", "splash · hard snap", "splash · 2 drops", "splash · up from below"],
            Family::Rain => ["rain", "rain · wind", "rain · heavy", "rain · drizzle", "rain · bottom up"],
            Family::Vortex => ["vortex", "vortex · tight", "vortex · reverse", "vortex · outward", "vortex · drain"],
            Family::Rise => ["rise · from a puddle", "rise · elastic", "rise · middle out", "rise · bottom first", "rise · 2 puddles"],
            Family::Ripple => ["ripple", "ripple · 2 rings", "ripple · 3 rings", "ripple · fast", "ripple · from below"],
            Family::Wave => ["wave · from the left", "wave · from the right", "wave · both sides", "wave · from below", "wave · tall"],
            Family::Condense => ["condense", "condense · steam", "condense · ring cloud", "condense · sparse", "condense · hard snap"],
            Family::Drip => ["drip", "drip · from the left", "drip · 2 taps", "drip · fill up", "drip · splashing"],
            Family::Merge => ["merge · 2 drops", "merge · 3 from above", "merge · 4 corners", "merge · 1 big drop", "merge · bubbles"],
        }
    }
}

pub fn count() -> usize {
    FAMILIES.len() * 5
}

pub fn name(i: usize) -> &'static str {
    FAMILIES[i / 5].names()[i % 5]
}

/// The family, the variant of animation I: "rain", "wind" (the 1st variant: "").
pub fn label(i: usize) -> (&'static str, &'static str) {
    let n = name(i);
    match n.split_once(" · ") {
        Some((f, v)) => (f, v),
        None => (n, ""),
    }
}

/// Dots of a cell as bits (2x4, the braille layout): braille as is, box lines as dots on the
/// line, inward (COL: the cell's column in the sprite).
fn mask(ch: char, col: usize) -> [[bool; 2]; 4] {
    let mut m = [[false; 2]; 4];
    let b = ch as u32;
    if (0x2800..=0x28FF).contains(&b) {
        const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
        for (y, row) in BIT.iter().enumerate() {
            for (x, bit) in row.iter().enumerate() {
                m[y][x] = (b - 0x2800) & bit != 0;
            }
        }
        return m;
    }
    // up, down, left, right
    let (u, d, l, r) = match ch {
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
        _ => return m,
    };
    let cx = if col == 0 { 1 } else { 0 };
    m[1][cx] = true;
    if u {
        m[0][cx] = true;
    }
    if d {
        m[2][cx] = true;
        m[3][cx] = true;
    }
    if l || r {
        m[1][0] = true;
        m[1][1] = true;
    }
    m
}

struct Particle {
    /// Target, dots.
    g: (f32, f32),
    /// 0..1 per particle.
    r: (f32, f32, f32),
    delay: f32,
}

fn ease_out(u: f32) -> f32 {
    1.0 - (1.0 - u).powi(3)
}

fn ease_in_out(u: f32) -> f32 {
    if u < 0.5 { 4.0 * u * u * u } else { 1.0 - (-2.0 * u + 2.0).powi(3) / 2.0 }
}

fn ease_back(u: f32, k: f32) -> f32 {
    let u = u - 1.0;
    1.0 + (k + 1.0) * u * u * u + k * u * u
}

const C: (f32, f32) = ((OX * 2 + 7) as f32, (OY * 4 + 8) as f32);

fn dot(dots: &mut [u8], x: f32, y: f32, hot: bool) {
    let (x, y) = (x.round(), y.round());
    if x >= 0.0 && y >= 0.0 && (x as usize) < W && (y as usize) < H {
        let k = y as usize * W + x as usize;
        dots[k] = dots[k].max(if hot { 2 } else { 1 });
    }
}

/// Sticky timing: slow to let go, slow to arrive.
fn sticky(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    let u = u * u * (3.0 - 2.0 * u);
    u * u * (3.0 - 2.0 * u)
}

/// Animation I: the stagger, the flight time of 1 blob, the snap (s from the start).
pub fn timing(i: usize) -> (f32, f32, f32) {
    let (fam, v) = (FAMILIES[i / 5], i % 5);
    let (stagger, dur): (f32, f32) = match (fam, v) {
        (Family::Fountain, 3) => (0.5, 0.45),
        (Family::Fountain, _) => (0.7, 0.75),
        (Family::Splash, 2) => (0.25, 0.55),
        (Family::Splash, _) => (0.35, 0.9),
        (Family::Rain, 3) => (1.0, 0.5),
        (Family::Rain, _) => (0.7, 0.6),
        (Family::Vortex, 1) => (0.3, 1.0),
        (Family::Vortex, _) => (0.4, 1.1),
        (Family::Rise, _) => (0.6, 0.8),
        (Family::Ripple, 3) => (0.0, 0.6),
        (Family::Ripple, _) => (0.0, 1.2),
        (Family::Wave, _) => (0.8, 0.7),
        (Family::Condense, 4) => (0.4, 0.9),
        (Family::Condense, _) => (0.5, 1.1),
        (Family::Drip, _) => (1.1, 0.5),
        (Family::Merge, _) => (0.0, 1.7),
    };
    let dur = if fam == Family::Merge { dur } else { dur * GOO };
    (stagger, dur, stagger + dur + if fam == Family::Merge { 0.0 } else { SETTLE })
}

/// The canvas of animation I at T: 13x6 cells. SPRITE: the 7x4 first frame of the agent; LIVE:
/// the agent S seconds after the snap.
pub fn render(i: usize, sprite: &[Cell], live: &dyn Fn(f32) -> Vec<Cell>, t: f32) -> Vec<Cell> {
    let fam = FAMILIES[i / 5];
    let v = i % 5;
    let tau = t.rem_euclid(PERIOD);
    // targets
    let mut ps = Vec::new();
    for (ci, cell) in sprite.iter().enumerate() {
        let (cx, cy) = (ci % 7, ci / 7);
        for (y, row) in mask(cell.ch, cx).iter().enumerate() {
            for (x, &on) in row.iter().enumerate() {
                if on {
                    let g = (((OX + cx) * 2 + x) as f32, ((OY + cy) * 4 + y) as f32);
                    let k = (g.0 as u64) * 131 + g.1 as u64 + i as u64 * 7919;
                    ps.push(Particle { g, r: (hash(k), hash(k + 1), hash(k + 2)), delay: 0.0 });
                }
            }
        }
    }
    // the order: a key 0..1 per particle, the stagger, the flight time
    let dist = |g: (f32, f32)| ((g.0 - C.0).hypot((g.1 - C.1) * 1.2) / 10.0).min(1.0);
    let (stagger, dur, end) = timing(i);
    for p in ps.iter_mut() {
        let key = match (fam, v) {
            (Family::Fountain, 4) | (Family::Wave, 0) => p.g.0 / W as f32,
            (Family::Wave, 1) => 1.0 - p.g.0 / W as f32,
            (Family::Wave, 2) => 1.0 - (p.g.0 - C.0).abs() / 8.0,
            (Family::Wave, _) | (Family::Rise, 3) | (Family::Rain, 4) | (Family::Drip, 3) => 1.0 - p.g.1 / H as f32,
            (Family::Rise, 2) | (Family::Splash, _) | (Family::Vortex, 3) => dist(p.g),
            (Family::Rain, 3) | (Family::Condense, _) => p.r.0,
            (Family::Drip, _) => p.r.0 * 0.3 + (p.g.1 / H as f32) * 0.7,
            _ => 0.6 * p.r.0 + 0.4 * dist(p.g),
        };
        p.delay = key.clamp(0.0, 1.0) * stagger;
    }
    let mut dots = vec![0u8; W * H];
    // the goo: a field of blobs, radius R (dots), hot: still moving
    let mut field = vec![0f32; W * H];
    let mut heat = vec![0f32; W * H];
    let mut blob = |x: f32, y: f32, r: f32, hot: bool| {
        let n = r.ceil() as i32;
        let (cx, cy) = (x.round() as i32, y.round() as i32);
        for yy in (cy - n).max(0)..=(cy + n).min(H as i32 - 1) {
            for xx in (cx - n).max(0)..=(cx + n).min(W as i32 - 1) {
                let d2 = (xx as f32 - x).powi(2) + (yy as f32 - y).powi(2);
                let k = 1.0 - d2 / (r * r);
                if k > 0.0 {
                    let i = yy as usize * W + xx as usize;
                    field[i] += k * k;
                    if hot {
                        heat[i] += k * k;
                    }
                }
            }
        }
    };
    if tau > PERIOD - 0.3 {
        return vec![Cell::BLANK; CW * CH];
    }
    if tau < end {
        if fam == Family::Merge {
            merge(v, &ps, tau / end, &mut |x, y, h| dot(&mut dots, x, y, h));
        } else {
            for p in &ps {
                let raw = (tau - p.delay) / dur;
                if raw < 0.0 && !matches!(fam, Family::Condense | Family::Wave) {
                    continue;
                }
                let Some((mut x, mut y)) = path(fam, v, p, sticky(raw), tau) else { continue };
                // after the arrival: the inertia carries it on, it wobbles back
                let s = tau - p.delay - dur;
                if s > 0.0
                    && let Some(b) = path(fam, v, p, 0.93, tau)
                {
                    let (dx, dy) = (p.g.0 - b.0, p.g.1 - b.1);
                    let l = dx.hypot(dy).max(0.01);
                    let o = 1.6 * (-6.5 * s).exp() * (17.0 * s).sin();
                    (x, y) = (x + dx / l * o, y + dy / l * o);
                }
                // a fat blob in flight, 1 dot at rest
                let settle = (s / SETTLE).clamp(0.0, 1.0);
                let r = 2.9 - 1.9 * settle * settle;
                blob(x, y, r, settle < 0.6);
                // the trail: where it was, thinner
                if raw < 1.0 {
                    for k in 1..=2 {
                        if let Some((tx, ty)) = path(fam, v, p, sticky(raw - k as f32 * 0.07), tau) {
                            blob(tx, ty, r * (1.0 - 0.22 * k as f32), true);
                        }
                    }
                }
            }
            for k in 0..W * H {
                if field[k] > 0.5 {
                    dots[k] = if heat[k] > 0.3 { 2 } else { 1 };
                }
            }
            ripples(fam, v, tau, &mut |x, y, h| dot(&mut dots, x, y, h));
        }
        spray(fam, v, tau, end, i, &mut |x, y, h| dot(&mut dots, x, y, h));
    }
    // the canvas as cells
    let mut out = vec![Cell::BLANK; CW * CH];
    const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
    for cy in 0..CH {
        for cx in 0..CW {
            let (mut bits, mut hot) = (0, false);
            for dy in 0..4 {
                for dx in 0..2 {
                    let d = dots[(cy * 4 + dy) * W + cx * 2 + dx];
                    if d > 0 {
                        bits |= BIT[dy][dx];
                    }
                    hot |= d == 2;
                }
            }
            if bits != 0 {
                out[cy * CW + cx] = Cell { ch: char::from_u32(0x2800 + bits).unwrap_or(' '), fg: if hot { CORAL } else { mix(BG, INK, 0.8) }, bg: None };
            }
        }
    }
    if tau >= end {
        // the snap: the sprite in, alive, a coral flash for 0.12 s
        let flash = tau - end < 0.12;
        for (ci, c) in live(tau - end).iter().enumerate() {
            let k = (OY + ci / 7) * CW + OX + ci % 7;
            out[k] = if flash && c.ch != ' ' { Cell { fg: CORAL, ..*c } } else { *c };
        }
    }
    out
}

/// Position of particle P at flight progress U (0..1) and loop time TAU.
fn path(fam: Family, v: usize, p: &Particle, u: f32, tau: f32) -> Option<(f32, f32)> {
    let g = p.g;
    let (r0, r1, r2) = p.r;
    let lerp = |a: f32, b: f32, k: f32| a + (b - a) * k;
    Some(match fam {
        Family::Fountain => {
            let sx = match v {
                1 => {
                    if r0 < 0.5 { 4.0 } else { W as f32 - 5.0 }
                }
                4 => 0.0,
                _ => C.0 + (r1 - 0.5) * 2.0,
            };
            let a = match v {
                2 => 22.0,
                3 => 7.0,
                _ => 14.0,
            };
            (lerp(sx, g.0, u), lerp(H as f32 + 1.0, g.1, u) - a * 4.0 * u * (1.0 - u))
        }
        Family::Splash => {
            let s = match v {
                3 => {
                    if g.0 < C.0 { (C.0 - 5.0, C.1 - 2.0) } else { (C.0 + 5.0, C.1 - 2.0) }
                }
                4 => (C.0, H as f32 + 1.0),
                _ => C,
            };
            let (k, w) = match v {
                1 => (3.0, 3.5),
                2 => (7.0, 1.6),
                _ => (4.5, 2.6),
            };
            let e = (-k * u).exp() * (std::f32::consts::PI * w * u).cos() * (1.0 - u);
            (g.0 + (s.0 - g.0) * e, g.1 + (s.1 - g.1) * e)
        }
        Family::Rain => {
            let fall = 8.0 + r1 * 14.0;
            let wind = if v == 1 { -7.0 } else { 0.0 };
            let hit = 0.7;
            if u < hit {
                let k = (u / hit).powi(2);
                (g.0 + wind * (1.0 - k), g.1 - fall * (1.0 - k))
            } else {
                let b = (u - hit) / (1.0 - hit);
                let hb = if v == 2 { 0.0 } else { 2.5 + r2 * 1.5 };
                (g.0, g.1 - hb * (b * std::f32::consts::PI * 2.0).sin().abs() * (1.0 - b))
            }
        }
        Family::Vortex => {
            let (dx, dy) = (g.0 - C.0, g.1 - C.1);
            let (rg, ag) = (dx.hypot(dy), dy.atan2(dx));
            let spin = match v {
                1 => 9.0,
                2 => -5.0,
                _ => 5.0,
            };
            let r = if v == 3 { lerp(0.0, rg, ease_out(u)) } else { lerp(15.0 + r0 * 4.0, rg, ease_out(u)) };
            let a = ag + spin * (1.0 - ease_out(u));
            let sy = if v == 4 { 0.45 + 0.55 * u } else { 1.0 };
            (C.0 + a.cos() * r, C.1 + a.sin() * r * sy)
        }
        Family::Rise => {
            let px = if v == 4 { (if g.0 < C.0 { 3.0 } else { W as f32 - 4.0 }) + (r0 - 0.5) * 5.0 } else { C.0 + (g.0 - C.0) * 1.8 + (r0 - 0.5) * 2.0 };
            let up = if v == 1 { ease_back(u, 3.2) } else { ease_back(u, 1.4) };
            let wob = (u * std::f32::consts::PI * 3.0 + r1 * 6.0).sin() * (1.0 - u) * 1.5;
            (lerp(px, g.0, ease_in_out(u)) + wob, lerp(H as f32 - 1.0, g.1, up))
        }
        Family::Ripple => {
            // the dot appears when the front passes, a radial bump after
            let o = if v == 4 { (C.0, H as f32) } else { C };
            let speed = if v == 3 { 34.0 } else { 18.0 };
            let (dx, dy) = (g.0 - o.0, g.1 - o.1);
            let d = dx.hypot(dy);
            let front = tau * speed;
            if front < d {
                return None;
            }
            let bump = 1.6 * ((front - d) * 0.5).sin() * (-(front - d) * 0.18).exp() * (1.0 - u);
            let l = d.max(0.01);
            (g.0 + dx / l * bump, g.1 + dy / l * bump)
        }
        Family::Wave => {
            let amp = if v == 4 { 7.0 } else { 3.5 };
            let s = (g.0 * 0.45 - tau * 9.0).sin() * amp * (1.0 - ease_out(u));
            match v {
                0 | 4 => (lerp(-3.0, g.0, ease_out(u)), g.1 + s),
                1 => (lerp(W as f32 + 2.0, g.0, ease_out(u)), g.1 + s),
                2 => (lerp(if g.0 < C.0 { -3.0 } else { W as f32 + 2.0 }, g.0, ease_out(u)), g.1 + s),
                _ => (g.0 + s * 0.6, lerp(H as f32 + 2.0, g.1, ease_out(u))),
            }
        }
        Family::Condense => {
            let start = match v {
                1 => (r0 * W as f32, r1 * 6.0 - 4.0),
                2 => {
                    let a = r0 * std::f32::consts::TAU;
                    (C.0 + a.cos() * 13.0, C.1 + a.sin() * 11.0)
                }
                _ => (r0 * W as f32, r1 * H as f32),
            };
            if v == 3 && r2 < 0.5 && u < 0.6 {
                return None;
            }
            let drift = ((tau * 1.7 + r2 * 6.0).sin() * 2.5, (tau * 1.3 + r0 * 5.0).cos() * 2.0 + if v == 1 { tau * 3.0 } else { 0.0 });
            let k = if v == 4 { u.powi(5) } else { u.powi(3) };
            (lerp(start.0 + drift.0, g.0, k), lerp(start.1 + drift.1, g.1, k))
        }
        Family::Drip => {
            let tap = match v {
                1 => 2.0,
                2 => {
                    if g.0 < C.0 { C.0 - 6.0 } else { C.0 + 6.0 }
                }
                _ => C.0,
            };
            if u < 0.55 {
                let k = (u / 0.55).powi(2);
                (tap, lerp(-1.0, g.1, k))
            } else {
                let k = ease_out((u - 0.55) / 0.45);
                let hop = if v == 4 { 2.0 * (k * std::f32::consts::PI).sin() } else { 0.0 };
                (lerp(tap, g.0, k), g.1 - hop)
            }
        }
        Family::Merge => return None,
    })
}

/// Ripple rings: the fronts as circles of dots.
fn ripples(fam: Family, v: usize, tau: f32, put: &mut impl FnMut(f32, f32, bool)) {
    if fam != Family::Ripple {
        return;
    }
    let o = if v == 4 { (C.0, H as f32) } else { C };
    let speed = if v == 3 { 34.0 } else { 18.0 };
    let rings = match v {
        1 => 2,
        2 => 3,
        _ => 1,
    };
    for k in 0..rings {
        let r = (tau - k as f32 * 0.22) * speed;
        if !(0.5..20.0).contains(&r) {
            continue;
        }
        let n = (r * 4.0) as usize;
        for j in 0..n {
            let a = std::f32::consts::TAU * j as f32 / n as f32;
            put(o.0 + a.cos() * r, o.1 + a.sin() * r * 0.9, true);
        }
    }
}

/// Droplets thrown off as the liquid lands: ballistic, gone after 0.5 s.
fn spray(fam: Family, v: usize, tau: f32, end: f32, i: usize, put: &mut impl FnMut(f32, f32, bool)) {
    let (n, at, from) = match (fam, v) {
        (Family::Fountain, _) => (10, end * 0.55, (C.0, C.1 + 4.0)),
        (Family::Splash, _) => (14, 0.05, C),
        (Family::Rain, 2) => (16, end * 0.6, (C.0, C.1 + 6.0)),
        (Family::Drip, 4) | (Family::Drip, 0) => (8, end * 0.5, (C.0, C.1)),
        (Family::Rise, 1) => (8, end * 0.75, (C.0, C.1 - 6.0)),
        (Family::Merge, 3) => (12, end * 0.4, C),
        _ => return,
    };
    for k in 0..n {
        let s = tau - at - (k % 3) as f32 * 0.06;
        // gone before the snap: 0 droplets over the sprite's first frame
        if !(0.0..0.5).contains(&s) || tau > end - 0.05 {
            continue;
        }
        let h = (i * 97 + k) as u64 * 31;
        let a = -std::f32::consts::PI * (0.1 + 0.8 * hash(h));
        let sp = 14.0 + 16.0 * hash(h + 1);
        put(from.0 + a.cos() * sp * s, from.1 + a.sin() * sp * s + 40.0 * s * s, true);
    }
}

/// Metaballs: drops fly in, merge into 1 blob, the blob turns into the sprite. U: 0..1.
fn merge(v: usize, ps: &[Particle], u: f32, put: &mut impl FnMut(f32, f32, bool)) {
    let starts: &[(f32, f32)] = match v {
        0 => &[(-4.0, C.1), (W as f32 + 3.0, C.1)],
        1 => &[(C.0 - 8.0, -5.0), (C.0, -9.0), (C.0 + 8.0, -6.0)],
        2 => &[(-3.0, -3.0), (W as f32 + 2.0, -3.0), (-3.0, H as f32 + 2.0), (W as f32 + 2.0, H as f32 + 2.0)],
        3 => &[(C.0, -12.0)],
        _ => &[(C.0 - 6.0, H as f32 + 4.0), (C.0 + 2.0, H as f32 + 9.0), (C.0 + 7.0, H as f32 + 6.0), (C.0 - 1.0, H as f32 + 14.0)],
    };
    let a = (u / 0.45).clamp(0.0, 1.0);
    let m = ((u - 0.4) / 0.45).clamp(0.0, 1.0);
    let m = m * m * (3.0 - 2.0 * m);
    let rad = if v == 3 { 6.0 } else { 3.6 };
    let blobs: Vec<(f32, f32, f32)> = starts
        .iter()
        .enumerate()
        .map(|(k, &(sx, sy))| {
            let e = ease_in_out((a - k as f32 * 0.06).clamp(0.0, 1.0));
            let wob = if v == 4 { (u * 12.0 + k as f32).sin() * 1.5 } else { 0.0 };
            (sx + (C.0 - sx) * e + wob, sy + (C.1 - sy) * e, rad * (1.0 - 0.4 * m))
        })
        .collect();
    let sigma = 0.8 + (1.0 - m) * 3.0;
    for y in 0..H {
        for x in 0..W {
            let (fx, fy) = (x as f32, y as f32);
            let bf: f32 = blobs.iter().map(|&(bx, by, r)| r * r / ((fx - bx).powi(2) + (fy - by).powi(2) * 1.3 + 0.01)).sum();
            let tf = if m > 0.0 { ps.iter().map(|p| (-((fx - p.g.0).powi(2) + (fy - p.g.1).powi(2)) / (sigma * sigma)).exp()).fold(0.0, f32::max) } else { 0.0 };
            if (1.0 - m) * bf + m * tf * 2.0 > 1.0 {
                put(fx, fy, m < 0.95);
            }
        }
    }
}
