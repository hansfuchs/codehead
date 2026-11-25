//! ~100 animated geometric avatars: a base solid (platonic solids, n-gon prisms and pyramids,
//! tori, frames, stars, stacks, cuts) times a motion (spin, tumble, snap, twist, pulse, morph,
//! slice, orbit, wobble). Each fits a sphere of radius ~1.15. Rendered in braille, 7x4 cells.

use super::sdf::Vector;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Base {
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
pub(crate) enum Motion {
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

pub(crate) struct Geo {
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

fn rbox(path: Vector, bytes: Vector, result: f32) -> f32 {
    let query = Vector(path.0.abs() - bytes.0, path.1.abs() - bytes.1, path.2.abs() - bytes.2);

    len2(len2(query.0.max(0.0), query.1.max(0.0)), query.2.max(0.0)) + query.0.max(query.1).max(query.2).min(0.0)
        - result
}

/// Max over a plane set |p·n| - r: a convex solid with those face normals.
fn planes(path: Vector, ns: &[Vector], result: f32) -> f32 {
    ns.iter().map(|count| path.dot(*count).abs()).fold(f32::MIN, f32::max) - result
}

fn normal(x: f32, y: f32, value: f32) -> Vector {
    Vector(x, y, value).norm()
}

/// Polar fold of xz into 1 sector of an n-gon: (distance along the apothem, along the side).
fn ngon(path: Vector, sides: u8) -> (f32, f32) {
    let sector = TAU / f32::from(sides);
    let agent = path.2.atan2(path.0).rem_euclid(sector) - sector / 2.0;
    let line = len2(path.0, path.2);

    (line * agent.cos(), line * agent.sin())
}

/// Distance to the segment A-B.
fn seg(path: Vector, agent: Vector, bytes: Vector) -> f32 {
    let pa = path - agent;
    let ba = bytes - agent;
    let height = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);

    (pa - ba * height).len()
}

fn tetra(path: Vector, result: f32) -> f32 {
    ((path.0 + path.1).abs() - path.2).max((path.0 - path.1).abs() + path.2) / 3f32.sqrt() - result
}

fn torus(path: Vector, big: f32, small: f32) -> f32 {
    len2(len2(path.0, path.2) - big, path.1) - small
}

impl Base {
    pub(crate) fn dist(self, path: Vector) -> f32 {
        match self {
            Base::Tetra => tetra(path, 0.5),
            Base::Cube => rbox(path, Vector(0.62, 0.62, 0.62), 0.02),
            Base::Octa => (path.0.abs() + path.1.abs() + path.2.abs() - 1.1) * 0.577,
            Base::Dodeca => dodecahedron_distance(path),
            Base::Icosa => icosahedron_distance(path),
            Base::Star => tetra(path, 0.42).min(tetra(Vector(-path.0, path.1, -path.2).rot_y(PI / 2.0), 0.42)),
            Base::Prism(text) => prism_distance(path, text),
            Base::Pyramid(text_value, inv) => pyramid_distance(path, text_value, inv),
            Base::Bipyramid(text_2) => bipyramid_distance(path, text_2),
            Base::Torus(key_value) => torus(path, 0.8, key_value),
            Base::SquareTorus => square_torus_distance(path),
            Base::HexTorus => hex_torus_distance(path),
            Base::BoxFrame => box_frame_distance(path),
            Base::OctaFrame => octahedron_frame_distance(path),
            Base::Cross => rbox(path, Vector(1.0, 0.22, 0.22), 0.0).min(rbox(path, Vector(0.22, 1.0, 0.22), 0.0)),
            Base::Jack => rbox(path, Vector(0.95, 0.15, 0.15), 0.02)
                .min(rbox(path, Vector(0.15, 0.95, 0.15), 0.02))
                .min(rbox(path, Vector(0.15, 0.15, 0.95), 0.02)),
            Base::Menger => menger_distance(path),
            Base::Saturn => (path.len() - 0.55).min(torus(path.rot_x(0.4), 0.95, 0.06).max(-(path.len() - 0.7))),
            Base::Stack(key_2) => stack_distance(path, key_2),
            Base::Cylinder => (len2(path.0, path.2) - 0.65).max(path.1.abs() - 0.8),
            Base::Cone => cone_distance(path),
            Base::Lens => {
                (Vector(path.0, path.1 - 0.9, path.2).len() - 1.2).max(Vector(path.0, path.1 + 0.9, path.2).len() - 1.2)
            }
            Base::Capsule => capsule_distance(path),
            Base::Gyroid => gyroid_distance(path),
            Base::Slab => rbox(path, Vector(0.44, 1.0, 0.1), 0.0),
            Base::Steps => steps_distance(path),
            Base::Arch => arch_distance(path),
            Base::Ring2 => torus(path, 0.75, 0.09).min(torus(Vector(path.1, path.0, path.2), 0.75, 0.09)),
        }
    }

    pub(crate) fn name(self) -> String {
        match self {
            Base::Tetra => "tetrahedron".into(),
            Base::Cube => "cube".into(),
            Base::Octa => "octahedron".into(),
            Base::Dodeca => "dodecahedron".into(),
            Base::Icosa => "icosahedron".into(),
            Base::Star => "star tetrahedron".into(),
            Base::Prism(text) => format!("{text}-gon prism"),
            Base::Pyramid(text_value, inv) => format!("{}{text_value}-gon pyramid", if inv { "inverted " } else { "" }),
            Base::Bipyramid(text_2) => format!("{text_2}-gon bipyramid"),
            Base::Torus(key) => format!("ring {key:.2}"),
            Base::SquareTorus => "square ring".into(),
            Base::HexTorus => "hex ring".into(),
            Base::BoxFrame => "cube frame".into(),
            Base::OctaFrame => "octa shell".into(),
            Base::Cross => "cross".into(),
            Base::Jack => "jack".into(),
            Base::Menger => "menger cube".into(),
            Base::Saturn => "saturn".into(),
            Base::Stack(key_value) => format!("{key_value} stacked slabs"),
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

fn pyramid_distance(path: Vector, text_value: u8, inv: bool) -> f32 {
    let y = if inv { -path.1 } else { path.1 } + 0.6;
    let (agent_value, _) = ngon(path, text_value);
    let (height, result) = (1.4, 0.85);

    ((agent_value * height + y * result - result * height) / len2(height, result)).max(-y)
}

fn box_frame_distance(path: Vector) -> f32 {
    let (bytes, error) = (Vector(0.7, 0.7, 0.7), 0.08);
    let path_value = Vector(path.0.abs() - bytes.0, path.1.abs() - bytes.1, path.2.abs() - bytes.2);
    let query_value = Vector(
        (path_value.0 + error).abs() - error,
        (path_value.1 + error).abs() - error,
        (path_value.2 + error).abs() - error,
    );

    let file = |agent_4: Vector| {
        len2(len2(agent_4.0.max(0.0), agent_4.1.max(0.0)), agent_4.2.max(0.0))
            + agent_4.0.max(agent_4.1).max(agent_4.2).min(0.0)
    };

    file(Vector(path_value.0, query_value.1, query_value.2))
        .min(file(Vector(query_value.0, path_value.1, query_value.2)))
        .min(file(Vector(query_value.0, query_value.1, path_value.2)))
}

impl Motion {
    pub(crate) fn name(self) -> &'static str {
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
    pub(crate) fn pose(&self, thread: f32) -> (f32, f32, f32) {
        let thread_value = thread * self.speed + self.phase;

        match self.motion {
            Motion::Spin | Motion::Twist | Motion::Pulse | Motion::Morph | Motion::Slice | Motion::Orbit => {
                (thread_value * 0.9, 0.4, 0.0)
            }
            Motion::Tumble => (thread_value * 0.8, 0.4 + thread_value * 0.55, 0.25),
            Motion::Snap => {
                let key = thread_value * 0.5;
                let step = key.floor() + ease((key.fract() - 0.6) / 0.4);

                (step * PI / 2.0, 0.45, 0.0)
            }
            Motion::Wobble => (0.6 * (thread_value * 1.3).sin(), 0.4, 0.25 * (thread_value * 1.7).sin()),
            Motion::Roll => (0.5, thread_value * 1.2, 0.0),
        }
    }

    /// Distance at P, time T: the base, deformed or cut by the motion.
    pub(crate) fn dist(&self, path: Vector, thread: f32) -> f32 {
        let thread_value = thread * self.speed + self.phase;

        match self.motion {
            Motion::Twist => {
                let key = 1.1 * (thread_value * 0.8).sin();

                self.base.dist(path.rot_y(path.1 * key))
            }
            Motion::Pulse => {
                let text = 1.0 + 0.16 * (thread_value * 2.0).sin();

                self.base.dist(path * (1.0 / text)) * text
            }
            Motion::Morph => {
                let message = ease(0.5 + 0.6 * (thread_value * 0.7).sin());

                self.base.dist(path) * (1.0 - message) + (path.len() - 0.8) * message
            }
            Motion::Slice => {
                // the 2 halves part along y, leaving a gap, then close
                let open = 0.22 * ease(0.5 + 0.7 * (thread_value * 0.9).sin());
                let query = Vector(path.0, path.1 - path.1.signum() * open, path.2);

                self.base.dist(query).max(open - path.1.abs())
            }
            Motion::Orbit => {
                let agent = thread_value * 2.2;
                let o1 = Vector(agent.cos() * 1.05, 0.25 * (agent * 2.0).sin(), agent.sin() * 1.05);
                let small = (path - o1).len().min((path + o1).len()) - 0.12;

                (self.base.dist(path * (1.0 / 0.78)) * 0.78).min(small)
            }
            _ => self.base.dist(path),
        }
    }
}

/// The catalog: every base, each with a few motions, ~100 in all.
pub(crate) fn catalog() -> Vec<Geo> {
    use Base::{
        Arch, Bipyramid, BoxFrame, Capsule, Cone, Cross, Cube, Cylinder, Dodeca, Gyroid, HexTorus, Icosa, Jack, Lens,
        Menger, Octa, OctaFrame, Prism, Pyramid, Ring2, Saturn, Slab, SquareTorus, Stack, Star, Steps, Tetra, Torus,
    };

    use Motion::{Morph, Orbit, Pulse, Roll, Slice, Snap, Spin, Tumble, Twist, Wobble};
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

    motion_catalog(bases)
}

fn motion_catalog(bases: Vec<(Base, &[Motion])>) -> Vec<Geo> {
    let mut out = Vec::new();

    for (base, motions) in bases {
        for message in motions {
            let i = out.len() as f32;

            out.push(Geo {
                name: format!("{} · {}", base.name(), message.name()),
                base,
                motion: *message,
                speed: 0.8 + 0.35 * ((i * 0.618).fract()),
                phase: i * 1.37,
            });
        }
    }

    out
}

/// The catalog, built once.
pub(crate) fn all() -> &'static [Geo] {
    static CATALOG: std::sync::OnceLock<Vec<Geo>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(catalog)
}

fn dodecahedron_distance(path: Vector) -> f32 {
    planes(
        path,
        &[
            normal(0.0, 1.0, PHI),
            normal(0.0, 1.0, -PHI),
            normal(1.0, PHI, 0.0),
            normal(-1.0, PHI, 0.0),
            normal(PHI, 0.0, 1.0),
            normal(-PHI, 0.0, 1.0),
        ],
        0.85,
    )
}
fn icosahedron_distance(path: Vector) -> f32 {
    let key = 1.0 / PHI;

    planes(
        path,
        &[
            normal(1.0, 1.0, 1.0),
            normal(-1.0, 1.0, 1.0),
            normal(1.0, -1.0, 1.0),
            normal(1.0, 1.0, -1.0),
            normal(0.0, key, PHI),
            normal(0.0, key, -PHI),
            normal(key, PHI, 0.0),
            normal(-key, PHI, 0.0),
            normal(PHI, 0.0, key),
            normal(-PHI, 0.0, key),
        ],
        0.88,
    )
}
fn prism_distance(path: Vector, text: u8) -> f32 {
    let (agent, _) = ngon(path, text);

    (agent - 0.7).max(path.1.abs() - 0.75)
}
fn bipyramid_distance(path: Vector, text_2: u8) -> f32 {
    let (agent_2, _) = ngon(path, text_2);
    let (height_value, result_value) = (1.1, 0.8);

    (agent_2 * height_value + path.1.abs() * result_value - result_value * height_value)
        / len2(height_value, result_value)
}
fn square_torus_distance(path: Vector) -> f32 {
    let query = Vector(len2(path.0, path.2) - 0.75, path.1, 0.0);

    rbox(query, Vector(0.22, 0.22, 1.0), 0.02)
        .max(query.0.abs() - 0.22)
        .max(path.1.abs() - 0.22)
}
fn hex_torus_distance(path: Vector) -> f32 {
    let (agent_3, _) = ngon(path, 6);

    rbox(Vector(agent_3 - 0.75, path.1, 0.0), Vector(0.2, 0.2, 1.0), 0.01)
        .max(path.1.abs() - 0.2)
        .max((agent_3 - 0.75).abs() - 0.2)
}
fn octahedron_frame_distance(path: Vector) -> f32 {
    // the 12 edges of an octahedron, folded into the 1st octant: 3 bars
    let query_2 = path.abs();
    let (x, y, value) = (Vector(1.1, 0.0, 0.0), Vector(0.0, 1.1, 0.0), Vector(0.0, 0.0, 1.1));

    seg(query_2, x, y)
        .min(seg(query_2, y, value))
        .min(seg(query_2, value, x))
        - 0.07
}
fn menger_distance(path: Vector) -> f32 {
    let bytes_value = rbox(path, Vector(0.7, 0.7, 0.7), 0.0);
    let height_2 = 0.24;
    let character = rbox(path, Vector(2.0, height_2, height_2), 0.0)
        .min(rbox(path, Vector(height_2, 2.0, height_2), 0.0))
        .min(rbox(path, Vector(height_2, height_2, 2.0), 0.0));

    bytes_value.max(-character)
}
fn stack_distance(path: Vector, key_2: u8) -> f32 {
    let key_3 = f32::from(key_2);
    let step = 1.6 / key_3;
    let y = path.1 + 0.8;
    let i = (y / step).floor().clamp(0.0, key_3 - 1.0);
    let character_value = (i + 0.5) * step - 0.8;
    let width = 0.85 - 0.5 * (i / (key_3 - 1.0).max(1.0));

    rbox(
        Vector(path.0, path.1 - character_value, path.2),
        Vector(width, step * 0.32, width),
        0.02,
    )
}
fn cone_distance(path: Vector) -> f32 {
    let (height_3, result_2) = (1.6, 0.8);
    let y = path.1 + 0.8;

    ((len2(path.0, path.2) * height_3 + y * result_2 - result_2 * height_3) / len2(height_3, result_2)).max(-y)
}
fn capsule_distance(path: Vector) -> f32 {
    let y = path.1.clamp(-0.55, 0.55);

    Vector(path.0, path.1 - y, path.2).len() - 0.45
}
fn gyroid_distance(path: Vector) -> f32 {
    let key_4 = 5.0;
    let group = (path.0 * key_4).sin() * (path.1 * key_4).cos()
        + (path.1 * key_4).sin() * (path.2 * key_4).cos()
        + (path.2 * key_4).sin() * (path.0 * key_4).cos();

    (path.len() - 0.95).max(group.abs() / key_4 - 0.06)
}
fn steps_distance(path: Vector) -> f32 {
    let agent_5 = rbox(Vector(path.0, path.1 + 0.5, path.2), Vector(0.8, 0.25, 0.8), 0.0);
    let bytes_2 = rbox(Vector(path.0, path.1, path.2), Vector(0.55, 0.25, 0.55), 0.0);
    let character_2 = rbox(Vector(path.0, path.1 - 0.5, path.2), Vector(0.3, 0.25, 0.3), 0.0);

    agent_5.min(bytes_2).min(character_2)
}
fn arch_distance(path: Vector) -> f32 {
    let outer = rbox(path, Vector(0.8, 0.9, 0.25), 0.0);
    let hole = (len2(path.0, path.1 + 0.2) - 0.45).min(rbox(
        Vector(path.0, path.1 + 0.6, path.2),
        Vector(0.45, 0.4, 1.0),
        0.0,
    ));

    outer.max(-hole)
}
