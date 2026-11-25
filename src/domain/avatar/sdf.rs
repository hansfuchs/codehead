use std::ops::{Add, ControlFlow, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vector(pub f32, pub f32, pub f32);

impl Add for Vector {
    type Output = Vector;
    fn add(self, other: Vector) -> Vector {
        Vector(self.0 + other.0, self.1 + other.1, self.2 + other.2)
    }
}
impl Sub for Vector {
    type Output = Vector;
    fn sub(self, other: Vector) -> Vector {
        Vector(self.0 - other.0, self.1 - other.1, self.2 - other.2)
    }
}
impl Mul<f32> for Vector {
    type Output = Vector;
    fn mul(self, factor: f32) -> Vector {
        Vector(self.0 * factor, self.1 * factor, self.2 * factor)
    }
}
impl Neg for Vector {
    type Output = Vector;
    fn neg(self) -> Vector {
        Vector(-self.0, -self.1, -self.2)
    }
}

impl Vector {
    pub(crate) fn dot(self, other: Vector) -> f32 {
        self.0 * other.0 + self.1 * other.1 + self.2 * other.2
    }
    pub(crate) fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub(crate) fn norm(self) -> Vector {
        let length = self.len().max(1e-6);

        self * (1.0 / length)
    }
    pub(crate) fn abs(self) -> Vector {
        Vector(self.0.abs(), self.1.abs(), self.2.abs())
    }
    fn max0(self) -> Vector {
        Vector(self.0.max(0.0), self.1.max(0.0), self.2.max(0.0))
    }
    /// Rotation about the y axis by A radians.
    pub(crate) fn rot_y(self, angle: f32) -> Vector {
        let (sin, cos) = angle.sin_cos();

        Vector(cos * self.0 + sin * self.2, self.1, -sin * self.0 + cos * self.2)
    }
    /// Rotation about the x axis by A radians.
    pub(crate) fn rot_x(self, angle: f32) -> Vector {
        let (sin, cos) = angle.sin_cos();

        Vector(self.0, cos * self.1 - sin * self.2, sin * self.1 + cos * self.2)
    }
    /// Rotation about the z axis by A radians.
    pub(crate) fn rot_z(self, angle: f32) -> Vector {
        let (sin, cos) = angle.sin_cos();

        Vector(cos * self.0 - sin * self.1, sin * self.0 + cos * self.1, self.2)
    }
}

fn rbox(point: Vector, half: Vector, radius: f32) -> f32 {
    let offset = point.abs() - half;

    offset.max0().len() + offset.0.max(offset.1).max(offset.2).min(0.0) - radius
}
pub(crate) const STONE: u8 = 0;
pub(crate) const EYE: u8 = 1;

/// Square pyramid (iq): base 1x1 on y 0, apex at y H.
fn pyramid(point: Vector, height: f32) -> f32 {
    let m2 = height * height + 0.25;
    let (mut px, mut pz) = (point.0.abs(), point.2.abs());

    if pz > px {
        std::mem::swap(&mut px, &mut pz);
    }

    px -= 0.5;
    pz -= 0.5;
    let local = Vector(pz, height * point.1 - 0.5 * px, height * px + 0.5 * point.1);
    let outside = (-local.0).max(0.0);
    let along = ((local.1 - 0.5 * pz) / (m2 + 0.25)).clamp(0.0, 1.0);
    let to_edge = m2 * (local.0 + outside) * (local.0 + outside) + local.1 * local.1;
    let to_apex =
        m2 * (local.0 + 0.5 * along) * (local.0 + 0.5 * along) + (local.1 - m2 * along) * (local.1 - m2 * along);

    let d2 = if local.1.min(-local.0 * m2 - local.1 * 0.5) > 0.0 {
        0.0
    } else {
        to_edge.min(to_apex)
    };

    ((d2 + local.2 * local.2) / m2).sqrt() * local.2.max(-point.1).signum()
}

/// Hexagonal prism (iq), the hexagon in xz: radius R, half height H.
fn hex_prism(point: Vector, radius: f32, height: f32) -> f32 {
    const TRIANGLE: (f32, f32, f32) = (-0.866_025_4, 0.5, 0.577_350_3);
    let (mut x, mut y) = (point.0.abs(), point.2.abs());
    let fold = 2.0 * (TRIANGLE.0 * x + TRIANGLE.1 * y).min(0.0);

    x -= fold * TRIANGLE.0;
    y -= fold * TRIANGLE.1;
    let dx = (x - x.clamp(-TRIANGLE.2 * radius, TRIANGLE.2 * radius)).hypot(y - radius) * (y - radius).signum();
    let dy = point.1.abs() - height;

    dx.max(dy).min(0.0) + dx.max(0.0).hypot(dy.max(0.0))
}

/// Solids: the moai, and the abstract shapes of agent avatars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Pyramid,
    Torus,
    Octa,
    Monolith,
    Pillar,
    Corner,
    /// Entry I of the geo catalog (geo.rs), animated by the view's time.
    Geo(u16),
}

/// The avatar shapes: plain, imposing geometry.
pub(crate) const AVATARS: [Shape; 6] = [
    Shape::Pyramid,
    Shape::Torus,
    Shape::Octa,
    Shape::Monolith,
    Shape::Pillar,
    Shape::Corner,
];

impl Shape {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Shape::Pyramid => "inverted pyramid",
            Shape::Torus => "ring",
            Shape::Octa => "octahedron",
            Shape::Monolith => "monolith 1:4:9",
            Shape::Pillar => "hex pillar",
            Shape::Corner => "cube on a vertex",
            Shape::Geo(_) => "geo",
        }
    }

    /// Distance, material.
    pub(crate) fn map(self, point: Vector, time: f32) -> (f32, u8) {
        let distance = match self {
            Shape::Geo(i) => super::geo::all()[i as usize].dist(point, time),
            Shape::Pyramid => {
                // apex down, the base on top
                let scale = 1.7;

                pyramid(Vector(point.0 / scale, (0.75 - point.1) / scale, point.2 / scale), 0.9) * scale
            }
            Shape::Torus => {
                let ring = (Vector(point.0, 0.0, point.2).len() - 0.8, point.1);

                (ring.0 * ring.0 + ring.1 * ring.1).sqrt() - 0.3
            }
            Shape::Octa => {
                let folded = point.abs();

                (folded.0 + folded.1 + folded.2 - 1.15) * 0.577
            }
            Shape::Monolith => rbox(point, Vector(0.44, 1.0, 0.1), 0.005),
            Shape::Pillar => hex_prism(point, 0.5, 0.95),
            Shape::Corner => rbox(
                point.rot_x(0.615_5).rot_z(-std::f32::consts::FRAC_PI_4),
                Vector(0.6, 0.6, 0.6),
                0.005,
            ),
        };

        (distance, STONE)
    }

    /// Framing: look-at y, half height of the view, camera distance, bounding radius.
    pub(crate) fn frame(self) -> (f32, f32, f32, f32) {
        match self {
            Shape::Torus => (0.0, 1.25, 6.0, 1.3),
            Shape::Monolith | Shape::Pillar => (0.0, 1.15, 6.0, 1.3),
            Shape::Corner => (0.0, 1.2, 6.0, 1.2),
            Shape::Geo(_) => (0.0, 1.25, 6.0, 1.8),
            _ => (0.0, 1.2, 6.0, 1.6),
        }
    }
}

/// A sample: lit value 0..1, material, hit depth.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Px {
    pub hit: bool,
    pub lum: f32,
    pub mat: u8,
    pub depth: f32,
    /// Surface normal, object space.
    pub normal: Vector,
}

/// View of a shape: rotation, glow of the eyes, light direction.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    pub shape: Shape,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Eye emission 0..1.
    pub glow: f32,
    /// Key light, world space, normalized.
    pub light: Vector,
    /// Zoom: 1 frames the whole shape, 2 the half around FOCUS.
    pub zoom: f32,
    /// Look-at y offset, object units.
    pub focus: f32,
    /// Time, seconds: animated shapes.
    pub time: f32,
    /// Rim light on the silhouette: a shine, 0 = matte.
    pub rim: f32,
}

impl View {
    pub(crate) fn new(shape: Shape) -> View {
        View {
            shape,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            glow: 1.0,
            light: Vector(-0.5, 0.6, 0.62).norm(),
            zoom: 1.0,
            focus: 0.0,
            time: 0.0,
            rim: 0.18,
        }
    }

    fn obj(self, world: Vector) -> Vector {
        world.rot_z(-self.roll).rot_x(-self.pitch).rot_y(-self.yaw)
    }

    /// Sample at screen point (U, V): -1..1 on y, x scaled the same (square units).
    pub(crate) fn sample(&self, screen_x: f32, screen_y: f32) -> Px {
        let (cy, half, dist, bound) = self.shape.frame();
        let cy = cy + self.focus;

        // perspective camera at DIST, field of view to fit HALF at the origin
        let scale = half / self.zoom / dist;
        let ro = self.obj(Vector(0.0, cy, dist));
        let rd = self.obj(Vector(screen_x * scale, screen_y * scale, -1.0).norm());

        // bounding sphere
        let along = ro.dot(rd);
        let sphere_offset = ro.dot(ro) - bound * bound;
        let discriminant = along * along - sphere_offset;

        if discriminant < 0.0 {
            return Px::default();
        }

        let near = (-along - discriminant.sqrt()).max(0.0);
        let far = -along + discriminant.sqrt();

        if let Some(sample) = trace_surface(ro, rd, near, far, self) {
            return sample;
        }

        Px::default()
    }
}

fn missed_ray(depth: f32, far: f32) -> ControlFlow<()> {
    if depth > far {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn normal(point: Vector, view: &View) -> Vector {
    let step = 0.002;
    let distance = |probe: Vector| view.shape.map(probe, view.time).0;

    Vector(
        distance(point + Vector(step, 0.0, 0.0)) - distance(point - Vector(step, 0.0, 0.0)),
        distance(point + Vector(0.0, step, 0.0)) - distance(point - Vector(0.0, step, 0.0)),
        distance(point + Vector(0.0, 0.0, step)) - distance(point - Vector(0.0, 0.0, step)),
    )
    .norm()
}

fn shade(point: Vector, rd: Vector, depth: f32, mat: u8, view: &View) -> Px {
    let normal = normal(point, view);
    let light = view.obj(view.light);
    let diff = normal.dot(light).max(0.0);

    // ambient occlusion: 4 steps along the normal
    let mut occ = 0.0;
    let mut weight = 1.0;

    for i in 1..=4 {
        let offset = 0.05 * i as f32;

        occ += (offset - view.shape.map(point + normal * offset, view.time).0) * weight;
        weight *= 0.6;
    }

    let ao = (1.0 - 3.5 * occ).clamp(0.0, 1.0);
    let rim = (1.0 - normal.dot(-rd).max(0.0)).powi(3);

    let lum = if mat == EYE {
        0.08 + 0.92 * view.glow
    } else {
        ((0.04 + 0.96 * diff * diff.sqrt()) * ao + view.rim * rim).clamp(0.0, 1.0)
    };

    Px {
        hit: true,
        lum,
        mat,
        depth,
        normal,
    }
}

fn trace_surface(ro: Vector, rd: Vector, mut depth: f32, far: f32, view: &View) -> Option<Px> {
    for _ in 0..110 {
        let point = ro + rd * depth;
        let (distance, mat) = view.shape.map(point, view.time);

        if let Some(sample) = finish_ray_sample(rd, depth, point, distance, mat, view) {
            return Some(sample);
        }

        depth += distance * 0.9;

        if let ControlFlow::Break(()) = missed_ray(depth, far) {
            break;
        }
    }

    None
}

fn finish_ray_sample(rd: Vector, depth: f32, point: Vector, distance: f32, mat: u8, view: &View) -> Option<Px> {
    if distance < 0.0015 {
        return Some(shade(point, rd, depth, mat, view));
    }

    None
}
