use super::{DH, DW, Fit, Mesh, Motion, P3};

pub(crate) fn rot(point: P3, yaw: f32, pitch: f32, roll: f32) -> P3 {
    let (sin, cos) = yaw.sin_cos();
    let (x, y, depth) = (cos * point.0 + sin * point.2, point.1, -sin * point.0 + cos * point.2);
    let (pitch_sin, pitch_cos) = pitch.sin_cos();
    let (y, pitched_depth) = (pitch_cos * y - pitch_sin * depth, pitch_sin * y + pitch_cos * depth);
    let (roll_sin, roll_cos) = roll.sin_cos();

    (roll_cos * x - roll_sin * y, roll_sin * x + roll_cos * y, pitched_depth)
}
pub(crate) fn sub(left: P3, right: P3) -> P3 {
    (left.0 - right.0, left.1 - right.1, left.2 - right.2)
}
pub(crate) fn cross(left: P3, right: P3) -> P3 {
    (
        left.1 * right.2 - left.2 * right.1,
        left.2 * right.0 - left.0 * right.2,
        left.0 * right.1 - left.1 * right.0,
    )
}
pub(crate) fn unit(vector: P3) -> P3 {
    let length = (vector.0 * vector.0 + vector.1 * vector.1 + vector.2 * vector.2)
        .sqrt()
        .max(1e-6);

    (vector.0 / length, vector.1 / length, vector.2 / length)
}
/// The pose of MOTION at T, slowed or stopped by status S: pitch, roll, bob (dots).
pub(crate) fn pose(motion: Motion, status: usize, time: f32) -> (f32, f32, f32) {
    let local = match status {
        0 => time,
        1 => time * 0.6,
        2 => time * 0.3,
        _ => 1.3,
    };

    match motion {
        Motion::Nod => (0.3 + 0.25 * (local * 1.6).sin(), 0.0, 0.0),
        Motion::Float => (0.25, 0.0, 0.9 * (local * 1.8).sin()),
        Motion::Hover => (0.25 + 0.06 * (local * 0.9 + 1.0).sin(), 0.0, 1.5 * (local * 0.9).sin()),
        Motion::Drift => (0.25, 0.07 * (local * 0.8).sin(), 0.7 * (local * 1.3).sin()),
        Motion::Bob => (0.25, 0.0, 0.45 * (local * 3.6).sin()),
    }
}
/// Perspective: a camera 40 dots away. P relative to the canvas center, y up → (x, y) right, down.
pub(crate) fn persp(point: P3, bob: f32) -> (f32, f32) {
    let scale = 40.0 / (40.0 - point.2);

    (point.0 * scale, -(point.1 + bob) * scale)
}
/// The fit of MESH in MOTION at turn YAW: every vertex over 1 whole motion (pitch, roll, bob,
/// the blocked shake) inside the canvas, the swept area centered; a smaller scale only when that
/// area is larger than the canvas. 0 dots cut off at the canvas edge while it floats.
pub(crate) fn fit(mesh: &Mesh, motion: Motion, yaw: f32) -> Fit {
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);

    let mut take = |yaw: f32, (pitch, roll, bob): (f32, f32, f32)| {
        for &vertex in &mesh.verts {
            let (x, y) = persp(rot(vertex, yaw, pitch, roll), bob);

            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
    };

    // 32 s in 1/8 s steps: every motion's period (max. 2π / 0.8 ≈ 7.9 s) several times
    for i in 0..256 {
        take(yaw, pose(motion, 0, i as f32 / 8.0));
    }

    // blocked: its fixed pose, the shake ±0.1 rad
    for shake in [-0.1, 0.0, 0.1] {
        take(yaw + shake, pose(motion, 3, 0.0));
    }

    let (width, height) = ((DW - 1) as f32, (DH - 1) as f32);
    let scale = (width / (x1 - x0).max(1e-3)).min(height / (y1 - y0).max(1e-3)).min(1.0);

    Fit {
        scale,
        cx: width / 2.0 - f32::midpoint(x0, x1) * scale,
        cy: height / 2.0 - f32::midpoint(y0, y1) * scale,
    }
}
/// Distance of P to the segment A B.
pub(crate) fn seg_dist(point: (f32, f32), start: (f32, f32), end: (f32, f32)) -> f32 {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length_squared = dx * dx + dy * dy;

    let along = if length_squared < 1e-6 {
        0.0
    } else {
        (((point.0 - start.0) * dx + (point.1 - start.1) * dy) / length_squared).clamp(0.0, 1.0)
    };

    (point.0 - start.0 - dx * along).hypot(point.1 - start.1 - dy * along)
}
/// [`draw3d_in`] and the line coverage of each line dot (1 elsewhere). The lines: a 1-dot-wide
/// strip along each float segment; a dot on where the strip covers 6/16 of its square (4x4
/// samples). Every dot follows the float line on its own, 0 snap of the ends to whole dots: a
/// move by less than 1 dot flips single dots (fluid), never the whole outline at once.
pub(crate) fn draw3d_cov(
    mesh: &Mesh,
    wires: &[(P3, P3)],
    anchor: (f32, f32),
    marks: &[(i32, i32, u8)],
    (yaw, pitch, roll): (f32, f32, f32),
    bob: f32,
    fit: Fit,
) -> (Vec<u8>, Vec<f32>) {
    let vr: Vec<P3> = mesh.verts.iter().map(|&vertex| rot(vertex, yaw, pitch, roll)).collect();

    let proj = |vertex: P3| -> (f32, f32) {
        let (x, y) = persp(vertex, bob);

        (fit.cx + x * fit.scale, fit.cy + y * fit.scale)
    };

    let normal = |face: &[usize]| unit(cross(sub(vr[face[1]], vr[face[0]]), sub(vr[face[2]], vr[face[0]])));
    let norms: Vec<P3> = mesh.faces.iter().map(|face| normal(face)).collect();
    let vis: Vec<bool> = norms.iter().map(|norm| norm.2 > 0.0).collect();

    // the segments in view: the silhouette, the creases, the wires
    let mut segs: Vec<((f32, f32), (f32, f32))> = Vec::new();
    let mut edges: Vec<(usize, usize, usize)> = Vec::new();

    for (fi, face) in mesh.faces.iter().enumerate() {
        for i in 0..face.len() {
            edges.push((
                face[i].min(face[(i + 1) % face.len()]),
                face[i].max(face[(i + 1) % face.len()]),
                fi,
            ));
        }
    }

    edges.sort_unstable();

    for pair in edges.chunks(2) {
        let [(start, end, fa), (_, _, fb)] = [pair[0], *pair.get(1).unwrap_or(&pair[0])];
        let (na, nb) = (norms[fa], norms[fb]);
        let crease = na.0 * nb.0 + na.1 * nb.1 + na.2 * nb.2 < 0.8;

        if !(vis[fa] || vis[fb]) || (vis[fa] && vis[fb] && !crease) {
            continue;
        }

        segs.push((proj(vr[start]), proj(vr[end])));
    }

    for &(start, end) in wires {
        segs.push((proj(rot(start, yaw, pitch, roll)), proj(rot(end, yaw, pitch, roll))));
    }

    let hit = raster_segment_hits(&segs);
    let (mut dots, coverage) = raster_dots(&hit);

    if norms[0].2 > 0.2 {
        draw_marks(mesh, anchor, marks, (yaw, pitch, roll), proj, &mut dots);
    }

    (dots, coverage)
}

// 4x4 samples per dot: sample (gx, gy) at ((gx - 1.5) / 4, (gy - 1.5) / 4); each segment marks
// the samples of its box 0.5 or less off it
type Segment = ((f32, f32), (f32, f32));

fn raster_segment_hits(segs: &[Segment]) -> Vec<bool> {
    let (sw, sh) = (DW * 4, DH * 4);
    let mut hit = vec![false; sw * sh];

    for &(start, end) in segs {
        let span = |lo: f32, hi: f32, limit: usize| {
            (
                (4.0 * (lo - 0.5) + 1.5).ceil().max(0.0) as usize,
                ((4.0 * (hi + 0.5) + 1.5).floor().max(-1.0) + 1.0).min(limit as f32) as usize,
            )
        };

        let (x0, x1) = span(start.0.min(end.0), start.0.max(end.0), sw);
        let (y0, y1) = span(start.1.min(end.1), start.1.max(end.1), sh);

        for gy in y0..y1 {
            raster_segment_row(sw, &mut hit, start, end, x0, x1, gy);
        }
    }

    hit
}

fn raster_dots(hit: &[bool]) -> (Vec<u8>, Vec<f32>) {
    let sw = DW * 4;
    let mut dots = vec![0u8; DW * DH];
    let mut cov = vec![1.0f32; DW * DH];

    for index in 0..DW * DH {
        let (x, y) = (index % DW, index / DW);
        let covered = (0..16).filter(|j| hit[(y * 4 + j / 4) * sw + x * 4 + j % 4]).count();

        set_covered_dot(&mut dots, &mut cov, y, x, covered);
    }

    (dots, cov)
}

fn draw_marks(
    mesh: &Mesh,
    anchor: (f32, f32),
    marks: &[(i32, i32, u8)],
    (yaw, pitch, roll): (f32, f32, f32),
    proj: impl Fn(P3) -> (f32, f32),
    dots: &mut [u8],
) {
    let (ax, ay) = proj(rot(
        (anchor.0, anchor.1, mesh.front_z(anchor.0, anchor.1)),
        yaw,
        pitch,
        roll,
    ));
    let (ax, ay) = (ax.round() as i32, ay.round() as i32);

    for &(x, y, mark) in marks {
        let (x, y) = (ax + x, ay + y);

        set_eye_dot(dots, mark, x, y);
    }
}

fn raster_segment_row(
    sw: usize,
    hit: &mut [bool],
    start: (f32, f32),
    end: (f32, f32),
    x0: usize,
    x1: usize,
    gy: usize,
) {
    for gx in x0..x1 {
        if !hit[gy * sw + gx] && seg_dist(((gx as f32 - 1.5) / 4.0, (gy as f32 - 1.5) / 4.0), start, end) <= 0.5 {
            hit[gy * sw + gx] = true;
        }
    }
}

fn set_eye_dot(dots: &mut [u8], mark: u8, x: i32, y: i32) {
    if x >= 0 && y >= 0 && (x as usize) < DW && (y as usize) < DH {
        let i = y as usize * DW + x as usize;

        dots[i] = dots[i].max(mark);
    }
}

fn set_covered_dot(dots: &mut [u8], cov: &mut [f32], y: usize, x: usize, covered: usize) {
    if covered >= 6 {
        dots[y * DW + x] = 1;
        cov[y * DW + x] = covered as f32 / 16.0;
    }
}
/// [`clean`] on a DW x DH grid of dots.
pub(crate) fn clean_in(dots: &mut [u8], dw: usize, dh: usize, spurs: bool) {
    let on = |grid: &[u8], x: i32, y: i32| {
        x >= 0 && y >= 0 && (x as usize) < dw && (y as usize) < dh && grid[y as usize * dw + x as usize] == 1
    };

    let around = |grid: &[u8], x: i32, y: i32| -> Vec<(i32, i32)> {
        let mut neighbors = Vec::new();

        for (ox, oy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            append_neighbor(on, grid, x, y, &mut neighbors, ox, oy);
        }

        neighbors
    };

    remove_loose_dots(dots, dw, dh, spurs, on, around);
}

fn remove_loose_dots(
    dots: &mut [u8],
    dw: usize,
    dh: usize,
    spurs: bool,
    on: impl Fn(&[u8], i32, i32) -> bool,
    around: impl Fn(&[u8], i32, i32) -> Vec<(i32, i32)>,
) {
    for _ in 0..3 {
        let mut changed = false;

        for y in 0..dh as i32 {
            remove_loose_row(dots, dw, spurs, &on, &around, &mut changed, y);
        }

        if !changed {
            break;
        }
    }
}

fn remove_loose_row(
    dots: &mut [u8],
    dw: usize,
    spurs: bool,
    on: &impl Fn(&[u8], i32, i32) -> bool,
    around: &impl Fn(&[u8], i32, i32) -> Vec<(i32, i32)>,
    changed: &mut bool,
    y: i32,
) {
    for x in 0..dw as i32 {
        if !on(dots, x, y) {
            continue;
        }

        let neighbors = around(dots, x, y);
        let lone = neighbors.is_empty();
        let spur = spurs && neighbors.len() == 1 && around(dots, neighbors[0].0, neighbors[0].1).len() >= 4;

        if lone || spur {
            dots[y as usize * dw + x as usize] = 0;
            *changed = true;
        }
    }
}

fn append_neighbor(
    on: impl Fn(&[u8], i32, i32) -> bool,
    grid: &[u8],
    x: i32,
    y: i32,
    neighbors: &mut Vec<(i32, i32)>,
    ox: i32,
    oy: i32,
) {
    if on(grid, x + ox, y + oy) {
        neighbors.push((x + ox, y + oy));
    }
}
