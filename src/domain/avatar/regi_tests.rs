use super::*;

#[test]
fn floating_solids_stay_inside_the_canvas() {
    // every solid, every motion, a range of turns: each vertex of each frame on a dot of the canvas
    for solid in SOLIDS.iter().chain(&NEW_SOLIDS).chain(&MORE_SOLIDS) {
        let (mesh, _, _) = solid.mesh();

        for motion in MOTIONS {
            assert_turns_inside(*solid, &mesh, motion);
        }
    }
}

fn assert_turns_inside(solid: Solid, mesh: &Mesh, motion: Motion) {
    for yaw in [-0.5f32, -0.2, 0.0, 0.3, 0.6] {
        let fit = fit(mesh, motion, yaw);

        assert!(fit.scale > 0.75, "{solid:?} {motion:?}: scale {}", fit.scale);

        for status in 0..4 {
            assert_status_poses_inside(solid, mesh, motion, yaw, fit, status);
        }
    }
}

fn assert_status_poses_inside(solid: Solid, mesh: &Mesh, motion: Motion, yaw: f32, fit: Fit, status: usize) {
    for i in 0..400 {
        let (pitch, roll, bob) = pose(motion, status, i as f32 * 0.05);

        for &vertex in &mesh.verts {
            let (x, y) = persp(rot(vertex, yaw, pitch, roll), bob);
            let (x, y) = (fit.cx + x * fit.scale, fit.cy + y * fit.scale);

            assert!(
                (-0.5..DW as f32 - 0.5).contains(&x) && (-0.5..DH as f32 - 0.5).contains(&y),
                "{solid:?} {motion:?} yaw {yaw} s {status}: ({x}, {y})"
            );
        }
    }
}

#[test]
fn lines_have_no_loose_ends() {
    // every regi, every status, 20 s at 10 fps: each line dot touches 2+ dots (a closed
    // wireframe: 0 lone dots, 0 spurs off a corner)
    assert_all_regis_closed();
}

fn assert_all_regis_closed() {
    for (index, model) in all().iter().enumerate() {
        for status in 0..5 {
            assert_closed_frames(index, model, status);
        }
    }
}

fn assert_closed_frames(index: usize, model: &Regi, status: usize) {
    for frame in 0..200 {
        let time = frame as f32 / 10.0;
        let dots = dots_with(model, model.pattern, status, time, 1.0, |_| true);

        assert_closed_dots(index, status, time, &dots);
    }
}

fn assert_closed_dots(index: usize, status: usize, time: f32, dots: &[u8]) {
    for position in 0..DW * DH {
        if dots[position] != 1 {
            continue;
        }

        let (x, y) = ((position % DW) as i32, (position / DW) as i32);
        let neighbors = neighbor_count(dots, x, y);

        assert!(
            neighbors >= 2,
            "regi {index} s{status} t{time}: a loose dot at ({x}, {y})"
        );
    }
}

fn neighbor_count(dots: &[u8], x: i32, y: i32) -> usize {
    [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
        .iter()
        .filter(|&&(ox, oy)| {
            let (x, y) = (x + ox, y + oy);

            x >= 0 && y >= 0 && (x as usize) < DW && (y as usize) < DH && dots[y as usize * DW + x as usize] != 0
        })
        .count()
}

#[test]
fn working_eyes_blink_one_by_one() {
    // over 40 s at 60 fps: each dot blinks, mostly 0 or 1 dot off, never half of them
    for talk in TALKS {
        let dot_count = 9;
        let (mut off_any, mut multi) = (vec![false; dot_count], 0);

        for frame in 0..2400 {
            let time = frame as f32 / 60.0;
            let off = (0..dot_count).filter(|&dot| blinks(talk, dot, time)).count();

            assert!(off <= dot_count / 2, "{talk:?} {time}: {off} off");

            multi += usize::from(off > 1);
            (0..dot_count).for_each(|dot| off_any[dot] |= blinks(talk, dot, time));
        }

        assert!(off_any.iter().all(|&blinked| blinked), "{talk:?}: a dot never blinks");
        assert!(multi < 2400 / 20, "{talk:?}: {multi} frames with 2+ off");
    }
}

#[test]
fn eye_patterns_stay_rigid() {
    for (index, model) in all().iter().enumerate() {
        assert_model_rigid(index, model);
    }
}

type EyeDots = Vec<(i32, i32)>;

struct RigidityCheck<'model> {
    index: usize,
    model: &'model Regi,
    pattern: Pattern,
    shape: Option<EyeDots>,
}

fn assert_model_rigid(index: usize, model: &Regi) {
    for pattern in std::iter::once(model.pattern).chain(PATTERNS) {
        let mut check = RigidityCheck {
            index,
            model,
            pattern,
            shape: None,
        };

        assert_pattern_rigid(&mut check);
    }
}

fn assert_pattern_rigid(check: &mut RigidityCheck<'_>) {
    for status in 0..5 {
        for frame in 0..100 {
            check.frame(status, frame as f32 / 10.0);
        }
    }
}

impl RigidityCheck<'_> {
    fn frame(&mut self, status: usize, time: f32) {
        let model = self.model;
        let pattern = self.pattern;
        let count = pattern.dots().len();
        let full = eye_dots(&dots_with(model, pattern, status, time, 1.0, |_| true));

        if full.is_empty() {
            return;
        }

        assert_eq!(
            full.len(),
            count,
            "regi {} {pattern:?} s{status} t{time}: {} of {count} eyes",
            self.index,
            full.len()
        );
        let shape = normalize_eyes(&full);

        match &self.shape {
            None => self.shape = Some(shape),
            Some(expected) => assert_eq!(
                expected, &shape,
                "regi {} {pattern:?} s{status} t{time}: the eye pattern changed",
                self.index
            ),
        }

        let dots = dots_with(model, pattern, status, time, 1.0, |key| {
            lit(pattern, model.talk, model.blink, status, time + model.phase, key)
        });

        assert!(
            eye_dots(&dots).iter().all(|point| full.contains(point)),
            "regi {} {pattern:?} s{status} t{time}: a lit eye off the pattern",
            self.index
        );
        assert_eye_cells(self.index, pattern, status, time, &dots);
    }
}

fn eye_dots(dots: &[u8]) -> EyeDots {
    (0..DW * DH)
        .filter(|&index| dots[index] == 2)
        .map(|index| ((index % DW) as i32, (index / DW) as i32))
        .collect()
}

fn normalize_eyes(eyes: &[(i32, i32)]) -> EyeDots {
    let min_x = eyes.iter().map(|point| point.0).min().unwrap_or(0);
    let min_y = eyes.iter().map(|point| point.1).min().unwrap_or(0);
    let mut shape: EyeDots = eyes.iter().map(|point| (point.0 - min_x, point.1 - min_y)).collect();

    shape.sort_unstable();
    shape
}

fn assert_eye_cells(index: usize, pattern: Pattern, status: usize, time: f32, dots: &[u8]) {
    let (eye_color, frame_color) = colors(status);
    let drawn = cells(dots, eye_color, frame_color);

    for (cell_index, cell) in drawn.iter().enumerate() {
        let bits = (cell.ch as u32).saturating_sub(0x2800) & 0xFF;
        let (mut frame, mut eye) = (0, 0);

        collect_cell_bits(dots, cell_index, &mut frame, &mut eye);
        assert_eq!(
            bits,
            frame | eye,
            "regi {index} {pattern:?} s{status} t{time}: cell {cell_index} a gap"
        );

        if eye_color != frame_color && cell.fg == eye_color {
            assert_eq!(
                frame, 0,
                "regi {index} {pattern:?} s{status} t{time}: cell {cell_index} a frame dot in the eye color"
            );
        }
    }
}

fn collect_cell_bits(dots: &[u8], cell_index: usize, frame: &mut u32, eye: &mut u32) {
    for bit in 0..8 {
        match dots[((cell_index / 7) * 4 + bit / 2) * DW + (cell_index % 7) * 2 + bit % 2] {
            1 => *frame |= BIT[bit / 2][bit % 2],
            2 => *eye |= BIT[bit / 2][bit % 2],
            _ => {}
        }
    }
}

#[test]
fn thinking_eyes_blink_slower() {
    // over 60 s at 60 fps: each dot blinks, blinks last longer, fewer blinks than working
    let dot_count = 9;

    for talk in TALKS {
        let count = |off: &dyn Fn(usize, f32) -> bool| {
            let (mut starts, mut frames, mut seen) = (0, 0, vec![false; dot_count]);

            count_blinks(dot_count, off, &mut starts, &mut frames, &mut seen);

            (
                starts,
                frames as f32 / starts.max(1) as f32,
                seen.iter().all(|&blinked| blinked),
            )
        };

        let (ws, wlen, _) = count(&|dot, time| blinks(talk, dot, time));
        let (ts, tlen, all) = count(&|dot, time| blinks_slow(talk, dot, time));

        assert!(all, "{talk:?}: a dot never blinks");
        assert!(
            ts < ws && tlen > wlen,
            "{talk:?}: thinking {ts} blinks of {tlen} frames, working {ws} of {wlen}"
        );

        for frame in 0..3600 {
            let off = (0..dot_count)
                .filter(|&dot| blinks_slow(talk, dot, frame as f32 / 60.0))
                .count();

            assert!(off <= dot_count / 2, "{talk:?}: {off} off");
        }
    }
}

fn count_blinks(
    dot_count: usize,
    off: &dyn Fn(usize, f32) -> bool,
    starts: &mut usize,
    frames: &mut usize,
    seen: &mut [bool],
) {
    for (dot, observed) in seen.iter_mut().enumerate().take(dot_count) {
        let mut was = false;

        for frame in 0..3600 {
            let blinked = off(dot, frame as f32 / 60.0);

            *starts += usize::from(blinked && !was);
            *frames += usize::from(blinked);
            *observed |= blinked;
            was = blinked;
        }
    }
}
