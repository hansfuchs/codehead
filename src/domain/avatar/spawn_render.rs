use super::{
    BG, CENTER, CH, CORAL, CW, Cell, DOT_HEIGHT, DOT_WIDTH, FAMILIES, Family, GOO, INK, OX, OY, PERIOD, Particle,
    SETTLE, ease_in_out, hash, mask, mix, path, ripples, spray,
};

pub(crate) fn dot(dots: &mut [u8], x: f32, y: f32, hot: bool) {
    let (x, y) = (x.round(), y.round());

    if x >= 0.0 && y >= 0.0 && (x as usize) < DOT_WIDTH && (y as usize) < DOT_HEIGHT {
        let key = y as usize * DOT_WIDTH + x as usize;

        dots[key] = dots[key].max(if hot { 2 } else { 1 });
    }
}
/// Sticky timing: slow to let go, slow to arrive.
pub(crate) fn sticky(user: f32) -> f32 {
    let user_value = user.clamp(0.0, 1.0);
    let user_2 = user_value * user_value * (3.0 - 2.0 * user_value);

    user_2 * user_2 * (3.0 - 2.0 * user_2)
}
/// Animation I: the stagger, the flight time of 1 blob, the snap (s from the start).
pub(crate) fn timing(i: usize) -> (f32, f32, f32) {
    let (fam, value) = (FAMILIES[i / 5], i % 5);

    let (stagger, dur): (f32, f32) = match (fam, value) {
        (Family::Fountain, 3) => (0.5, 0.45),
        (Family::Fountain, _) => (0.7, 0.75),
        (Family::Splash, 2) => (0.25, 0.55),
        (Family::Splash, _) => (0.35, 0.9),
        (Family::Rain, 3) => (1.0, 0.5),
        (Family::Rain, _) => (0.7, 0.6),
        (Family::Vortex, 1) => (0.3, 1.0),
        (Family::Vortex, _) => (0.4, 1.1),
        (Family::Rise, _) => (0.6, 0.8),
        (Family::Ripple, 3) => (0.0, 0.6),
        (Family::Ripple, _) => (0.0, 1.2),
        (Family::Wave, _) => (0.8, 0.7),
        (Family::Condense, 4) => (0.4, 0.9),
        (Family::Condense, _) => (0.5, 1.1),
        (Family::Drip, _) => (1.1, 0.5),
        (Family::Merge, _) => (0.0, 1.7),
    };

    let dur = if fam == Family::Merge { dur } else { dur * GOO };

    (
        stagger,
        dur,
        stagger + dur + if fam == Family::Merge { 0.0 } else { SETTLE },
    )
}
/// The canvas of animation I at T: 13x6 cells. SPRITE: the 7x4 first frame of the agent; LIVE:
/// the agent S seconds after the snap.
pub(crate) fn render(i: usize, sprite: &[Cell], live: &dyn Fn(f32) -> Vec<Cell>, time: f32) -> Vec<Cell> {
    let (stagger, duration, end) = timing(i);
    let frame = SpawnFrame {
        family: FAMILIES[i / 5],
        variant: i % 5,
        time: time.rem_euclid(PERIOD),
        duration,
        end,
    };
    let particles = particles(sprite, i, stagger, &frame);
    let mut dots = vec![0; DOT_WIDTH * DOT_HEIGHT];

    if frame.time > PERIOD - 0.3 {
        return vec![Cell::BLANK; CW * CH];
    }

    if frame.time < frame.end {
        form_particles(&frame, &particles, &mut dots);
        spray(
            frame.family,
            frame.variant,
            frame.time,
            frame.end,
            i,
            &mut |x, y, hot| dot(&mut dots, x, y, hot),
        );
    }

    let mut cells = (0..CW * CH)
        .map(|index| dot_cell(&dots, index % CW, index / CW))
        .collect::<Vec<_>>();

    if frame.time >= frame.end {
        snap_sprite(&mut cells, &live(frame.time - frame.end), frame.time - frame.end < 0.12);
    }

    cells
}

struct SpawnFrame {
    family: Family,
    variant: usize,
    time: f32,
    duration: f32,
    end: f32,
}

fn particles(sprite: &[Cell], animation: usize, stagger: f32, frame: &SpawnFrame) -> Vec<Particle> {
    let mut particles = Vec::new();

    for (index, cell) in sprite.iter().enumerate() {
        append_particles(&mut particles, *cell, index, animation);
    }

    for particle in &mut particles {
        particle.delay = particle_order(particle, frame).clamp(0.0, 1.0) * stagger;
    }

    particles
}

fn append_particles(particles: &mut Vec<Particle>, cell: Cell, index: usize, animation: usize) {
    let (cx, cy) = (index % 7, index / 7);
    let dots = mask(cell.ch, cx);

    for index in 0..8 {
        let (x, y) = (index % 2, index / 2);

        if !dots[y][x] {
            continue;
        }

        let goal = (((OX + cx) * 2 + x) as f32, ((OY + cy) * 4 + y) as f32);
        let seed = (goal.0 as u64) * 131 + goal.1 as u64 + animation as u64 * 7919;

        particles.push(Particle {
            goal,
            random: (hash(seed), hash(seed + 1), hash(seed + 2)),
            delay: 0.0,
        });
    }
}

fn particle_order(particle: &Particle, frame: &SpawnFrame) -> f32 {
    let distance = |goal: (f32, f32)| ((goal.0 - CENTER.0).hypot((goal.1 - CENTER.1) * 1.2) / 10.0).min(1.0);

    match (frame.family, frame.variant) {
        (Family::Fountain, 4) | (Family::Wave, 0) => particle.goal.0 / DOT_WIDTH as f32,
        (Family::Wave, 1) => 1.0 - particle.goal.0 / DOT_WIDTH as f32,
        (Family::Wave, 2) => 1.0 - (particle.goal.0 - CENTER.0).abs() / 8.0,
        (Family::Wave, _) | (Family::Rise | Family::Drip, 3) | (Family::Rain, 4) => {
            1.0 - particle.goal.1 / DOT_HEIGHT as f32
        }
        (Family::Rise, 2) | (Family::Splash, _) | (Family::Vortex, 3) => distance(particle.goal),
        (Family::Rain, 3) | (Family::Condense, _) => particle.random.0,
        (Family::Drip, _) => particle.random.0 * 0.3 + (particle.goal.1 / DOT_HEIGHT as f32) * 0.7,
        _ => 0.6 * particle.random.0 + 0.4 * distance(particle.goal),
    }
}

fn form_particles(frame: &SpawnFrame, particles: &[Particle], dots: &mut [u8]) {
    if frame.family == Family::Merge {
        merge(frame.variant, particles, frame.time / frame.end, &mut |x, y, hot| {
            dot(dots, x, y, hot);
        });
        return;
    }

    let mut field = BlobField {
        values: vec![0.0; dots.len()],
        heat: vec![0.0; dots.len()],
    };

    for particle in particles {
        draw_particle(particle, frame, &mut field);
    }

    for (index, dot) in dots.iter_mut().enumerate() {
        apply_blob_coverage(&field, index, dot);
    }

    ripples(frame.family, frame.variant, frame.time, &mut |x, y, hot| {
        dot(dots, x, y, hot);
    });
}

fn apply_blob_coverage(field: &BlobField, index: usize, dot: &mut u8) {
    if field.values[index] > 0.5 {
        *dot = if field.heat[index] > 0.3 { 2 } else { 1 };
    }
}

fn draw_particle(particle: &Particle, frame: &SpawnFrame, field: &mut BlobField) {
    let progress = (frame.time - particle.delay) / frame.duration;

    if progress < 0.0 && !matches!(frame.family, Family::Condense | Family::Wave) {
        return;
    }

    let Some((mut x, mut y)) = path(frame.family, frame.variant, particle, sticky(progress), frame.time) else {
        return;
    };

    let elapsed = frame.time - particle.delay - frame.duration;

    if elapsed > 0.0
        && let Some(previous) = path(frame.family, frame.variant, particle, 0.93, frame.time)
    {
        let (dx, dy) = (particle.goal.0 - previous.0, particle.goal.1 - previous.1);
        let length = dx.hypot(dy).max(0.01);
        let offset = 1.6 * (-6.5 * elapsed).exp() * (17.0 * elapsed).sin();

        (x, y) = (x + dx / length * offset, y + dy / length * offset);
    }

    let settle = (elapsed / SETTLE).clamp(0.0, 1.0);
    let radius = 2.9 - 1.9 * settle * settle;

    field.blob(x, y, radius, settle < 0.6);

    if progress < 1.0 {
        draw_trail(particle, frame, field, progress, radius);
    }
}

fn draw_trail(particle: &Particle, frame: &SpawnFrame, field: &mut BlobField, progress: f32, radius: f32) {
    for offset in 1..=2 {
        if let Some((x, y)) = path(
            frame.family,
            frame.variant,
            particle,
            sticky(progress - offset as f32 * 0.07),
            frame.time,
        ) {
            field.blob(x, y, radius * (1.0 - 0.22 * offset as f32), true);
        }
    }
}

struct BlobField {
    values: Vec<f32>,
    heat: Vec<f32>,
}

impl BlobField {
    fn blob(&mut self, x: f32, y: f32, radius: f32, hot: bool) {
        let extent = radius.ceil() as i32;
        let (cx, cy) = (x.round() as i32, y.round() as i32);

        for yy in (cy - extent).max(0)..=(cy + extent).min(DOT_HEIGHT as i32 - 1) {
            blob_row(x, y, radius, hot, cx, yy, self);
        }
    }
}

fn blob_row(x: f32, y: f32, radius: f32, hot: bool, cx: i32, yy: i32, field: &mut BlobField) {
    let extent = radius.ceil() as i32;

    for xx in (cx - extent).max(0)..=(cx + extent).min(DOT_WIDTH as i32 - 1) {
        let distance = (xx as f32 - x).powi(2) + (yy as f32 - y).powi(2);
        let coverage = 1.0 - distance / (radius * radius);

        if coverage.is_nan() || coverage <= 0.0 {
            continue;
        }

        let index = yy as usize * DOT_WIDTH + xx as usize;

        field.values[index] += coverage * coverage;

        if hot {
            field.heat[index] += coverage * coverage;
        }
    }
}

fn dot_cell(dots: &[u8], x: usize, y: usize) -> Cell {
    const DOT_BITS: [u32; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];
    let (mut bits, mut hot) = (0, false);

    for (index, bit) in DOT_BITS.iter().enumerate() {
        let value = dots[(y * 4 + index / 2) * DOT_WIDTH + x * 2 + index % 2];

        if value > 0 {
            bits |= bit;
        }

        hot |= value == 2;
    }

    if bits == 0 {
        return Cell::BLANK;
    }

    Cell {
        ch: char::from_u32(0x2800 + bits).unwrap_or(' '),
        fg: if hot { CORAL } else { mix(BG, INK, 0.8) },
        bg: None,
    }
}

fn snap_sprite(cells: &mut [Cell], sprite: &[Cell], flash: bool) {
    for (index, cell) in sprite.iter().enumerate() {
        let target = (OY + index / 7) * CW + OX + index % 7;

        cells[target] = if flash && cell.ch != ' ' {
            Cell { fg: CORAL, ..*cell }
        } else {
            *cell
        };
    }
}
/// Metaballs: drops fly in, merge into 1 blob, the blob turns into the sprite. U: 0..1.
pub(super) fn merge(value: usize, ps: &[Particle], user: f32, put: &mut impl FnMut(f32, f32, bool)) {
    let starts: &[(f32, f32)] = match value {
        0 => &[(-4.0, CENTER.1), (DOT_WIDTH as f32 + 3.0, CENTER.1)],
        1 => &[(CENTER.0 - 8.0, -5.0), (CENTER.0, -9.0), (CENTER.0 + 8.0, -6.0)],
        2 => &[
            (-3.0, -3.0),
            (DOT_WIDTH as f32 + 2.0, -3.0),
            (-3.0, DOT_HEIGHT as f32 + 2.0),
            (DOT_WIDTH as f32 + 2.0, DOT_HEIGHT as f32 + 2.0),
        ],
        3 => &[(CENTER.0, -12.0)],
        _ => &[
            (CENTER.0 - 6.0, DOT_HEIGHT as f32 + 4.0),
            (CENTER.0 + 2.0, DOT_HEIGHT as f32 + 9.0),
            (CENTER.0 + 7.0, DOT_HEIGHT as f32 + 6.0),
            (CENTER.0 - 1.0, DOT_HEIGHT as f32 + 14.0),
        ],
    };

    let agent = (user / 0.45).clamp(0.0, 1.0);
    let message = ((user - 0.4) / 0.45).clamp(0.0, 1.0);
    let message_value = message * message * (3.0 - 2.0 * message);
    let rad = if value == 3 { 6.0 } else { 3.6 };

    let blobs: Vec<(f32, f32, f32)> = starts
        .iter()
        .enumerate()
        .map(|(key, &(sx, sy))| {
            let error = ease_in_out((agent - key as f32 * 0.06).clamp(0.0, 1.0));

            let wob = if value == 4 {
                (user * 12.0 + key as f32).sin() * 1.5
            } else {
                0.0
            };

            (
                sx + (CENTER.0 - sx) * error + wob,
                sy + (CENTER.1 - sy) * error,
                rad * (1.0 - 0.4 * message_value),
            )
        })
        .collect();

    let sigma = 0.8 + (1.0 - message_value) * 3.0;

    for y in 0..DOT_HEIGHT {
        for x in 0..DOT_WIDTH {
            let (fx, fy) = (x as f32, y as f32);
            let bf: f32 = blobs
                .iter()
                .map(|&(bx, by, result)| result * result / ((fx - bx).powi(2) + (fy - by).powi(2) * 1.3 + 0.01))
                .sum();

            let tf = sprite_density(ps, message_value, sigma, fx, fy);

            draw_merged_dot(put, message_value, fx, fy, bf, tf);
        }
    }
}

fn sprite_density(ps: &[Particle], message_value: f32, sigma: f32, fx: f32, fy: f32) -> f32 {
    if message_value > 0.0 {
        ps.iter()
            .map(|path| (-((fx - path.goal.0).powi(2) + (fy - path.goal.1).powi(2)) / (sigma * sigma)).exp())
            .fold(0.0, f32::max)
    } else {
        0.0
    }
}

fn draw_merged_dot(put: &mut impl FnMut(f32, f32, bool), message_value: f32, fx: f32, fy: f32, bf: f32, tf: f32) {
    if (1.0 - message_value) * bf + message_value * tf * 2.0 > 1.0 {
        put(fx, fy, message_value < 0.95);
    }
}
