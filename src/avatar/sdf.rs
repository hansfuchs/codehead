//! Signed distance fields: the moai head (logo, mascot) and small solids (avatars), a ray
//! marcher, and the light model. Units: the moai head spans y -1.6..1.05.
//!
//! The moai: a long block head, a heavy brow, deep eye sockets, a long nose with flared
//! nostrils, pursed lips, a jutting chin, long ears, a neck stump. Real moai had eyes of white
//! coral: here they glow in the accent (material EYE).

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V(pub f32, pub f32, pub f32);

impl Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        V(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
}
impl Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        V(self.0 - o.0, self.1 - o.1, self.2 - o.2)
    }
}
impl Mul<f32> for V {
    type Output = V;
    fn mul(self, k: f32) -> V {
        V(self.0 * k, self.1 * k, self.2 * k)
    }
}
impl Neg for V {
    type Output = V;
    fn neg(self) -> V {
        V(-self.0, -self.1, -self.2)
    }
}

impl V {
    pub fn dot(self, o: V) -> f32 {
        self.0 * o.0 + self.1 * o.1 + self.2 * o.2
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V {
        let l = self.len().max(1e-6);
        self * (1.0 / l)
    }
    pub fn abs(self) -> V {
        V(self.0.abs(), self.1.abs(), self.2.abs())
    }
    fn max0(self) -> V {
        V(self.0.max(0.0), self.1.max(0.0), self.2.max(0.0))
    }
    fn div(self, o: V) -> V {
        V(self.0 / o.0, self.1 / o.1, self.2 / o.2)
    }
    /// Rotation about the y axis by A radians.
    pub fn rot_y(self, a: f32) -> V {
        let (s, c) = a.sin_cos();
        V(c * self.0 + s * self.2, self.1, -s * self.0 + c * self.2)
    }
    /// Rotation about the x axis by A radians.
    pub fn rot_x(self, a: f32) -> V {
        let (s, c) = a.sin_cos();
        V(self.0, c * self.1 - s * self.2, s * self.1 + c * self.2)
    }
    /// Rotation about the z axis by A radians.
    pub fn rot_z(self, a: f32) -> V {
        let (s, c) = a.sin_cos();
        V(c * self.0 - s * self.1, s * self.0 + c * self.1, self.2)
    }
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}
/// Smooth subtraction: A minus B.
fn ssub(a: f32, b: f32, k: f32) -> f32 {
    -smin(-a, b, k)
}
fn rbox(p: V, b: V, r: f32) -> f32 {
    let q = p.abs() - b;
    q.max0().len() + q.0.max(q.1).max(q.2).min(0.0) - r
}
/// Ellipsoid, approximate (iq).
fn ell(p: V, r: V) -> f32 {
    let k0 = p.div(r).len();
    let k1 = p.div(V(r.0 * r.0, r.1 * r.1, r.2 * r.2)).len();
    k0 * (k0 - 1.0) / k1.max(1e-6)
}
pub const STONE: u8 = 0;
pub const EYE: u8 = 1;

/// The moai, seen from below like a museum visitor: a tall block head, a short forehead
/// leaning back, a brow shelf over 2 dark caves, a long wedge nose with a heavy flared tip, a
/// long upper lip, a thin pursed mouth, a square jaw, a narrow neck, the top of the body.
pub fn moai(p: V) -> (f32, u8) {
    let q = V(p.0, p.1, p.2 + (p.1 - 0.8).max(0.0) * 0.3);
    let m = V(q.0.abs(), q.1, q.2);
    // head block, a little narrower at the top, the top cut on a slant
    let taper = 1.0 - 0.05 * (q.1 + 1.0);
    let mut d = rbox(V(q.0 / taper, q.1, q.2) - V(0.0, 0.12, -0.08), V(0.4, 1.12, 0.48), 0.06) * taper;
    d = d.max(q.dot(V(0.0, 0.95, 0.3)) - 1.25);
    // brow shelf
    d = smin(d, rbox(q - V(0.0, 0.78, 0.42), V(0.41, 0.07, 0.1), 0.04), 0.05);
    // eye caves under the brow
    d = ssub(d, ell(m - V(0.2, 0.6, 0.52), V(0.18, 0.14, 0.24)), 0.03);
    // nose: a wedge from the brow, wider and further out towards the tip
    let nq = (q - V(0.0, 0.38, 0.52)).rot_x(0.38);
    let nw = 0.07 + (0.38 - nq.1).clamp(0.0, 0.76) * 0.17;
    d = smin(d, rbox(V(nq.0 * 0.07 / nw, nq.1, nq.2), V(0.07, 0.38, 0.08), 0.03) * (nw / 0.07).min(1.0), 0.05);
    d = smin(d, ell(q - V(0.0, 0.04, 0.66), V(0.22, 0.12, 0.13)), 0.05);
    d = ssub(d, ell(m - V(0.1, -0.04, 0.7), V(0.06, 0.03, 0.06)), 0.015);
    // upper lip, mouth: thin lips, a slit
    d = smin(d, ell(q - V(0.0, -0.15, 0.42), V(0.32, 0.2, 0.12)), 0.06);
    d = smin(d, ell(q - V(0.0, -0.36, 0.5), V(0.3, 0.07, 0.08)), 0.04);
    d = d.max(-rbox(q - V(0.0, -0.36, 0.6), V(0.28, 0.008, 0.14), 0.0));
    // square jaw
    d = smin(d, rbox(q - V(0.0, -0.7, 0.3), V(0.39, 0.22, 0.16), 0.08), 0.12);
    // long ears
    d = smin(d, rbox(m - V(0.43, 0.45, 0.0), V(0.04, 0.45, 0.12), 0.03), 0.04);
    // narrow neck, then the body, cut off by the frame
    d = d.min(rbox(p - V(0.0, -1.15, -0.08), V(0.34, 0.22, 0.38), 0.1));
    d = smin(d, rbox(p - V(0.0, -1.75, -0.05), V(0.62, 0.42, 0.55), 0.15), 0.06);
    d = d.max(-p.1 - 2.1);
    // coral eyes deep in the caves
    let e = ell(m - V(0.2, 0.6, 0.38), V(0.07, 0.035, 0.03));
    if e < d { (e, EYE) } else { (d, STONE) }
}

/// Square pyramid (iq): base 1x1 on y 0, apex at y H.
fn pyramid(p: V, h: f32) -> f32 {
    let m2 = h * h + 0.25;
    let (mut px, mut pz) = (p.0.abs(), p.2.abs());
    if pz > px {
        std::mem::swap(&mut px, &mut pz);
    }
    px -= 0.5;
    pz -= 0.5;
    let q = V(pz, h * p.1 - 0.5 * px, h * px + 0.5 * p.1);
    let s = (-q.0).max(0.0);
    let t = ((q.1 - 0.5 * pz) / (m2 + 0.25)).clamp(0.0, 1.0);
    let a = m2 * (q.0 + s) * (q.0 + s) + q.1 * q.1;
    let b = m2 * (q.0 + 0.5 * t) * (q.0 + 0.5 * t) + (q.1 - m2 * t) * (q.1 - m2 * t);
    let d2 = if q.1.min(-q.0 * m2 - q.1 * 0.5) > 0.0 { 0.0 } else { a.min(b) };
    ((d2 + q.2 * q.2) / m2).sqrt() * q.2.max(-p.1).signum()
}

/// Hexagonal prism (iq), the hexagon in xz: radius R, half height H.
fn hex_prism(p: V, r: f32, h: f32) -> f32 {
    const K: (f32, f32, f32) = (-0.866_025_4, 0.5, 0.577_350_3);
    let (mut x, mut y) = (p.0.abs(), p.2.abs());
    let d = 2.0 * (K.0 * x + K.1 * y).min(0.0);
    x -= d * K.0;
    y -= d * K.1;
    let dx = (x - x.clamp(-K.2 * r, K.2 * r)).hypot(y - r) * (y - r).signum();
    let dy = p.1.abs() - h;
    dx.max(dy).min(0.0) + dx.max(0.0).hypot(dy.max(0.0))
}

/// Solids: the moai, and the abstract shapes of agent avatars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Moai,
    Pyramid,
    Torus,
    Octa,
    Monolith,
    Pillar,
    Corner,
    /// Entry I of the geo catalog (geo.rs), animated by the view's time.
    Geo(u16),
    /// Entry I of the monoliths (mono.rs).
    Mono(u16),
}

/// The avatar shapes: plain, imposing geometry.
pub const AVATARS: [Shape; 6] = [Shape::Pyramid, Shape::Torus, Shape::Octa, Shape::Monolith, Shape::Pillar, Shape::Corner];

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::Moai => "moai",
            Shape::Pyramid => "inverted pyramid",
            Shape::Torus => "ring",
            Shape::Octa => "octahedron",
            Shape::Monolith => "monolith 1:4:9",
            Shape::Pillar => "hex pillar",
            Shape::Corner => "cube on a vertex",
            Shape::Geo(_) => "geo",
            Shape::Mono(_) => "monolith",
        }
    }

    /// Distance, material.
    pub fn map(self, p: V, t: f32) -> (f32, u8) {
        let d = match self {
            Shape::Moai => return moai(p),
            Shape::Mono(i) => return super::mono::all()[i as usize].map(p, t),
            Shape::Geo(i) => super::geo::all()[i as usize].dist(p, t),
            Shape::Pyramid => {
                // apex down, the base on top
                let s = 1.7;
                pyramid(V(p.0 / s, (0.75 - p.1) / s, p.2 / s), 0.9) * s
            }
            Shape::Torus => {
                let q = (V(p.0, 0.0, p.2).len() - 0.8, p.1);
                (q.0 * q.0 + q.1 * q.1).sqrt() - 0.3
            }
            Shape::Octa => {
                let q = p.abs();
                (q.0 + q.1 + q.2 - 1.15) * 0.577
            }
            Shape::Monolith => rbox(p, V(0.44, 1.0, 0.1), 0.005),
            Shape::Pillar => hex_prism(p, 0.5, 0.95),
            Shape::Corner => rbox(p.rot_x(0.615_5).rot_z(-std::f32::consts::FRAC_PI_4), V(0.6, 0.6, 0.6), 0.005),
        };
        (d, STONE)
    }

    /// Framing: look-at y, half height of the view, camera distance, bounding radius.
    pub fn frame(self) -> (f32, f32, f32, f32) {
        match self {
            Shape::Moai => (0.3, 1.05, 4.5, 2.7),
            Shape::Torus => (0.0, 1.25, 6.0, 1.3),
            Shape::Monolith | Shape::Pillar => (0.0, 1.15, 6.0, 1.3),
            Shape::Corner => (0.0, 1.2, 6.0, 1.2),
            Shape::Geo(_) => (0.0, 1.25, 6.0, 1.8),
            Shape::Mono(_) => (0.0, 1.25, 6.0, 1.7),
            _ => (0.0, 1.2, 6.0, 1.6),
        }
    }
}

/// A sample: lit value 0..1, material, hit depth.
#[derive(Clone, Copy, Debug, Default)]
pub struct Px {
    pub hit: bool,
    pub lum: f32,
    pub mat: u8,
    pub depth: f32,
    /// Surface normal, object space.
    pub n: V,
}

/// View of a shape: rotation, glow of the eyes, light direction.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub shape: Shape,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Eye emission 0..1.
    pub glow: f32,
    /// Key light, world space, normalized.
    pub light: V,
    /// Zoom: 1 frames the whole shape, 2 the half around FOCUS.
    pub zoom: f32,
    /// Look-at y offset, object units.
    pub focus: f32,
    /// Time, seconds: animated shapes.
    pub t: f32,
    /// Rim light on the silhouette: a shine, 0 = matte.
    pub rim: f32,
}

impl View {
    pub fn new(shape: Shape) -> View {
        View { shape, yaw: 0.0, pitch: 0.0, roll: 0.0, glow: 1.0, light: V(-0.5, 0.6, 0.62).norm(), zoom: 1.0, focus: 0.0, t: 0.0, rim: 0.18 }
    }

    fn obj(self, v: V) -> V {
        v.rot_z(-self.roll).rot_x(-self.pitch).rot_y(-self.yaw)
    }

    fn normal(&self, p: V) -> V {
        let e = 0.002;
        let f = |q: V| self.shape.map(q, self.t).0;
        V(f(p + V(e, 0.0, 0.0)) - f(p - V(e, 0.0, 0.0)), f(p + V(0.0, e, 0.0)) - f(p - V(0.0, e, 0.0)), f(p + V(0.0, 0.0, e)) - f(p - V(0.0, 0.0, e))).norm()
    }

    /// Screen point (U, V) of object point P: the inverse of `sample`.
    pub fn project(&self, p: V) -> (f32, f32) {
        let (cy, half, dist, _) = self.shape.frame();
        let k = half / self.zoom / dist;
        let q = p.rot_y(self.yaw).rot_x(self.pitch).rot_z(self.roll) - V(0.0, cy + self.focus, dist);
        (q.0 / -q.2 / k, q.1 / -q.2 / k)
    }

    /// Sample at screen point (U, V): -1..1 on y, x scaled the same (square units).
    pub fn sample(&self, u: f32, v: f32) -> Px {
        let (cy, half, dist, bound) = self.shape.frame();
        let cy = cy + self.focus;
        // perspective camera at DIST, field of view to fit HALF at the origin
        let k = half / self.zoom / dist;
        let ro = self.obj(V(0.0, cy, dist));
        let rd = self.obj(V(u * k, v * k, -1.0).norm());
        // bounding sphere
        let b = ro.dot(rd);
        let c = ro.dot(ro) - bound * bound;
        let disc = b * b - c;
        if disc < 0.0 {
            return Px::default();
        }
        let mut t = (-b - disc.sqrt()).max(0.0);
        let far = -b + disc.sqrt();
        for _ in 0..110 {
            let p = ro + rd * t;
            let (d, mat) = self.shape.map(p, self.t);
            if d < 0.0015 {
                return self.shade(p, rd, t, mat);
            }
            t += d * 0.9;
            if t > far {
                break;
            }
        }
        Px::default()
    }

    fn shade(&self, p: V, rd: V, t: f32, mat: u8) -> Px {
        let n = self.normal(p);
        let l = self.obj(self.light);
        let diff = n.dot(l).max(0.0);
        // ambient occlusion: 4 steps along the normal
        let mut occ = 0.0;
        let mut w = 1.0;
        for i in 1..=4 {
            let h = 0.05 * i as f32;
            occ += (h - self.shape.map(p + n * h, self.t).0) * w;
            w *= 0.6;
        }
        let ao = (1.0 - 3.5 * occ).clamp(0.0, 1.0);
        let rim = (1.0 - n.dot(-rd).max(0.0)).powi(3);
        let lum = if mat == EYE { 0.08 + 0.92 * self.glow } else { ((0.04 + 0.96 * diff * diff.sqrt()) * ao + self.rim * rim).clamp(0.0, 1.0) };
        Px { hit: true, lum, mat, depth: t, n }
    }
}
