use super::{CENTER, DOT_HEIGHT, DOT_WIDTH, Family, Particle, hash};

pub(crate) fn ease_out(user: f32) -> f32 {
    1.0 - (1.0 - user).powi(3)
}
pub(crate) fn ease_in_out(user: f32) -> f32 {
    if user < 0.5 {
        4.0 * user * user * user
    } else {
        1.0 - (-2.0 * user + 2.0).powi(3) / 2.0
    }
}
pub(crate) fn ease_back(user: f32, key: f32) -> f32 {
    let user_value = user - 1.0;

    1.0 + (key + 1.0) * user_value * user_value * user_value + key * user_value * user_value
}
/// Position of particle P at flight progress U (0..1) and loop time TAU.
pub(super) fn path(fam: Family, value: usize, path_value: &Particle, user: f32, tau: f32) -> Option<(f32, f32)> {
    let group = path_value.goal;
    let (r0, r1, r2) = path_value.random;
    let lerp = |agent: f32, bytes: f32, key: f32| agent + (bytes - agent) * key;

    Some(match fam {
        Family::Fountain => fountain_path(value, user, group, r0, r1, lerp),
        Family::Splash => splash_path(value, user, group),
        Family::Rain => path_rain(value, user, group, r1, r2),
        Family::Vortex => path_vortex(value, user, group, r0, lerp),
        Family::Rise => {
            let px = if value == 4 {
                (rise_start_column(group)) + (r0 - 0.5) * 5.0
            } else {
                CENTER.0 + (group.0 - CENTER.0) * 1.8 + (r0 - 0.5) * 2.0
            };

            let up = if value == 1 {
                ease_back(user, 3.2)
            } else {
                ease_back(user, 1.4)
            };

            let wob = (user * std::f32::consts::PI * 3.0 + r1 * 6.0).sin() * (1.0 - user) * 1.5;

            (
                lerp(px, group.0, ease_in_out(user)) + wob,
                lerp(DOT_HEIGHT as f32 - 1.0, group.1, up),
            )
        }
        Family::Ripple => {
            // the dot appears when the front passes, a radial bump after
            let output = if value == 4 {
                (CENTER.0, DOT_HEIGHT as f32)
            } else {
                CENTER
            };

            let speed = if value == 3 { 34.0 } else { 18.0 };
            let (dx, dy) = (group.0 - output.0, group.1 - output.1);
            let directory = dx.hypot(dy);
            let front = tau * speed;

            if front < directory {
                return None;
            }

            let bump = 1.6 * ((front - directory) * 0.5).sin() * (-(front - directory) * 0.18).exp() * (1.0 - user);
            let line = directory.max(0.01);

            (group.0 + dx / line * bump, group.1 + dy / line * bump)
        }
        Family::Wave => path_wave(value, user, tau, group, lerp),
        Family::Condense => match path_condense(value, user, tau, group, (r0, r1, r2), lerp) {
            Ok(value) => value,
            Err(value) => return value,
        },
        Family::Drip => drip_path(value, user, group, lerp),
        Family::Merge => return None,
    })
}

fn path_vortex(value: usize, user: f32, group: (f32, f32), r0: f32, lerp: impl Fn(f32, f32, f32) -> f32) -> (f32, f32) {
    let (dx, dy) = (group.0 - CENTER.0, group.1 - CENTER.1);
    let (rg, ag) = (dx.hypot(dy), dy.atan2(dx));

    let spin = match value {
        1 => 9.0,
        2 => -5.0,
        _ => 5.0,
    };

    let result = if value == 3 {
        lerp(0.0, rg, ease_out(user))
    } else {
        lerp(15.0 + r0 * 4.0, rg, ease_out(user))
    };

    let agent_2 = ag + spin * (1.0 - ease_out(user));
    let sy = if value == 4 { 0.45 + 0.55 * user } else { 1.0 };

    (
        CENTER.0 + agent_2.cos() * result,
        CENTER.1 + agent_2.sin() * result * sy,
    )
}

fn path_condense(
    value: usize,
    user: f32,
    tau: f32,
    group: (f32, f32),
    (r0, r1, r2): (f32, f32, f32),
    lerp: impl Fn(f32, f32, f32) -> f32,
) -> Result<(f32, f32), Option<(f32, f32)>> {
    let start = match value {
        1 => (r0 * DOT_WIDTH as f32, r1 * 6.0 - 4.0),
        2 => {
            let agent_3 = r0 * std::f32::consts::TAU;

            (CENTER.0 + agent_3.cos() * 13.0, CENTER.1 + agent_3.sin() * 11.0)
        }
        _ => (r0 * DOT_WIDTH as f32, r1 * DOT_HEIGHT as f32),
    };

    if value == 3 && r2 < 0.5 && user < 0.6 {
        return Err(None);
    }

    let drift = (
        (tau * 1.7 + r2 * 6.0).sin() * 2.5,
        (tau * 1.3 + r0 * 5.0).cos() * 2.0 + if value == 1 { tau * 3.0 } else { 0.0 },
    );
    let key_3 = if value == 4 { user.powi(5) } else { user.powi(3) };

    Ok((
        lerp(start.0 + drift.0, group.0, key_3),
        lerp(start.1 + drift.1, group.1, key_3),
    ))
}

fn path_wave(value: usize, user: f32, tau: f32, group: (f32, f32), lerp: impl Fn(f32, f32, f32) -> f32) -> (f32, f32) {
    let amp = if value == 4 { 7.0 } else { 3.5 };
    let text_value = (group.0 * 0.45 - tau * 9.0).sin() * amp * (1.0 - ease_out(user));

    match value {
        0 | 4 => (lerp(-3.0, group.0, ease_out(user)), group.1 + text_value),
        1 => (
            lerp(DOT_WIDTH as f32 + 2.0, group.0, ease_out(user)),
            group.1 + text_value,
        ),
        2 => (
            lerp(
                if group.0 < CENTER.0 {
                    -3.0
                } else {
                    DOT_WIDTH as f32 + 2.0
                },
                group.0,
                ease_out(user),
            ),
            group.1 + text_value,
        ),
        _ => (
            group.0 + text_value * 0.6,
            lerp(DOT_HEIGHT as f32 + 2.0, group.1, ease_out(user)),
        ),
    }
}

fn path_rain(value: usize, user: f32, group: (f32, f32), r1: f32, r2: f32) -> (f32, f32) {
    let fall = 8.0 + r1 * 14.0;
    let wind = if value == 1 { -7.0 } else { 0.0 };
    let hit = 0.7;

    if user < hit {
        let key_2 = (user / hit).powi(2);

        (group.0 + wind * (1.0 - key_2), group.1 - fall * (1.0 - key_2))
    } else {
        let bytes_value = (user - hit) / (1.0 - hit);
        let hb = if value == 2 { 0.0 } else { 2.5 + r2 * 1.5 };

        (
            group.0,
            group.1 - hb * (bytes_value * std::f32::consts::PI * 2.0).sin().abs() * (1.0 - bytes_value),
        )
    }
}

fn rise_start_column(group: (f32, f32)) -> f32 {
    if group.0 < CENTER.0 {
        3.0
    } else {
        DOT_WIDTH as f32 - 4.0
    }
}

fn fountain_path(
    value: usize,
    user: f32,
    group: (f32, f32),
    r0: f32,
    r1: f32,
    lerp: impl Fn(f32, f32, f32) -> f32,
) -> (f32, f32) {
    let sx = match value {
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

    let agent_value = match value {
        2 => 22.0,
        3 => 7.0,
        _ => 14.0,
    };

    (
        lerp(sx, group.0, user),
        lerp(DOT_HEIGHT as f32 + 1.0, group.1, user) - agent_value * 4.0 * user * (1.0 - user),
    )
}

fn splash_path(value: usize, user: f32, group: (f32, f32)) -> (f32, f32) {
    let text = match value {
        3 => {
            if group.0 < CENTER.0 {
                (CENTER.0 - 5.0, CENTER.1 - 2.0)
            } else {
                (CENTER.0 + 5.0, CENTER.1 - 2.0)
            }
        }
        4 => (CENTER.0, DOT_HEIGHT as f32 + 1.0),
        _ => CENTER,
    };

    let (key_value, width) = match value {
        1 => (3.0, 3.5),
        2 => (7.0, 1.6),
        _ => (4.5, 2.6),
    };

    let error = (-key_value * user).exp() * (std::f32::consts::PI * width * user).cos() * (1.0 - user);

    (
        group.0 + (text.0 - group.0) * error,
        group.1 + (text.1 - group.1) * error,
    )
}

fn drip_path(value: usize, user: f32, group: (f32, f32), lerp: impl Fn(f32, f32, f32) -> f32) -> (f32, f32) {
    let tap = match value {
        1 => 2.0,
        2 => {
            if group.0 < CENTER.0 {
                CENTER.0 - 6.0
            } else {
                CENTER.0 + 6.0
            }
        }
        _ => CENTER.0,
    };

    if user < 0.55 {
        let key_4 = (user / 0.55).powi(2);

        (tap, lerp(-1.0, group.1, key_4))
    } else {
        let key_5 = ease_out((user - 0.55) / 0.45);

        let hop = if value == 4 {
            2.0 * (key_5 * std::f32::consts::PI).sin()
        } else {
            0.0
        };

        (lerp(tap, group.0, key_5), group.1 - hop)
    }
}
/// Ripple rings: the fronts as circles of dots.
pub(crate) fn ripples(fam: Family, value: usize, tau: f32, put: &mut impl FnMut(f32, f32, bool)) {
    if fam != Family::Ripple {
        return;
    }

    let output = if value == 4 {
        (CENTER.0, DOT_HEIGHT as f32)
    } else {
        CENTER
    };

    let speed = if value == 3 { 34.0 } else { 18.0 };

    let rings = match value {
        1 => 2,
        2 => 3,
        _ => 1,
    };

    for key in 0..rings {
        let result = (tau - key as f32 * 0.22) * speed;

        if !(0.5..20.0).contains(&result) {
            continue;
        }

        let count = (result * 4.0) as usize;

        for j in 0..count {
            let agent = std::f32::consts::TAU * j as f32 / count as f32;

            put(
                output.0 + agent.cos() * result,
                output.1 + agent.sin() * result * 0.9,
                true,
            );
        }
    }
}
/// Droplets thrown off as the liquid lands: ballistic, gone after 0.5 s.
pub(crate) fn spray(fam: Family, value: usize, tau: f32, end: f32, i: usize, put: &mut impl FnMut(f32, f32, bool)) {
    let (count, at, from) = match (fam, value) {
        (Family::Fountain, _) => (10, end * 0.55, (CENTER.0, CENTER.1 + 4.0)),
        (Family::Splash, _) => (14, 0.05, CENTER),
        (Family::Rain, 2) => (16, end * 0.6, (CENTER.0, CENTER.1 + 6.0)),
        (Family::Drip, 4 | 0) => (8, end * 0.5, (CENTER.0, CENTER.1)),
        (Family::Rise, 1) => (8, end * 0.75, (CENTER.0, CENTER.1 - 6.0)),
        (Family::Merge, 3) => (12, end * 0.4, CENTER),
        _ => return,
    };

    for key in 0..count {
        let text = tau - at - (key % 3) as f32 * 0.06;

        // gone before the snap: 0 droplets over the sprite's first frame
        if !(0.0..0.5).contains(&text) || tau > end - 0.05 {
            continue;
        }

        let height = (i * 97 + key) as u64 * 31;
        let agent = -std::f32::consts::PI * (0.1 + 0.8 * hash(height));
        let sp = 14.0 + 16.0 * hash(height + 1);

        put(
            from.0 + agent.cos() * sp * text,
            from.1 + agent.sin() * sp * text + 40.0 * text * text,
            true,
        );
    }
}
