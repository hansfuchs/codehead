//! Monoliths: the moai's successor as a slab of stone in the style of the CONTROL monoliths and
//! the regis' bodies. 16 bodies (slabs and columns at different angles and slopes, an inverted
//! pyramid, slabs with moons or rocks that hover around them) x the eye patterns of regi.rs.
//! Weathered, unpolished stone lit from above, as the moai. The eye dots sit on the front as
//! coral lights; a dot that is off leaves its socket. They talk as in regi.rs.

use super::regi::{PATTERNS, Pattern, TALKS, Talk, lit};
use super::sdf::{EYE, STONE, V};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Status of the eyes (0 working … 4 done), set by the lab before a draw.
pub static STATUS: AtomicUsize = AtomicUsize::new(0);
/// The eyes forced (the website export): 0 as the talk goes, 1 all lit, 2 all off.
pub static FORCE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Body {
    Slab,
    Lean,
    LeanBack,
    Slanted,
    Gable,
    Taper,
    Sloped,
    Twin,
    Column,
    Hex,
    Board,
    Moons,
    Rocks,
    Pebbles,
    Boulders,
    Shards,
}

pub const BODIES: [Body; 16] = [
    Body::Slab,
    Body::Lean,
    Body::LeanBack,
    Body::Slanted,
    Body::Gable,
    Body::Taper,
    Body::Sloped,
    Body::Twin,
    Body::Column,
    Body::Hex,
    Body::Board,
    Body::Moons,
    Body::Rocks,
    Body::Pebbles,
    Body::Boulders,
    Body::Shards,
];

/// A satellite: center, size, kind (0 cube, 1 rock, 2 shard), the phase of its float.
type Sat = (V, f32, u8, f32);

impl Body {
    pub fn name(self) -> &'static str {
        match self {
            Body::Slab => "slab 1:4:9",
            Body::Lean => "slab, leaning aside",
            Body::LeanBack => "slab, leaning back",
            Body::Slanted => "slab, slanted top",
            Body::Gable => "stele, gable top",
            Body::Taper => "slab, tapering up",
            Body::Sloped => "slab, sloped front",
            Body::Twin => "2 slabs, staggered",
            Body::Column => "chamfered column",
            Body::Hex => "hex column",
            Body::Board => "inverted pyramid",
            Body::Moons => "slab, 3 cube moons",
            Body::Rocks => "slab, 3 rocks",
            Body::Pebbles => "slab, 6 pebbles",
            Body::Boulders => "slab, 2 boulders",
            Body::Shards => "slab, 4 shards",
        }
    }

    /// Eye center y, eye scale (x, y).
    fn face(self) -> (f32, f32, f32) {
        match self {
            Body::Board => (0.45, 0.22, 0.13),
            Body::Slanted => (0.3, 0.22, 0.14),
            Body::Hex => (0.35, 0.2, 0.14),
            _ => (0.35, 0.22, 0.14),
        }
    }

    /// World → body space: the lean.
    fn local(self, p: V) -> V {
        match self {
            Body::Lean => p.rot_z(0.13),
            Body::LeanBack => p.rot_x(0.16),
            _ => p,
        }
    }

    /// The satellites around the slab.
    fn sats(self) -> &'static [Sat] {
        match self {
            Body::Moons => &[(V(-0.72, 0.55, 0.1), 0.08, 0, 0.0), (V(0.75, -0.1, -0.05), 0.07, 0, 2.1), (V(-0.6, -0.65, -0.2), 0.06, 0, 4.0)],
            Body::Rocks => &[(V(-0.74, 0.45, 0.05), 0.12, 1, 0.0), (V(0.72, 0.05, 0.1), 0.1, 1, 2.5), (V(-0.55, -0.7, -0.15), 0.08, 1, 4.4)],
            Body::Pebbles => &[
                (V(-0.62, 0.7, 0.1), 0.05, 1, 0.0),
                (V(-0.78, 0.2, -0.1), 0.06, 1, 1.0),
                (V(-0.6, -0.4, 0.15), 0.045, 1, 2.0),
                (V(0.66, 0.5, -0.05), 0.055, 1, 3.0),
                (V(0.8, -0.15, 0.1), 0.05, 1, 4.0),
                (V(0.6, -0.75, -0.1), 0.06, 1, 5.0),
            ],
            Body::Boulders => &[(V(-0.82, -0.35, -0.05), 0.2, 1, 0.0), (V(0.8, 0.4, -0.15), 0.16, 1, 3.0)],
            Body::Shards => &[(V(-0.7, 0.6, 0.0), 0.1, 2, 0.0), (V(0.75, 0.25, 0.1), 0.09, 2, 1.7), (V(-0.66, -0.45, 0.05), 0.08, 2, 3.4), (V(0.62, -0.7, -0.1), 0.1, 2, 5.1)],
            _ => &[],
        }
    }

    /// Distance in body space.
    fn dist(self, p: V, t: f32) -> f32 {
        let slab = |p: V| rbox(p, V(0.42, 1.05, 0.13), 0.01);
        match self {
            Body::Slab | Body::Lean | Body::LeanBack => slab(p),
            Body::Slanted => slab(p).max((p.1 - 0.35 * p.0 - 0.82) / 1.06),
            Body::Gable => rbox(p, V(0.42, 1.05, 0.16), 0.01).max((p.0.abs() * 0.55 + p.1 - 0.92) / 1.14),
            Body::Taper => {
                let k = (p.1 + 1.05) / 2.1;
                let (w, d) = (0.44 - 0.12 * k, 0.15 - 0.05 * k);
                rbox(V(p.0 * 0.44 / w, p.1, p.2 * 0.15 / d), V(0.44, 1.05, 0.15), 0.005) * (w / 0.44).min(d / 0.15)
            }
            Body::Sloped => {
                let zf = Self::sloped_front(p.1);
                rbox(p - V(0.0, 0.0, -0.08), V(0.42, 1.05, 0.3), 0.01).max((p.2 - zf) / 1.005)
            }
            Body::Twin => slab(p).min(rbox(p - V(-0.3, -0.28, -0.26), V(0.34, 0.77, 0.11), 0.01)),
            Body::Column => rbox(p, V(0.4, 1.05, 0.3), 0.01).max(rbox(p.rot_y(std::f32::consts::FRAC_PI_4), V(0.46, 1.1, 0.46), 0.0)),
            Body::Hex => (p.2.abs().max(0.866 * p.0.abs() / 1.6 + 0.5 * p.2.abs()) - 0.3).max(p.1.abs() - 1.05),
            Body::Board => {
                // apex down at y -1, the base (0.8) at y 0.95
                let (h, b, ya): (f32, f32, f32) = (1.95, 0.8, -1.0);
                let l = (h * h + b * b).sqrt();
                let sx = (p.0.abs() * h - (p.1 - ya) * b) / l;
                let sz = (p.2.abs() * h - (p.1 - ya) * b) / l;
                sx.max(sz).max(p.1 - 0.95)
            }
            Body::Moons | Body::Rocks | Body::Pebbles | Body::Boulders | Body::Shards => {
                let mut d = rbox(p, V(0.34, 0.98, 0.12), 0.01);
                for (k, &(c, r, kind, ph)) in self.sats().iter().enumerate() {
                    // a faint, slow float: 3 cm up and down, a slow turn of its own
                    let c = V(c.0, c.1 + 0.035 * (t * 0.45 + ph).sin(), c.2);
                    let q = (p - c).rot_y(0.3 * (t * 0.2 + ph).sin() + k as f32).rot_x(ph);
                    let s = match kind {
                        0 => rbox(q, V(r, r, r), r * 0.06),
                        1 => lumpy(q, r, ph),
                        _ => lumpy(V(q.0 * 2.2, q.1 * 0.8, q.2 * 1.6), r, ph) / 2.2,
                    };
                    d = d.min(s);
                }
                d
            }
        }
    }

    fn sloped_front(y: f32) -> f32 {
        0.24 - 0.2 * (y + 1.05) / 2.1
    }

    /// Front z of the body (body space) at height Y: where the eyes sit.
    fn front(self, y: f32) -> f32 {
        match self {
            Body::Taper => {
                let k = (y + 1.05) / 2.1;
                0.155 - 0.05 * k
            }
            Body::Sloped => Self::sloped_front(y),
            Body::Gable => 0.17,
            Body::Column => 0.31,
            Body::Hex => 0.3,
            Body::Board => 0.8 * (y + 1.0) / 1.95,
            Body::Moons | Body::Rocks | Body::Pebbles | Body::Boulders | Body::Shards => 0.13,
            _ => 0.14,
        }
    }
}

/// A rock: a sphere of radius R, lumped by 3 waves (phase PH), flatter on top.
fn lumpy(q: V, r: f32, ph: f32) -> f32 {
    let f = 2.6 / r;
    let n = (q.0 * f + ph).sin() * (q.1 * f * 1.3).sin() * (q.2 * f + ph * 2.0).sin() * 0.22 + (q.0 * f * 2.1 - q.2 * f * 1.7 + ph).sin() * 0.07;
    (V(q.0, q.1 * 1.25, q.2).len() - r * (1.0 + n)) * 0.8
}

fn rbox(p: V, b: V, r: f32) -> f32 {
    let q = V(p.0.abs() - b.0, p.1.abs() - b.1, p.2.abs() - b.2);
    V(q.0.max(0.0), q.1.max(0.0), q.2.max(0.0)).len() + q.0.max(q.1).max(q.2).min(0.0) - r
}

pub struct Mono {
    pub body: Body,
    pub pattern: Pattern,
    pub talk: Talk,
}

impl Mono {
    pub fn name(&self) -> String {
        format!("{} · {} · talk: {}", self.body.name(), self.pattern.name(), self.talk.name())
    }

    /// Distance, material.
    pub fn map(&self, p: V, t: f32) -> (f32, u8) {
        let p = self.body.local(p);
        let mut d = self.body.dist(p, t);
        // weathered stone, as the moai: 0 facets, 0 polish, a slow erosion
        d += 0.012 * (p.0 * 7.0 + 1.3).sin() * (p.1 * 5.0).sin() * (p.2 * 8.0 + 0.4).sin() + 0.005 * (p.0 * 19.0 - p.1 * 13.0 + p.2 * 11.0).sin();
        // the eyes: lights on, sockets off
        let s = STATUS.load(Ordering::Relaxed);
        let force = FORCE.load(Ordering::Relaxed);
        let mut e = f32::MAX;
        for (k, c) in self.eyes().into_iter().enumerate() {
            let q = p - c;
            if force == 1 || (force == 0 && lit(self.pattern, self.talk, 1, s, t, k)) {
                e = e.min(q.len() - 0.045);
            } else {
                d = d.max(-(q.len() - 0.05));
            }
        }
        if e < d { (e, EYE) } else { (d, STONE) }
    }
}

impl Mono {
    /// The eye dot centers, body space (world space but for the leaning bodies).
    pub fn eyes(&self) -> Vec<V> {
        let (cy, ex, ey) = self.body.face();
        self.pattern.dots().iter().map(|&(x, y)| {
            let y = cy - y * ey;
            V(x * ex, y, self.body.front(y))
        })
        .collect()
    }
}

/// Body-major: design B with pattern P at B * 3 + P.
pub fn all() -> &'static [Mono] {
    static C: std::sync::OnceLock<Vec<Mono>> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        let mut out = Vec::new();
        for (b, &body) in BODIES.iter().enumerate() {
            for (p, &pattern) in PATTERNS.iter().enumerate() {
                out.push(Mono { body, pattern, talk: TALKS[(b + p * 3) % TALKS.len()] });
            }
        }
        out
    })
}
