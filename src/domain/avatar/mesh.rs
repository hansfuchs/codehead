use super::{Mesh, P3, Solid, cross, sub};
use std::f32::consts::{FRAC_PI_2, PI};

impl Mesh {
    /// The solid between the outlines FRONT and BACK (same count, ccw seen from the front).
    pub(crate) fn loft(front: &[P3], back: &[P3]) -> Mesh {
        let count = front.len();
        let mut verts = front.to_vec();

        verts.extend_from_slice(back);
        let mut faces = vec![
            (0..count).collect::<Vec<_>>(),
            (0..count).rev().map(|i| i + count).collect(),
        ];

        for i in 0..count {
            let j = (i + 1) % count;

            faces.push(vec![j, i, i + count, j + count]);
        }

        Mesh { verts, faces }
    }
    /// Prism of the outline OUT, depth D; the back outline scaled by BACK, shifted by OFF,
    /// turned by TURN (rad).
    pub(crate) fn prism(
        outline: impl IntoIterator<Item = (f32, f32)>,
        depth: f32,
        back: f32,
        offset: (f32, f32),
        turn: f32,
    ) -> Mesh {
        let (sine, cosine) = turn.sin_cos();
        let points = outline.into_iter();
        let capacity = points.size_hint().0;
        let mut front = Vec::with_capacity(capacity);
        let mut rear = Vec::with_capacity(capacity);

        for (x, y) in points {
            front.push((x, y, depth / 2.0));
            rear.push((
                (cosine * x - sine * y) * back + offset.0,
                (sine * x + cosine * y) * back + offset.1,
                -depth / 2.0,
            ));
        }

        Mesh::loft(&front, &rear)
    }
    /// Z on the plane of the front face at (X, Y).
    pub(crate) fn front_z(&self, x: f32, y: f32) -> f32 {
        let face = &self.faces[0];
        let (first, second, third) = (self.verts[face[0]], self.verts[face[1]], self.verts[face[2]]);
        let normal = cross(sub(second, first), sub(third, first));

        if normal.2.abs() < 1e-4 {
            first.2
        } else {
            first.2 - (normal.0 * (x - first.0) + normal.1 * (y - first.1)) / normal.2
        }
    }
}
pub(crate) fn ngon(sides: usize, rx: f32, ry: f32, rot: f32) -> Vec<(f32, f32)> {
    (0..sides)
        .map(|i| {
            let angle = rot + std::f32::consts::TAU * i as f32 / sides as f32;

            (rx * angle.cos(), ry * angle.sin())
        })
        .collect()
}
/// Half circle of radius R around (0, CY) from angle A0, ccw, N steps.
pub(crate) fn arc(radius: f32, cy: f32, a0: f32, steps: usize) -> impl Iterator<Item = (f32, f32)> {
    (0..=steps).map(move |i| {
        let angle = a0 + std::f32::consts::PI * i as f32 / steps as f32;

        (radius * angle.cos(), cy + radius * angle.sin())
    })
}
pub(crate) fn rect(width: f32, height: f32) -> Vec<(f32, f32)> {
    vec![(width, height), (-width, height), (-width, -height), (width, -height)]
}
impl Solid {
    pub(crate) fn name(self) -> &'static str {
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
            Solid::Star4 => "4-point star",
            Solid::Ellipse => "ellipse",
            Solid::WideTrapezoid => "wide trapezoid",
        }
    }
    /// The mesh, the eye scale (x, y) in dots, the eye center y.
    pub(crate) fn mesh(self) -> (Mesh, (f32, f32), f32) {
        match self {
            Solid::Cube => (prism_mesh(rect(5.5, 5.5), 6.0, 1.0), DEFAULT_EYES, 0.0),
            Solid::Slab => (prism_mesh(rect(6.0, 4.5), 3.0, 1.0), DEFAULT_EYES, 0.0),
            Solid::Tall => (prism_mesh(rect(5.0, 6.5), 2.5, 1.0), (4.0, 2.5), 1.0),
            Solid::Monolith => (prism_mesh(rect(5.0, 6.5), 1.5, 1.0), DEFAULT_EYES, 1.5),
            Solid::Obelisk => obelisk_mesh(),
            Solid::Stele => stele_mesh(),
            Solid::Trapezoid => trapezoid_mesh(),
            Solid::Screen => (prism_mesh(rect(6.0, 5.0), 5.0, 0.55), DEFAULT_EYES, 0.0),
            Solid::Wedge => wedge_mesh(prism_mesh, DEFAULT_EYES),
            Solid::Arch => arch_mesh(),
            Solid::Capsule => capsule_mesh(),
            Solid::Shield => shield_mesh(),
            Solid::Lozenge => lozenge_mesh(),
            Solid::Diamond => diamond_mesh(),
            Solid::Pentagon => (prism_mesh(ngon(5, 7.2, 7.2, FRAC_PI_2), 2.5, 1.0), DEFAULT_EYES, -0.5),
            Solid::Hex => (prism_mesh(ngon(6, 6.6, 6.6, 0.0), 3.0, 1.0), DEFAULT_EYES, 0.0),
            Solid::Heptagon => (prism_mesh(ngon(7, 6.8, 6.8, FRAC_PI_2), 2.5, 1.0), DEFAULT_EYES, 0.0),
            Solid::Oct => (prism_mesh(ngon(8, 6.4, 6.4, PI / 8.0), 3.0, 1.0), DEFAULT_EYES, 0.0),
            Solid::Disk => (prism_mesh(ngon(16, 6.4, 6.4, 0.0), 2.5, 1.0), DEFAULT_EYES, 0.0),
            Solid::TrapPrism | Solid::TrapPrismInv => self.trapezoid_prism_mesh(DEFAULT_EYES),
            Solid::Skew => (
                Mesh::prism(rect(5.0, 5.0), 4.0, 1.0, (2.4, 1.8), 0.0),
                DEFAULT_EYES,
                0.0,
            ),
            Solid::Notch => notch_mesh(),
            Solid::HexFrustum => (prism_mesh(ngon(6, 6.8, 6.8, 0.0), 4.0, 0.55), DEFAULT_EYES, 0.0),
            Solid::Pyramid => (prism_mesh(rect(6.0, 6.0), 6.0, 0.03), DEFAULT_EYES, 0.0),
            Solid::CutBox => cut_box_mesh(),
            Solid::Twist => (
                Mesh::prism(rect(5.5, 5.5), 5.0, 0.95, (0.0, 0.0), 0.45),
                DEFAULT_EYES,
                0.0,
            ),
            Solid::Rhomb => rhomb_mesh(),
            Solid::Crystal => crystal_mesh(),
            Solid::Kite => kite_mesh(),
            Solid::Gem => (prism_mesh(ngon(8, 5.4, 5.4, PI / 8.0), 3.0, 1.3), DEFAULT_EYES, 0.0),
            Solid::Star4 => star_mesh(prism_mesh),
            Solid::Ellipse => (prism_mesh(ngon(20, 7.0, 5.2, 0.0), 3.0, 1.0), DEFAULT_EYES, 0.0),
            Solid::WideTrapezoid => wide_trapezoid_mesh(),
        }
    }

    fn trapezoid_prism_mesh(self, eyes: (f32, f32)) -> (Mesh, (f32, f32), f32) {
        // the front leans: its top (or bottom) set back
        let (zt, zb) = if self == Solid::TrapPrism {
            (0.0, 2.8)
        } else {
            (2.8, 0.0)
        };

        let front = [(5.5, 5.5, zt), (-5.5, 5.5, zt), (-5.5, -5.5, zb), (5.5, -5.5, zb)];
        let back = [
            (5.5, 5.5, -2.8),
            (-5.5, 5.5, -2.8),
            (-5.5, -5.5, -2.8),
            (5.5, -5.5, -2.8),
        ];

        (Mesh::loft(&front, &back), eyes, 0.0)
    }
}

fn wedge_mesh(prism: impl Fn(Vec<(f32, f32)>, f32, f32) -> Mesh, eyes: (f32, f32)) -> (Mesh, (f32, f32), f32) {
    let mut mesh = prism(rect(5.5, 5.5), 6.0, 1.0);

    // the top of the back edge pulled down: a wedge leaning back
    for vertex in mesh.verts.iter_mut().skip(4).take(2) {
        vertex.1 -= 4.0;
    }

    (mesh, eyes, 0.0)
}

fn star_mesh(prism: impl Fn(Vec<(f32, f32)>, f32, f32) -> Mesh) -> (Mesh, (f32, f32), f32) {
    // 4 points (up, down, left, right), the inner corners on the diagonals; the
    // loop starts on an inner corner: the 2nd vertex convex (the front normal +z)
    let outline: Vec<(f32, f32)> = (0..8)
        .map(|i| {
            let angle = -PI / 4.0 + PI / 4.0 * i as f32;
            let (rx, ry) = if i % 2 == 1 { (6.8, 7.6) } else { (3.9, 3.9) };

            (rx * angle.cos(), ry * angle.sin())
        })
        .collect();

    (prism(outline, 1.8, 1.0), (2.0, 1.5), 0.0)
}

const DEFAULT_EYES: (f32, f32) = (4.0, 2.0);
fn prism_mesh(outline: Vec<(f32, f32)>, depth: f32, back: f32) -> Mesh {
    Mesh::prism(outline, depth, back, (0.0, 0.0), 0.0)
}

fn obelisk_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(
            vec![(4.6, -6.8), (4.6, 3.2), (0.0, 7.2), (-4.6, 3.2), (-4.6, -6.8)],
            3.0,
            1.0,
        ),
        (3.5, 2.0),
        0.0,
    )
}
fn stele_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(vec![(5.5, -6.5), (5.5, 4.5), (-5.5, 6.5), (-5.5, -6.5)], 2.5, 1.0),
        DEFAULT_EYES,
        0.5,
    )
}
fn trapezoid_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(vec![(6.5, -6.5), (4.2, 6.0), (-4.2, 6.0), (-6.5, -6.5)], 3.0, 1.0),
        DEFAULT_EYES,
        0.0,
    )
}
fn arch_mesh() -> (Mesh, (f32, f32), f32) {
    let mut outline = vec![(6.0, -6.0)];

    outline.extend(arc(6.0, 1.0, 0.0, 8));
    outline.push((-6.0, -6.0));
    (prism_mesh(outline, 3.0, 1.0), DEFAULT_EYES, 0.0)
}
fn capsule_mesh() -> (Mesh, (f32, f32), f32) {
    let mut outline: Vec<(f32, f32)> = arc(5.6, 1.4, 0.0, 8).collect();

    outline.extend(arc(5.6, -1.4, PI, 8));
    (prism_mesh(outline, 2.5, 1.0), DEFAULT_EYES, 0.0)
}
fn shield_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(
            vec![(6.0, 5.5), (-6.0, 5.5), (-6.0, -0.5), (0.0, -7.0), (6.0, -0.5)],
            2.5,
            1.0,
        ),
        DEFAULT_EYES,
        1.5,
    )
}
fn lozenge_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(
            vec![
                (0.0, 7.5),
                (-5.6, 3.5),
                (-5.6, -3.5),
                (0.0, -7.5),
                (5.6, -3.5),
                (5.6, 3.5),
            ],
            2.5,
            1.0,
        ),
        DEFAULT_EYES,
        0.0,
    )
}
fn diamond_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(vec![(7.0, 0.0), (0.0, 7.5), (-7.0, 0.0), (0.0, -7.5)], 3.0, 1.0),
        (3.5, 2.0),
        0.0,
    )
}
fn notch_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(
            vec![
                (5.5, 5.5),
                (-5.5, 5.5),
                (-5.5, -5.5),
                (1.5, -5.5),
                (1.5, -2.0),
                (5.5, -2.0),
            ],
            4.0,
            1.0,
        ),
        DEFAULT_EYES,
        1.5,
    )
}
fn cut_box_mesh() -> (Mesh, (f32, f32), f32) {
    let front: Vec<P3> = [(5.5, 1.5), (1.5, 5.5), (-5.5, 5.5), (-5.5, -5.5), (5.5, -5.5)]
        .iter()
        .map(|&(x, y)| (x, y, 2.8))
        .collect();
    let back: Vec<P3> = [(5.5, 4.8), (4.8, 5.5), (-5.5, 5.5), (-5.5, -5.5), (5.5, -5.5)]
        .iter()
        .map(|&(x, y)| (x, y, -2.8))
        .collect();

    (Mesh::loft(&front, &back), DEFAULT_EYES, -0.5)
}
fn rhomb_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(vec![(6.8, 5.0), (-3.2, 5.0), (-6.8, -5.0), (3.2, -5.0)], 3.0, 1.0),
        DEFAULT_EYES,
        0.0,
    )
}
fn crystal_mesh() -> (Mesh, (f32, f32), f32) {
    let outline = vec![
        (6.2, 1.0),
        (3.0, 6.8),
        (-3.6, 5.8),
        (-6.6, -0.5),
        (-3.0, -6.8),
        (3.6, -5.8),
    ];

    (Mesh::prism(outline, 4.0, 0.7, (1.2, -0.8), 0.25), DEFAULT_EYES, 0.0)
}
fn kite_mesh() -> (Mesh, (f32, f32), f32) {
    (
        Mesh::prism(
            [(0.0, 7.6), (-6.4, 1.5), (0.0, -7.6), (6.4, 1.5)],
            4.0,
            0.55,
            (0.0, 0.8),
            0.0,
        ),
        (3.5, 2.0),
        0.8,
    )
}
fn wide_trapezoid_mesh() -> (Mesh, (f32, f32), f32) {
    (
        prism_mesh(vec![(7.0, -4.8), (3.6, 4.8), (-3.6, 4.8), (-7.0, -4.8)], 3.0, 1.0),
        DEFAULT_EYES,
        1.0,
    )
}
