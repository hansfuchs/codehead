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

fn rbox(point: Vector, half: Vector, radius: f32) -> f32 {
    let offset = Vector(point.0.abs() - half.0, point.1.abs() - half.1, point.2.abs() - half.2);

    len2(len2(offset.0.max(0.0), offset.1.max(0.0)), offset.2.max(0.0)) + offset.0.max(offset.1).max(offset.2).min(0.0)
        - radius
}

/// Max over a plane set |p·n| - r: a convex solid with those face normals.
fn planes(point: Vector, normals: &[Vector], radius: f32) -> f32 {
    normals
        .iter()
        .map(|face| point.dot(*face).abs())
        .fold(f32::MIN, f32::max)
        - radius
}

fn normal(x: f32, y: f32, depth: f32) -> Vector {
    Vector(x, y, depth).norm()
}

/// Polar fold of xz into 1 sector of an n-gon: (distance along the apothem, along the side).
fn ngon(point: Vector, sides: u8) -> (f32, f32) {
    let sector = TAU / f32::from(sides);
    let angle = point.2.atan2(point.0).rem_euclid(sector) - sector / 2.0;
    let radius = len2(point.0, point.2);

    (radius * angle.cos(), radius * angle.sin())
}

/// Distance to the segment A-B.
fn seg(point: Vector, start: Vector, end: Vector) -> f32 {
    let pa = point - start;
    let ba = end - start;
    let along = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);

    (pa - ba * along).len()
}

fn tetra(point: Vector, radius: f32) -> f32 {
    ((point.0 + point.1).abs() - point.2).max((point.0 - point.1).abs() + point.2) / 3f32.sqrt() - radius
}

fn torus(point: Vector, big: f32, small: f32) -> f32 {
    len2(len2(point.0, point.2) - big, point.1) - small
}

impl Base {
    pub(crate) fn dist(self, point: Vector) -> f32 {
        match self {
            Base::Tetra => tetra(point, 0.5),
            Base::Cube => rbox(point, Vector(0.62, 0.62, 0.62), 0.02),
            Base::Octa => (point.0.abs() + point.1.abs() + point.2.abs() - 1.1) * 0.577,
            Base::Dodeca => dodecahedron_distance(point),
            Base::Icosa => icosahedron_distance(point),
            Base::Star => tetra(point, 0.42).min(tetra(Vector(-point.0, point.1, -point.2).rot_y(PI / 2.0), 0.42)),
            Base::Prism(sides) => prism_distance(point, sides),
            Base::Pyramid(sides, inv) => pyramid_distance(point, sides, inv),
            Base::Bipyramid(sides) => bipyramid_distance(point, sides),
            Base::Torus(thickness) => torus(point, 0.8, thickness),
            Base::SquareTorus => square_torus_distance(point),
            Base::HexTorus => hex_torus_distance(point),
            Base::BoxFrame => box_frame_distance(point),
            Base::OctaFrame => octahedron_frame_distance(point),
            Base::Cross => rbox(point, Vector(1.0, 0.22, 0.22), 0.0).min(rbox(point, Vector(0.22, 1.0, 0.22), 0.0)),
            Base::Jack => rbox(point, Vector(0.95, 0.15, 0.15), 0.02)
                .min(rbox(point, Vector(0.15, 0.95, 0.15), 0.02))
                .min(rbox(point, Vector(0.15, 0.15, 0.95), 0.02)),
            Base::Menger => menger_distance(point),
            Base::Saturn => (point.len() - 0.55).min(torus(point.rot_x(0.4), 0.95, 0.06).max(-(point.len() - 0.7))),
            Base::Stack(slabs) => stack_distance(point, slabs),
            Base::Cylinder => (len2(point.0, point.2) - 0.65).max(point.1.abs() - 0.8),
            Base::Cone => cone_distance(point),
            Base::Lens => (Vector(point.0, point.1 - 0.9, point.2).len() - 1.2)
                .max(Vector(point.0, point.1 + 0.9, point.2).len() - 1.2),
            Base::Capsule => capsule_distance(point),
            Base::Gyroid => gyroid_distance(point),
            Base::Slab => rbox(point, Vector(0.44, 1.0, 0.1), 0.0),
            Base::Steps => steps_distance(point),
            Base::Arch => arch_distance(point),
            Base::Ring2 => torus(point, 0.75, 0.09).min(torus(Vector(point.1, point.0, point.2), 0.75, 0.09)),
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
            Base::Prism(sides) => format!("{sides}-gon prism"),
            Base::Pyramid(sides, inv) => format!("{}{sides}-gon pyramid", if inv { "inverted " } else { "" }),
            Base::Bipyramid(sides) => format!("{sides}-gon bipyramid"),
            Base::Torus(thickness) => format!("ring {thickness:.2}"),
            Base::SquareTorus => "square ring".into(),
            Base::HexTorus => "hex ring".into(),
            Base::BoxFrame => "cube frame".into(),
            Base::OctaFrame => "octa shell".into(),
            Base::Cross => "cross".into(),
            Base::Jack => "jack".into(),
            Base::Menger => "menger cube".into(),
            Base::Saturn => "saturn".into(),
            Base::Stack(slabs) => format!("{slabs} stacked slabs"),
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

fn pyramid_distance(point: Vector, sides: u8, inv: bool) -> f32 {
    let y = if inv { -point.1 } else { point.1 } + 0.6;
    let (apothem, _) = ngon(point, sides);
    let (rise, run) = (1.4, 0.85);

    ((apothem * rise + y * run - run * rise) / len2(rise, run)).max(-y)
}

fn box_frame_distance(point: Vector) -> f32 {
    let (half, edge) = (Vector(0.7, 0.7, 0.7), 0.08);
    let outer = Vector(point.0.abs() - half.0, point.1.abs() - half.1, point.2.abs() - half.2);
    let inner = Vector(
        (outer.0 + edge).abs() - edge,
        (outer.1 + edge).abs() - edge,
        (outer.2 + edge).abs() - edge,
    );

    let box_distance = |offset: Vector| {
        len2(len2(offset.0.max(0.0), offset.1.max(0.0)), offset.2.max(0.0))
            + offset.0.max(offset.1).max(offset.2).min(0.0)
    };

    box_distance(Vector(outer.0, inner.1, inner.2))
        .min(box_distance(Vector(inner.0, outer.1, inner.2)))
        .min(box_distance(Vector(inner.0, inner.1, outer.2)))
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
    pub(crate) fn pose(&self, time: f32) -> (f32, f32, f32) {
        let local = time * self.speed + self.phase;

        match self.motion {
            Motion::Spin | Motion::Twist | Motion::Pulse | Motion::Morph | Motion::Slice | Motion::Orbit => {
                (local * 0.9, 0.4, 0.0)
            }
            Motion::Tumble => (local * 0.8, 0.4 + local * 0.55, 0.25),
            Motion::Snap => {
                let turns = local * 0.5;
                let step = turns.floor() + ease((turns.fract() - 0.6) / 0.4);

                (step * PI / 2.0, 0.45, 0.0)
            }
            Motion::Wobble => (0.6 * (local * 1.3).sin(), 0.4, 0.25 * (local * 1.7).sin()),
            Motion::Roll => (0.5, local * 1.2, 0.0),
        }
    }

    /// Distance at P, time T: the base, deformed or cut by the motion.
    pub(crate) fn dist(&self, point: Vector, time: f32) -> f32 {
        let local = time * self.speed + self.phase;

        match self.motion {
            Motion::Twist => {
                let twist = 1.1 * (local * 0.8).sin();

                self.base.dist(point.rot_y(point.1 * twist))
            }
            Motion::Pulse => {
                let scale = 1.0 + 0.16 * (local * 2.0).sin();

                self.base.dist(point * (1.0 / scale)) * scale
            }
            Motion::Morph => {
                let blend = ease(0.5 + 0.6 * (local * 0.7).sin());

                self.base.dist(point) * (1.0 - blend) + (point.len() - 0.8) * blend
            }
            Motion::Slice => {
                // the 2 halves part along y, leaving a gap, then close
                let open = 0.22 * ease(0.5 + 0.7 * (local * 0.9).sin());
                let parted = Vector(point.0, point.1 - point.1.signum() * open, point.2);

                self.base.dist(parted).max(open - point.1.abs())
            }
            Motion::Orbit => {
                let angle = local * 2.2;
                let moon = Vector(angle.cos() * 1.05, 0.25 * (angle * 2.0).sin(), angle.sin() * 1.05);
                let small = (point - moon).len().min((point + moon).len()) - 0.12;

                (self.base.dist(point * (1.0 / 0.78)) * 0.78).min(small)
            }
            _ => self.base.dist(point),
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
        for motion in motions {
            let i = out.len() as f32;

            out.push(Geo {
                name: format!("{} · {}", base.name(), motion.name()),
                base,
                motion: *motion,
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

fn dodecahedron_distance(point: Vector) -> f32 {
    planes(
        point,
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
fn icosahedron_distance(point: Vector) -> f32 {
    let inverse = 1.0 / PHI;

    planes(
        point,
        &[
            normal(1.0, 1.0, 1.0),
            normal(-1.0, 1.0, 1.0),
            normal(1.0, -1.0, 1.0),
            normal(1.0, 1.0, -1.0),
            normal(0.0, inverse, PHI),
            normal(0.0, inverse, -PHI),
            normal(inverse, PHI, 0.0),
            normal(-inverse, PHI, 0.0),
            normal(PHI, 0.0, inverse),
            normal(-PHI, 0.0, inverse),
        ],
        0.88,
    )
}
fn prism_distance(point: Vector, sides: u8) -> f32 {
    let (apothem, _) = ngon(point, sides);

    (apothem - 0.7).max(point.1.abs() - 0.75)
}
fn bipyramid_distance(point: Vector, sides: u8) -> f32 {
    let (apothem, _) = ngon(point, sides);
    let (rise, run) = (1.1, 0.8);

    (apothem * rise + point.1.abs() * run - run * rise) / len2(rise, run)
}
fn square_torus_distance(point: Vector) -> f32 {
    let folded = Vector(len2(point.0, point.2) - 0.75, point.1, 0.0);

    rbox(folded, Vector(0.22, 0.22, 1.0), 0.02)
        .max(folded.0.abs() - 0.22)
        .max(point.1.abs() - 0.22)
}
fn hex_torus_distance(point: Vector) -> f32 {
    let (apothem, _) = ngon(point, 6);

    rbox(Vector(apothem - 0.75, point.1, 0.0), Vector(0.2, 0.2, 1.0), 0.01)
        .max(point.1.abs() - 0.2)
        .max((apothem - 0.75).abs() - 0.2)
}
fn octahedron_frame_distance(point: Vector) -> f32 {
    // the 12 edges of an octahedron, folded into the 1st octant: 3 bars
    let folded = point.abs();
    let (on_x, on_y, on_z) = (Vector(1.1, 0.0, 0.0), Vector(0.0, 1.1, 0.0), Vector(0.0, 0.0, 1.1));

    seg(folded, on_x, on_y)
        .min(seg(folded, on_y, on_z))
        .min(seg(folded, on_z, on_x))
        - 0.07
}
fn menger_distance(point: Vector) -> f32 {
    let cube = rbox(point, Vector(0.7, 0.7, 0.7), 0.0);
    let bar = 0.24;
    let holes = rbox(point, Vector(2.0, bar, bar), 0.0)
        .min(rbox(point, Vector(bar, 2.0, bar), 0.0))
        .min(rbox(point, Vector(bar, bar, 2.0), 0.0));

    cube.max(-holes)
}
fn stack_distance(point: Vector, slabs: u8) -> f32 {
    let count = f32::from(slabs);
    let step = 1.6 / count;
    let y = point.1 + 0.8;
    let i = (y / step).floor().clamp(0.0, count - 1.0);
    let center = (i + 0.5) * step - 0.8;
    let width = 0.85 - 0.5 * (i / (count - 1.0).max(1.0));

    rbox(
        Vector(point.0, point.1 - center, point.2),
        Vector(width, step * 0.32, width),
        0.02,
    )
}
fn cone_distance(point: Vector) -> f32 {
    let (rise, run) = (1.6, 0.8);
    let y = point.1 + 0.8;

    ((len2(point.0, point.2) * rise + y * run - run * rise) / len2(rise, run)).max(-y)
}
fn capsule_distance(point: Vector) -> f32 {
    let y = point.1.clamp(-0.55, 0.55);

    Vector(point.0, point.1 - y, point.2).len() - 0.45
}
fn gyroid_distance(point: Vector) -> f32 {
    let frequency = 5.0;
    let field = (point.0 * frequency).sin() * (point.1 * frequency).cos()
        + (point.1 * frequency).sin() * (point.2 * frequency).cos()
        + (point.2 * frequency).sin() * (point.0 * frequency).cos();

    (point.len() - 0.95).max(field.abs() / frequency - 0.06)
}
fn steps_distance(point: Vector) -> f32 {
    let bottom = rbox(Vector(point.0, point.1 + 0.5, point.2), Vector(0.8, 0.25, 0.8), 0.0);
    let middle = rbox(Vector(point.0, point.1, point.2), Vector(0.55, 0.25, 0.55), 0.0);
    let top = rbox(Vector(point.0, point.1 - 0.5, point.2), Vector(0.3, 0.25, 0.3), 0.0);

    bottom.min(middle).min(top)
}
fn arch_distance(point: Vector) -> f32 {
    let outer = rbox(point, Vector(0.8, 0.9, 0.25), 0.0);
    let hole = (len2(point.0, point.1 + 0.2) - 0.45).min(rbox(
        Vector(point.0, point.1 + 0.6, point.2),
        Vector(0.45, 0.4, 1.0),
        0.0,
    ));

    outer.max(-hole)
}
