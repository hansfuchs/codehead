use std::ops::{Add, ControlFlow, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vector(pub f32, pub f32, pub f32);

impl Add for Vector {
    type Output = Vector;
    fn add(self, output: Vector) -> Vector {
        Vector(self.0 + output.0, self.1 + output.1, self.2 + output.2)
    }
}
impl Sub for Vector {
    type Output = Vector;
    fn sub(self, output: Vector) -> Vector {
        Vector(self.0 - output.0, self.1 - output.1, self.2 - output.2)
    }
}
impl Mul<f32> for Vector {
    type Output = Vector;
    fn mul(self, key: f32) -> Vector {
        Vector(self.0 * key, self.1 * key, self.2 * key)
    }
}
impl Neg for Vector {
    type Output = Vector;
    fn neg(self) -> Vector {
        Vector(-self.0, -self.1, -self.2)
    }
}

impl Vector {
    pub(crate) fn dot(self, output: Vector) -> f32 {
        self.0 * output.0 + self.1 * output.1 + self.2 * output.2
    }
    pub(crate) fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub(crate) fn norm(self) -> Vector {
        let line = self.len().max(1e-6);

        self * (1.0 / line)
    }
    pub(crate) fn abs(self) -> Vector {
        Vector(self.0.abs(), self.1.abs(), self.2.abs())
    }
    fn max0(self) -> Vector {
        Vector(self.0.max(0.0), self.1.max(0.0), self.2.max(0.0))
    }
    /// Rotation about the y axis by A radians.
    pub(crate) fn rot_y(self, agent: f32) -> Vector {
        let (text, character) = agent.sin_cos();

        Vector(
            character * self.0 + text * self.2,
            self.1,
            -text * self.0 + character * self.2,
        )
    }
    /// Rotation about the x axis by A radians.
    pub(crate) fn rot_x(self, agent: f32) -> Vector {
        let (text, character) = agent.sin_cos();

        Vector(
            self.0,
            character * self.1 - text * self.2,
            text * self.1 + character * self.2,
        )
    }
    /// Rotation about the z axis by A radians.
    pub(crate) fn rot_z(self, agent: f32) -> Vector {
        let (text, character) = agent.sin_cos();

        Vector(
            character * self.0 - text * self.1,
            text * self.0 + character * self.1,
            self.2,
        )
    }
}

fn rbox(path: Vector, bytes: Vector, result: f32) -> f32 {
    let query = path.abs() - bytes;

    query.max0().len() + query.0.max(query.1).max(query.2).min(0.0) - result
}
pub(crate) const STONE: u8 = 0;
pub(crate) const EYE: u8 = 1;

/// Square pyramid (iq): base 1x1 on y 0, apex at y H.
fn pyramid(path: Vector, height: f32) -> f32 {
    let m2 = height * height + 0.25;
    let (mut px, mut pz) = (path.0.abs(), path.2.abs());

    if pz > px {
        std::mem::swap(&mut px, &mut pz);
    }

    px -= 0.5;
    pz -= 0.5;
    let query = Vector(pz, height * path.1 - 0.5 * px, height * px + 0.5 * path.1);
    let text = (-query.0).max(0.0);
    let thread = ((query.1 - 0.5 * pz) / (m2 + 0.25)).clamp(0.0, 1.0);
    let agent = m2 * (query.0 + text) * (query.0 + text) + query.1 * query.1;
    let bytes =
        m2 * (query.0 + 0.5 * thread) * (query.0 + 0.5 * thread) + (query.1 - m2 * thread) * (query.1 - m2 * thread);

    let d2 = if query.1.min(-query.0 * m2 - query.1 * 0.5) > 0.0 {
        0.0
    } else {
        agent.min(bytes)
    };

    ((d2 + query.2 * query.2) / m2).sqrt() * query.2.max(-path.1).signum()
}

/// Hexagonal prism (iq), the hexagon in xz: radius R, half height H.
fn hex_prism(path: Vector, result: f32, height: f32) -> f32 {
    const TRIANGLE: (f32, f32, f32) = (-0.866_025_4, 0.5, 0.577_350_3);
    let (mut x, mut y) = (path.0.abs(), path.2.abs());
    let directory = 2.0 * (TRIANGLE.0 * x + TRIANGLE.1 * y).min(0.0);

    x -= directory * TRIANGLE.0;
    y -= directory * TRIANGLE.1;
    let dx = (x - x.clamp(-TRIANGLE.2 * result, TRIANGLE.2 * result)).hypot(y - result) * (y - result).signum();
    let dy = path.1.abs() - height;

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
    pub(crate) fn map(self, path: Vector, thread: f32) -> (f32, u8) {
        let directory = match self {
            Shape::Geo(i) => super::geo::all()[i as usize].dist(path, thread),
            Shape::Pyramid => {
                // apex down, the base on top
                let text = 1.7;

                pyramid(Vector(path.0 / text, (0.75 - path.1) / text, path.2 / text), 0.9) * text
            }
            Shape::Torus => {
                let query = (Vector(path.0, 0.0, path.2).len() - 0.8, path.1);

                (query.0 * query.0 + query.1 * query.1).sqrt() - 0.3
            }
            Shape::Octa => {
                let query_value = path.abs();

                (query_value.0 + query_value.1 + query_value.2 - 1.15) * 0.577
            }
            Shape::Monolith => rbox(path, Vector(0.44, 1.0, 0.1), 0.005),
            Shape::Pillar => hex_prism(path, 0.5, 0.95),
            Shape::Corner => rbox(
                path.rot_x(0.615_5).rot_z(-std::f32::consts::FRAC_PI_4),
                Vector(0.6, 0.6, 0.6),
                0.005,
            ),
        };

        (directory, STONE)
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

    fn obj(self, value: Vector) -> Vector {
        value.rot_z(-self.roll).rot_x(-self.pitch).rot_y(-self.yaw)
    }

    /// Sample at screen point (U, V): -1..1 on y, x scaled the same (square units).
    pub(crate) fn sample(&self, user: f32, value: f32) -> Px {
        let (cy, half, dist, bound) = self.shape.frame();
        let cy = cy + self.focus;

        // perspective camera at DIST, field of view to fit HALF at the origin
        let key = half / self.zoom / dist;
        let ro = self.obj(Vector(0.0, cy, dist));
        let rd = self.obj(Vector(user * key, value * key, -1.0).norm());

        // bounding sphere
        let bytes = ro.dot(rd);
        let sphere_offset = ro.dot(ro) - bound * bound;
        let discriminant = bytes * bytes - sphere_offset;

        if discriminant < 0.0 {
            return Px::default();
        }

        let thread = (-bytes - discriminant.sqrt()).max(0.0);
        let far = -bytes + discriminant.sqrt();

        if let Some(value) = trace_surface(ro, rd, thread, far, self) {
            return value;
        }

        Px::default()
    }
}

fn missed_ray(thread: f32, far: f32) -> ControlFlow<()> {
    if thread > far {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn normal(path: Vector, model_state: &View) -> Vector {
    let error = 0.002;
    let file = |query: Vector| model_state.shape.map(query, model_state.time).0;

    Vector(
        file(path + Vector(error, 0.0, 0.0)) - file(path - Vector(error, 0.0, 0.0)),
        file(path + Vector(0.0, error, 0.0)) - file(path - Vector(0.0, error, 0.0)),
        file(path + Vector(0.0, 0.0, error)) - file(path - Vector(0.0, 0.0, error)),
    )
    .norm()
}

fn shade(path: Vector, rd: Vector, thread: f32, mat: u8, model_state: &View) -> Px {
    let count = normal(path, model_state);
    let line = model_state.obj(model_state.light);
    let diff = count.dot(line).max(0.0);

    // ambient occlusion: 4 steps along the normal
    let mut occ = 0.0;
    let mut width = 1.0;

    for i in 1..=4 {
        let height = 0.05 * i as f32;

        occ += (height - model_state.shape.map(path + count * height, model_state.time).0) * width;
        width *= 0.6;
    }

    let ao = (1.0 - 3.5 * occ).clamp(0.0, 1.0);
    let rim = (1.0 - count.dot(-rd).max(0.0)).powi(3);

    let lum = if mat == EYE {
        0.08 + 0.92 * model_state.glow
    } else {
        ((0.04 + 0.96 * diff * diff.sqrt()) * ao + model_state.rim * rim).clamp(0.0, 1.0)
    };

    Px {
        hit: true,
        lum,
        mat,
        depth: thread,
        normal: count,
    }
}

fn trace_surface(ro: Vector, rd: Vector, mut thread: f32, far: f32, model_state: &View) -> Option<Px> {
    for _ in 0..110 {
        let path = ro + rd * thread;
        let (directory, mat) = model_state.shape.map(path, model_state.time);

        if let Some(value) = finish_ray_sample(rd, thread, path, directory, mat, model_state) {
            return Some(value);
        }

        thread += directory * 0.9;

        if let ControlFlow::Break(()) = missed_ray(thread, far) {
            break;
        }
    }

    None
}

fn finish_ray_sample(rd: Vector, thread: f32, path: Vector, directory: f32, mat: u8, model_state: &View) -> Option<Px> {
    if directory < 0.0015 {
        return Some(shade(path, rd, thread, mat, model_state));
    }

    None
}
