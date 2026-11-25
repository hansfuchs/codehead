use super::{CENTER, DOT_HEIGHT, DOT_WIDTH, Family, Particle, hash};

pub(crate) fn ease_out(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}
pub(crate) fn ease_in_out(progress: f32) -> f32 {
    if progress < 0.5 {
        4.0 * progress * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
    }
}
pub(crate) fn ease_back(progress: f32, overshoot: f32) -> f32 {
    let back = progress - 1.0;

    1.0 + (overshoot + 1.0) * back * back * back + overshoot * back * back
}
/// Position of particle P at flight progress U (0..1) and loop time TAU.
pub(super) fn path(fam: Family, variant: usize, particle: &Particle, progress: f32, tau: f32) -> Option<(f32, f32)> {
    let goal = particle.goal;
    let (r0, r1, r2) = particle.random;
    let lerp = |from: f32, to: f32, amount: f32| from + (to - from) * amount;

    Some(match fam {
        Family::Fountain => fountain_path(variant, progress, goal, r0, r1, lerp),
        Family::Splash => splash_path(variant, progress, goal),
        Family::Rain => path_rain(variant, progress, goal, r1, r2),
        Family::Vortex => path_vortex(variant, progress, goal, r0, lerp),
        Family::Rise => {
            let px = if variant == 4 {
                (rise_start_column(goal)) + (r0 - 0.5) * 5.0
            } else {
                CENTER.0 + (goal.0 - CENTER.0) * 1.8 + (r0 - 0.5) * 2.0
            };

            let up = if variant == 1 {
                ease_back(progress, 3.2)
            } else {
                ease_back(progress, 1.4)
            };

            let wob = (progress * std::f32::consts::PI * 3.0 + r1 * 6.0).sin() * (1.0 - progress) * 1.5;

            (
                lerp(px, goal.0, ease_in_out(progress)) + wob,
                lerp(DOT_HEIGHT as f32 - 1.0, goal.1, up),
            )
        }
        Family::Ripple => {
            // the dot appears when the front passes, a radial bump after
            let origin = if variant == 4 {
                (CENTER.0, DOT_HEIGHT as f32)
            } else {
                CENTER
            };

            let speed = if variant == 3 { 34.0 } else { 18.0 };
            let (dx, dy) = (goal.0 - origin.0, goal.1 - origin.1);
            let distance = dx.hypot(dy);
            let front = tau * speed;

            if front < distance {
                return None;
            }

            let bump = 1.6 * ((front - distance) * 0.5).sin() * (-(front - distance) * 0.18).exp() * (1.0 - progress);
            let radius = distance.max(0.01);

            (goal.0 + dx / radius * bump, goal.1 + dy / radius * bump)
        }
        Family::Wave => path_wave(variant, progress, tau, goal, lerp),
        Family::Condense => match path_condense(variant, progress, tau, goal, (r0, r1, r2), lerp) {
            Ok(point) => point,
            Err(missed) => return missed,
        },
        Family::Drip => drip_path(variant, progress, goal, lerp),
        Family::Merge => return None,
    })
}

fn path_vortex(
    variant: usize,
    progress: f32,
    goal: (f32, f32),
    r0: f32,
    lerp: impl Fn(f32, f32, f32) -> f32,
) -> (f32, f32) {
    let (dx, dy) = (goal.0 - CENTER.0, goal.1 - CENTER.1);
    let (rg, ag) = (dx.hypot(dy), dy.atan2(dx));

    let spin = match variant {
        1 => 9.0,
        2 => -5.0,
        _ => 5.0,
    };

    let radius = if variant == 3 {
        lerp(0.0, rg, ease_out(progress))
    } else {
        lerp(15.0 + r0 * 4.0, rg, ease_out(progress))
    };

    let angle = ag + spin * (1.0 - ease_out(progress));
    let sy = if variant == 4 { 0.45 + 0.55 * progress } else { 1.0 };

    (CENTER.0 + angle.cos() * radius, CENTER.1 + angle.sin() * radius * sy)
}

fn path_condense(
    variant: usize,
    progress: f32,
    tau: f32,
    goal: (f32, f32),
    (r0, r1, r2): (f32, f32, f32),
    lerp: impl Fn(f32, f32, f32) -> f32,
) -> Result<(f32, f32), Option<(f32, f32)>> {
    let start = match variant {
        1 => (r0 * DOT_WIDTH as f32, r1 * 6.0 - 4.0),
        2 => {
            let angle = r0 * std::f32::consts::TAU;

            (CENTER.0 + angle.cos() * 13.0, CENTER.1 + angle.sin() * 11.0)
        }
        _ => (r0 * DOT_WIDTH as f32, r1 * DOT_HEIGHT as f32),
    };

    if variant == 3 && r2 < 0.5 && progress < 0.6 {
        return Err(None);
    }

    let drift = (
        (tau * 1.7 + r2 * 6.0).sin() * 2.5,
        (tau * 1.3 + r0 * 5.0).cos() * 2.0 + if variant == 1 { tau * 3.0 } else { 0.0 },
    );

    let pull = if variant == 4 {
        progress.powi(5)
    } else {
        progress.powi(3)
    };

    Ok((
        lerp(start.0 + drift.0, goal.0, pull),
        lerp(start.1 + drift.1, goal.1, pull),
    ))
}

fn path_wave(
    variant: usize,
    progress: f32,
    tau: f32,
    goal: (f32, f32),
    lerp: impl Fn(f32, f32, f32) -> f32,
) -> (f32, f32) {
    let amp = if variant == 4 { 7.0 } else { 3.5 };
    let swell = (goal.0 * 0.45 - tau * 9.0).sin() * amp * (1.0 - ease_out(progress));

    match variant {
        0 | 4 => (lerp(-3.0, goal.0, ease_out(progress)), goal.1 + swell),
        1 => (lerp(DOT_WIDTH as f32 + 2.0, goal.0, ease_out(progress)), goal.1 + swell),
        2 => (
            lerp(
                if goal.0 < CENTER.0 {
                    -3.0
                } else {
                    DOT_WIDTH as f32 + 2.0
                },
                goal.0,
                ease_out(progress),
            ),
            goal.1 + swell,
        ),
        _ => (
            goal.0 + swell * 0.6,
            lerp(DOT_HEIGHT as f32 + 2.0, goal.1, ease_out(progress)),
        ),
    }
}

fn path_rain(variant: usize, progress: f32, goal: (f32, f32), r1: f32, r2: f32) -> (f32, f32) {
    let fall = 8.0 + r1 * 14.0;
    let wind = if variant == 1 { -7.0 } else { 0.0 };
    let hit = 0.7;

    if progress < hit {
        let fall_progress = (progress / hit).powi(2);

        (
            goal.0 + wind * (1.0 - fall_progress),
            goal.1 - fall * (1.0 - fall_progress),
        )
    } else {
        let bounce = (progress - hit) / (1.0 - hit);
        let hb = if variant == 2 { 0.0 } else { 2.5 + r2 * 1.5 };

        (
            goal.0,
            goal.1 - hb * (bounce * std::f32::consts::PI * 2.0).sin().abs() * (1.0 - bounce),
        )
    }
}

fn rise_start_column(goal: (f32, f32)) -> f32 {
    if goal.0 < CENTER.0 { 3.0 } else { DOT_WIDTH as f32 - 4.0 }
}

fn fountain_path(
    variant: usize,
    progress: f32,
    goal: (f32, f32),
    r0: f32,
    r1: f32,
    lerp: impl Fn(f32, f32, f32) -> f32,
) -> (f32, f32) {
    let sx = match variant {
        1 => {
            if r0 < 0.5 {
                4.0
            } else {
                DOT_WIDTH as f32 - 5.0
            }
        }
        4 => 0.0,
        _ => CENTER.0 + (r1 - 0.5) * 2.0,
    };

    let arc = match variant {
        2 => 22.0,
        3 => 7.0,
        _ => 14.0,
    };

    (
        lerp(sx, goal.0, progress),
        lerp(DOT_HEIGHT as f32 + 1.0, goal.1, progress) - arc * 4.0 * progress * (1.0 - progress),
    )
}

fn splash_path(variant: usize, progress: f32, goal: (f32, f32)) -> (f32, f32) {
    let origin = match variant {
        3 => {
            if goal.0 < CENTER.0 {
                (CENTER.0 - 5.0, CENTER.1 - 2.0)
            } else {
                (CENTER.0 + 5.0, CENTER.1 - 2.0)
            }
        }
        4 => (CENTER.0, DOT_HEIGHT as f32 + 1.0),
        _ => CENTER,
    };

    let (damping, waves) = match variant {
        1 => (3.0, 3.5),
        2 => (7.0, 1.6),
        _ => (4.5, 2.6),
    };

    let offset = (-damping * progress).exp() * (std::f32::consts::PI * waves * progress).cos() * (1.0 - progress);

    (
        goal.0 + (origin.0 - goal.0) * offset,
        goal.1 + (origin.1 - goal.1) * offset,
    )
}

fn drip_path(variant: usize, progress: f32, goal: (f32, f32), lerp: impl Fn(f32, f32, f32) -> f32) -> (f32, f32) {
    let tap = match variant {
        1 => 2.0,
        2 => {
            if goal.0 < CENTER.0 {
                CENTER.0 - 6.0
            } else {
                CENTER.0 + 6.0
            }
        }
        _ => CENTER.0,
    };

    if progress < 0.55 {
        let fall = (progress / 0.55).powi(2);

        (tap, lerp(-1.0, goal.1, fall))
    } else {
        let spread = ease_out((progress - 0.55) / 0.45);

        let hop = if variant == 4 {
            2.0 * (spread * std::f32::consts::PI).sin()
        } else {
            0.0
        };

        (lerp(tap, goal.0, spread), goal.1 - hop)
    }
}
/// Ripple rings: the fronts as circles of dots.
pub(crate) fn ripples(fam: Family, variant: usize, tau: f32, put: &mut impl FnMut(f32, f32, bool)) {
    if fam != Family::Ripple {
        return;
    }

    let origin = if variant == 4 {
        (CENTER.0, DOT_HEIGHT as f32)
    } else {
        CENTER
    };

    let speed = if variant == 3 { 34.0 } else { 18.0 };

    let rings = match variant {
        1 => 2,
        2 => 3,
        _ => 1,
    };

    for ring in 0..rings {
        let radius = (tau - ring as f32 * 0.22) * speed;

        if !(0.5..20.0).contains(&radius) {
            continue;
        }

        let count = (radius * 4.0) as usize;

        for j in 0..count {
            let angle = std::f32::consts::TAU * j as f32 / count as f32;

            put(
                origin.0 + angle.cos() * radius,
                origin.1 + angle.sin() * radius * 0.9,
                true,
            );
        }
    }
}
/// Droplets thrown off as the liquid lands: ballistic, gone after 0.5 s.
pub(crate) fn spray(fam: Family, variant: usize, tau: f32, end: f32, i: usize, put: &mut impl FnMut(f32, f32, bool)) {
    let (count, at, from) = match (fam, variant) {
        (Family::Fountain, _) => (10, end * 0.55, (CENTER.0, CENTER.1 + 4.0)),
        (Family::Splash, _) => (14, 0.05, CENTER),
        (Family::Rain, 2) => (16, end * 0.6, (CENTER.0, CENTER.1 + 6.0)),
        (Family::Drip, 4 | 0) => (8, end * 0.5, (CENTER.0, CENTER.1)),
        (Family::Rise, 1) => (8, end * 0.75, (CENTER.0, CENTER.1 - 6.0)),
        (Family::Merge, 3) => (12, end * 0.4, CENTER),
        _ => return,
    };

    for drop in 0..count {
        let age = tau - at - (drop % 3) as f32 * 0.06;

        // gone before the snap: 0 droplets over the sprite's first frame
        if !(0.0..0.5).contains(&age) || tau > end - 0.05 {
            continue;
        }

        let seed = (i * 97 + drop) as u64 * 31;
        let angle = -std::f32::consts::PI * (0.1 + 0.8 * hash(seed));
        let sp = 14.0 + 16.0 * hash(seed + 1);

        put(
            from.0 + angle.cos() * sp * age,
            from.1 + angle.sin() * sp * age + 40.0 * age * age,
            true,
        );
    }
}
