//! ~100 animated geometric avatars: a base solid (platonic solids, n-gon prisms and pyramids,
//! tori, frames, stars, stacks, cuts) times a motion (spin, tumble, snap, twist, pulse, morph,
//! slice, orbit, wobble). Each fits a sphere of radius ~1.15. Rendered in braille, 7x4 cells.

use super::sdf::V;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Base {
    Tetra,
    Cube,
    Octa,
    Dodeca,
    Icosa,
    Star,
    Prism(u8),
    Pyramid(u8, bool),
    Bipyramid(u8),
    Torus(f32),
    SquareTorus,
    HexTorus,
    BoxFrame,
    OctaFrame,
    Cross,
    Jack,
    Menger,
    Saturn,
    Stack(u8),
    Cylinder,
    Cone,
    Lens,
    Capsule,
    Gyroid,
    Slab,
    Steps,
    Arch,
    Ring2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Turn about y.
    Spin,
    /// Turn about a tilted axis.
    Tumble,
    /// Quarter turns with a pause between: a mechanism.
    Snap,
    /// Twist about y, back and forth.
    Twist,
    /// Swell and shrink.
    Pulse,
    /// Blend into a sphere and back.
    Morph,
    /// A plane cuts it open and closes it.
    Slice,
    /// 2 small copies circle it.
    Orbit,
    /// Rock side to side.
    Wobble,
    /// Spin about x: a wheel.
    Roll,
}

pub struct Geo {
    pub name: String,
    pub base: Base,
    pub motion: Motion,
    /// Speed and phase, from the index.
    pub speed: f32,
    pub phase: f32,
}

const PHI: f32 = std::f32::consts::GOLDEN_RATIO;

fn len2(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

fn rbox(p: V, b: V, r: f32) -> f32 {
    let q = V(p.0.abs() - b.0, p.1.abs() - b.1, p.2.abs() - b.2);
    len2(len2(q.0.max(0.0), q.1.max(0.0)), q.2.max(0.0)) + q.0.max(q.1).max(q.2).min(0.0) - r
}

/// Max over a plane set |p·n| - r: a convex solid with those face normals.
fn planes(p: V, ns: &[V], r: f32) -> f32 {
    ns.iter().map(|n| p.dot(*n).abs()).fold(f32::MIN, f32::max) - r
}

fn n(x: f32, y: f32, z: f32) -> V {
    V(x, y, z).norm()
}

/// Polar fold of xz into 1 sector of an n-gon: (distance along the apothem, along the side).
fn ngon(p: V, sides: u8) -> (f32, f32) {
    let sector = TAU / sides as f32;
    let a = p.2.atan2(p.0).rem_euclid(sector) - sector / 2.0;
    let l = len2(p.0, p.2);
    (l * a.cos(), l * a.sin())
}

/// Distance to the segment A-B.
fn seg(p: V, a: V, b: V) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);
    (pa - ba * h).len()
}

fn tetra(p: V, r: f32) -> f32 {
    ((p.0 + p.1).abs() - p.2).max((p.0 - p.1).abs() + p.2) / 3f32.sqrt() - r
}

fn torus(p: V, big: f32, small: f32) -> f32 {
    len2(len2(p.0, p.2) - big, p.1) - small
}

impl Base {
    pub fn dist(self, p: V) -> f32 {
        match self {
            Base::Tetra => tetra(p, 0.5),
            Base::Cube => rbox(p, V(0.62, 0.62, 0.62), 0.02),
            Base::Octa => (p.0.abs() + p.1.abs() + p.2.abs() - 1.1) * 0.577,
            Base::Dodeca => planes(p, &[n(0.0, 1.0, PHI), n(0.0, 1.0, -PHI), n(1.0, PHI, 0.0), n(-1.0, PHI, 0.0), n(PHI, 0.0, 1.0), n(-PHI, 0.0, 1.0)], 0.85),
            Base::Icosa => {
                let k = 1.0 / PHI;
                planes(p, &[n(1.0, 1.0, 1.0), n(-1.0, 1.0, 1.0), n(1.0, -1.0, 1.0), n(1.0, 1.0, -1.0), n(0.0, k, PHI), n(0.0, k, -PHI), n(k, PHI, 0.0), n(-k, PHI, 0.0), n(PHI, 0.0, k), n(-PHI, 0.0, k)], 0.88)
            }
            Base::Star => tetra(p, 0.42).min(tetra(V(-p.0, p.1, -p.2).rot_y(PI / 2.0), 0.42)),
            Base::Prism(s) => {
                let (a, _) = ngon(p, s);
                (a - 0.7).max(p.1.abs() - 0.75)
            }
            Base::Pyramid(s, inv) => {
                let y = if inv { -p.1 } else { p.1 } + 0.6;
                let (a, _) = ngon(p, s);
                let (h, r) = (1.4, 0.85);
                ((a * h + y * r - r * h) / len2(h, r)).max(-y)
            }
            Base::Bipyramid(s) => {
                let (a, _) = ngon(p, s);
                let (h, r) = (1.1, 0.8);
                (a * h + p.1.abs() * r - r * h) / len2(h, r)
            }
            Base::Torus(k) => torus(p, 0.8, k),
            Base::SquareTorus => {
                let q = V(len2(p.0, p.2) - 0.75, p.1, 0.0);
                rbox(q, V(0.22, 0.22, 1.0), 0.02).max(q.0.abs() - 0.22).max(p.1.abs() - 0.22)
            }
            Base::HexTorus => {
                let (a, _) = ngon(p, 6);
                rbox(V(a - 0.75, p.1, 0.0), V(0.2, 0.2, 1.0), 0.01).max(p.1.abs() - 0.2).max((a - 0.75).abs() - 0.2)
            }
            Base::BoxFrame => {
                let (b, e) = (V(0.7, 0.7, 0.7), 0.08);
                let p = V(p.0.abs() - b.0, p.1.abs() - b.1, p.2.abs() - b.2);
                let q = V((p.0 + e).abs() - e, (p.1 + e).abs() - e, (p.2 + e).abs() - e);
                let f = |a: V| len2(len2(a.0.max(0.0), a.1.max(0.0)), a.2.max(0.0)) + a.0.max(a.1).max(a.2).min(0.0);
                f(V(p.0, q.1, q.2)).min(f(V(q.0, p.1, q.2))).min(f(V(q.0, q.1, p.2)))
            }
            Base::OctaFrame => {
                // the 12 edges of an octahedron, folded into the 1st octant: 3 bars
                let q = p.abs();
                let (x, y, z) = (V(1.1, 0.0, 0.0), V(0.0, 1.1, 0.0), V(0.0, 0.0, 1.1));
                seg(q, x, y).min(seg(q, y, z)).min(seg(q, z, x)) - 0.07
            }
            Base::Cross => rbox(p, V(1.0, 0.22, 0.22), 0.0).min(rbox(p, V(0.22, 1.0, 0.22), 0.0)),
            Base::Jack => rbox(p, V(0.95, 0.15, 0.15), 0.02).min(rbox(p, V(0.15, 0.95, 0.15), 0.02)).min(rbox(p, V(0.15, 0.15, 0.95), 0.02)),
            Base::Menger => {
                let b = rbox(p, V(0.7, 0.7, 0.7), 0.0);
                let h = 0.24;
                let c = rbox(p, V(2.0, h, h), 0.0).min(rbox(p, V(h, 2.0, h), 0.0)).min(rbox(p, V(h, h, 2.0), 0.0));
                b.max(-c)
            }
            Base::Saturn => (p.len() - 0.55).min(torus(p.rot_x(0.4), 0.95, 0.06).max(-(p.len() - 0.7))),
            Base::Stack(k) => {
                let k = k as f32;
                let step = 1.6 / k;
                let y = p.1 + 0.8;
                let i = (y / step).floor().clamp(0.0, k - 1.0);
                let c = (i + 0.5) * step - 0.8;
                let w = 0.85 - 0.5 * (i / (k - 1.0).max(1.0));
                rbox(V(p.0, p.1 - c, p.2), V(w, step * 0.32, w), 0.02)
            }
            Base::Cylinder => (len2(p.0, p.2) - 0.65).max(p.1.abs() - 0.8),
            Base::Cone => {
                let (h, r) = (1.6, 0.8);
                let y = p.1 + 0.8;
                ((len2(p.0, p.2) * h + y * r - r * h) / len2(h, r)).max(-y)
            }
            Base::Lens => (V(p.0, p.1 - 0.9, p.2).len() - 1.2).max(V(p.0, p.1 + 0.9, p.2).len() - 1.2),
            Base::Capsule => {
                let y = p.1.clamp(-0.55, 0.55);
                V(p.0, p.1 - y, p.2).len() - 0.45
            }
            Base::Gyroid => {
                let k = 5.0;
                let g = (p.0 * k).sin() * (p.1 * k).cos() + (p.1 * k).sin() * (p.2 * k).cos() + (p.2 * k).sin() * (p.0 * k).cos();
                (p.len() - 0.95).max(g.abs() / k - 0.06)
            }
            Base::Slab => rbox(p, V(0.44, 1.0, 0.1), 0.0),
            Base::Steps => {
                let a = rbox(V(p.0, p.1 + 0.5, p.2), V(0.8, 0.25, 0.8), 0.0);
                let b = rbox(V(p.0, p.1, p.2), V(0.55, 0.25, 0.55), 0.0);
                let c = rbox(V(p.0, p.1 - 0.5, p.2), V(0.3, 0.25, 0.3), 0.0);
                a.min(b).min(c)
            }
            Base::Arch => {
                let outer = rbox(p, V(0.8, 0.9, 0.25), 0.0);
                let hole = (len2(p.0, p.1 + 0.2) - 0.45).min(rbox(V(p.0, p.1 + 0.6, p.2), V(0.45, 0.4, 1.0), 0.0));
                outer.max(-hole)
            }
            Base::Ring2 => torus(p, 0.75, 0.09).min(torus(V(p.1, p.0, p.2), 0.75, 0.09)),
        }
    }

    pub fn name(self) -> String {
        match self {
            Base::Tetra => "tetrahedron".into(),
            Base::Cube => "cube".into(),
            Base::Octa => "octahedron".into(),
            Base::Dodeca => "dodecahedron".into(),
            Base::Icosa => "icosahedron".into(),
            Base::Star => "star tetrahedron".into(),
            Base::Prism(s) => format!("{s}-gon prism"),
            Base::Pyramid(s, inv) => format!("{}{s}-gon pyramid", if inv { "inverted " } else { "" }),
            Base::Bipyramid(s) => format!("{s}-gon bipyramid"),
            Base::Torus(k) => format!("ring {:.2}", k),
            Base::SquareTorus => "square ring".into(),
            Base::HexTorus => "hex ring".into(),
            Base::BoxFrame => "cube frame".into(),
            Base::OctaFrame => "octa shell".into(),
            Base::Cross => "cross".into(),
            Base::Jack => "jack".into(),
            Base::Menger => "menger cube".into(),
            Base::Saturn => "saturn".into(),
            Base::Stack(k) => format!("{k} stacked slabs"),
            Base::Cylinder => "cylinder".into(),
            Base::Cone => "cone".into(),
            Base::Lens => "lens".into(),
            Base::Capsule => "capsule".into(),
            Base::Gyroid => "gyroid ball".into(),
            Base::Slab => "monolith".into(),
            Base::Steps => "ziggurat".into(),
            Base::Arch => "gate".into(),
            Base::Ring2 => "gimbal".into(),
        }
    }
}

impl Motion {
    pub fn name(self) -> &'static str {
        match self {
            Motion::Spin => "spin",
            Motion::Tumble => "tumble",
            Motion::Snap => "snap",
            Motion::Twist => "twist",
            Motion::Pulse => "pulse",
            Motion::Morph => "morph",
            Motion::Slice => "slice",
            Motion::Orbit => "orbit",
            Motion::Wobble => "wobble",
            Motion::Roll => "roll",
        }
    }
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Geo {
    /// View rotation at time T: yaw, pitch, roll.
    pub fn pose(&self, t: f32) -> (f32, f32, f32) {
        let t = t * self.speed + self.phase;
        match self.motion {
            Motion::Spin | Motion::Twist | Motion::Pulse | Motion::Morph | Motion::Slice | Motion::Orbit => (t * 0.9, 0.4, 0.0),
            Motion::Tumble => (t * 0.8, 0.4 + t * 0.55, 0.25),
            Motion::Snap => {
                let k = t * 0.5;
                let step = k.floor() + ease((k.fract() - 0.6) / 0.4);
                (step * PI / 2.0, 0.45, 0.0)
            }
            Motion::Wobble => (0.6 * (t * 1.3).sin(), 0.4, 0.25 * (t * 1.7).sin()),
            Motion::Roll => (0.5, t * 1.2, 0.0),
        }
    }

    /// Distance at P, time T: the base, deformed or cut by the motion.
    pub fn dist(&self, p: V, t: f32) -> f32 {
        let t = t * self.speed + self.phase;
        match self.motion {
            Motion::Twist => {
                let k = 1.1 * (t * 0.8).sin();
                self.base.dist(p.rot_y(p.1 * k))
            }
            Motion::Pulse => {
                let s = 1.0 + 0.16 * (t * 2.0).sin();
                self.base.dist(p * (1.0 / s)) * s
            }
            Motion::Morph => {
                let m = ease(0.5 + 0.6 * (t * 0.7).sin());
                self.base.dist(p) * (1.0 - m) + (p.len() - 0.8) * m
            }
            Motion::Slice => {
                // the 2 halves part along y, leaving a gap, then close
                let open = 0.22 * ease(0.5 + 0.7 * (t * 0.9).sin());
                let q = V(p.0, p.1 - p.1.signum() * open, p.2);
                self.base.dist(q).max(open - p.1.abs())
            }
            Motion::Orbit => {
                let a = t * 2.2;
                let o1 = V(a.cos() * 1.05, 0.25 * (a * 2.0).sin(), a.sin() * 1.05);
                let small = (p - o1).len().min((p + o1).len()) - 0.12;
                (self.base.dist(p * (1.0 / 0.78)) * 0.78).min(small)
            }
            _ => self.base.dist(p),
        }
    }
}

/// The catalog: every base, each with a few motions, ~100 in all.
pub fn catalog() -> Vec<Geo> {
    use Base::*;
    use Motion::*;
    let bases: Vec<(Base, &[Motion])> = vec![
        (Tetra, &[Tumble, Snap, Pulse, Orbit]),
        (Cube, &[Tumble, Snap, Twist, Slice]),
        (Octa, &[Spin, Tumble, Morph, Slice]),
        (Dodeca, &[Tumble, Spin, Pulse]),
        (Icosa, &[Tumble, Wobble, Orbit]),
        (Star, &[Tumble, Snap, Pulse, Spin]),
        (Prism(3), &[Spin, Roll, Twist]),
        (Prism(5), &[Tumble, Twist]),
        (Prism(6), &[Spin, Twist, Slice]),
        (Prism(8), &[Roll, Snap]),
        (Pyramid(3, false), &[Spin, Snap]),
        (Pyramid(4, false), &[Spin, Slice, Pulse]),
        (Pyramid(4, true), &[Spin, Wobble, Orbit, Slice]),
        (Pyramid(6, true), &[Spin, Twist]),
        (Bipyramid(3), &[Spin, Tumble]),
        (Bipyramid(4), &[Spin, Twist, Morph]),
        (Bipyramid(6), &[Spin, Wobble]),
        (Torus(0.3), &[Tumble, Roll, Wobble, Morph]),
        (Torus(0.12), &[Tumble, Orbit, Roll]),
        (SquareTorus, &[Tumble, Twist, Snap]),
        (HexTorus, &[Tumble, Spin, Roll]),
        (BoxFrame, &[Tumble, Snap, Twist, Pulse]),
        (OctaFrame, &[Tumble, Spin]),
        (Cross, &[Snap, Roll, Twist]),
        (Jack, &[Tumble, Snap, Orbit]),
        (Menger, &[Tumble, Snap, Slice]),
        (Saturn, &[Wobble, Spin]),
        (Stack(3), &[Twist, Spin, Pulse]),
        (Stack(5), &[Twist, Wobble]),
        (Cylinder, &[Roll, Tumble, Slice]),
        (Cone, &[Wobble, Tumble, Spin]),
        (Lens, &[Roll, Tumble, Pulse]),
        (Capsule, &[Tumble, Roll]),
        (Gyroid, &[Spin, Tumble, Pulse]),
        (Slab, &[Spin, Snap, Wobble]),
        (Steps, &[Spin, Twist, Slice]),
        (Arch, &[Wobble, Spin, Snap]),
        (Ring2, &[Tumble, Spin, Morph]),
    ];
    let mut out = Vec::new();
    for (base, motions) in bases {
        for m in motions {
            let i = out.len() as f32;
            out.push(Geo {
                name: format!("{} · {}", base.name(), m.name()),
                base,
                motion: *m,
                speed: 0.8 + 0.35 * ((i * 0.618).fract()),
                phase: i * 1.37,
            });
        }
    }
    out
}

/// The catalog, built once.
pub fn all() -> &'static [Geo] {
    static C: std::sync::OnceLock<Vec<Geo>> = std::sync::OnceLock::new();
    C.get_or_init(catalog)
}
