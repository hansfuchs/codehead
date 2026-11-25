use super::{BG, CORAL, INK, MUTED, Pattern, Talk, WARN, mix};

impl Pattern {
    pub(crate) fn dots(self) -> &'static [(f32, f32)] {
        match self {
            // as tight as the rows: a 3x3 square, not a wide H
            Pattern::Rock => ROCK_DOTS,
            Pattern::Ice => ICE_DOTS,
            Pattern::Steel => STEEL_DOTS,
            Pattern::Cross4 => CROSS4_DOTS,
            Pattern::Tee => TEE_DOTS,
            Pattern::Vee => VEE_DOTS,
            Pattern::Ay => AY_DOTS,
            Pattern::Bar => BAR_DOTS,
            Pattern::Columns => COLUMNS_DOTS,
            Pattern::Pillars => PILLARS_DOTS,
            Pattern::Center => CENTER_DOTS,
            Pattern::InvCross => INV_CROSS_DOTS,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Pattern::Rock => "rock H",
            Pattern::Ice => "ice cross",
            Pattern::Steel => "steel hexagon",
            Pattern::Cross4 => "cross 4 rows (008)",
            Pattern::Tee => "T (014)",
            Pattern::Vee => "V (016)",
            Pattern::Ay => "A (017)",
            Pattern::Bar => "bar 5 (020)",
            Pattern::Columns => "columns (022)",
            Pattern::Pillars => "twin pillars (034)",
            Pattern::Center => "center column (076)",
            Pattern::InvCross => "inverted cross",
        }
    }
}
impl Talk {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Talk::Sweep => "sweep",
            Talk::Chatter => "chatter",
            Talk::CenterOut => "center out",
            Talk::Chase => "chase",
            Talk::Morse => "morse",
            Talk::Rows => "rows",
            Talk::Halves => "halves",
            Talk::Stutter => "stutter",
        }
    }
}
pub(crate) fn hash(count: u64) -> f32 {
    let mut x = count.wrapping_mul(0x9E37_79B9_7F4A_7C15);

    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x & 0xFFFF) as f32 / 65535.0
}
/// Working: dot K of TALK off at time T. Every dot blinks on its own: per slot (its own length
/// and phase) a chance of 1 short blink at a random point, so the pattern stays in view and single
/// dots wink in a pseudo random rhythm. TALK sets the tempo and how often.
pub(crate) fn blinks(talk: Talk, key: usize, thread: f32) -> bool {
    // (slot seconds, blink chance per slot, blink seconds)
    let (slot, chance, dur) = match talk {
        Talk::Sweep => (0.55, 0.16, 0.11),
        Talk::Chatter => (0.38, 0.14, 0.08),
        Talk::CenterOut => (0.6, 0.15, 0.12),
        Talk::Chase => (0.45, 0.16, 0.09),
        Talk::Morse => (0.5, 0.2, 0.07),
        Talk::Rows => (0.7, 0.16, 0.13),
        Talk::Halves => (0.48, 0.15, 0.1),
        Talk::Stutter => (0.42, 0.22, 0.06),
    };

    blink(
        (talk as u64 + 1) * 7919 + key as u64 * 104_729,
        slot,
        chance,
        dur,
        thread,
    )
}
/// Thinking: dot K of TALK off at time T. As `blinks`, slower: longer slots, longer blinks
/// (a slow, heavy-lidded wink per dot), own phases per talk.
pub(crate) fn blinks_slow(talk: Talk, key: usize, thread: f32) -> bool {
    blink((talk as u64 + 41) * 7919 + key as u64 * 104_729, 1.5, 0.3, 0.26, thread)
}
/// 1 dot of seed SEED off at time T: per slot (SLOT s, ±30 % per dot, its own phase) CHANCE of 1
/// blink of DUR s at a random point of the slot.
pub(crate) fn blink(seed: u64, slot: f32, chance: f32, dur: f32, thread: f32) -> bool {
    let len = slot * (0.7 + 0.6 * hash(seed));
    let user = thread + hash(seed + 1) * len;
    let j = (user / len).floor() as u64;
    let at = user - j as f32 * len;
    let text = seed.wrapping_mul(31).wrapping_add(j);

    if hash(text * 3 + 2) >= chance {
        return false;
    }

    let start = hash(text * 3 + 3) * (len - dur).max(0.0);

    (start..start + dur).contains(&at)
}
/// Dot K of PAT on at time T, status S (0 working, 1 thinking, 2 idle, 3 blocked, 4 done).
/// BLINK: the idle blink style (0 single, 1 double, 2 row by row).
pub(crate) fn lit(pat: Pattern, talk: Talk, blink: usize, text: usize, thread: f32, key: usize) -> bool {
    let directory = pat.dots();
    let (x, y) = directory[key];
    let center = x == 0.0 && y == 0.0;

    match text {
        0 => !blinks(talk, key, thread),
        1 => !blinks_slow(talk, key, thread),
        2 => {
            let path = 2.6 + (blink as f32) * 0.7;
            let user = thread.rem_euclid(path);

            match blink % 3 {
                0 => user > 0.13,
                1 => !(user < 0.1 || (0.22..0.32).contains(&user)),
                _ => !(user < 0.48 && (user / 0.12) as usize == (y + 1.5) as usize),
            }
        }
        3 => center || ((thread * 4.0) as usize).is_multiple_of(2) == (x <= 0.0),
        // done: the center (a pattern without one: its middle column, else its 1st dot)
        _ => {
            center
                || (!directory.contains(&(0.0, 0.0))
                    && if directory.iter().any(|path_value| path_value.0 == 0.0) {
                        x == 0.0
                    } else {
                        key == 0
                    })
        }
    }
}
/// Eye, frame color of status S.
pub(crate) fn colors(text: usize) -> (crate::domain::avatar::glyph::Rgb, crate::domain::avatar::glyph::Rgb) {
    match text {
        0 => (CORAL, mix(MUTED, CORAL, 0.5)),
        3 => (WARN, WARN),
        4 => (MUTED, mix(BG, MUTED, 0.7)),
        _ => (INK, MUTED),
    }
}
pub(crate) fn braille(bits: u32) -> char {
    if bits == 0 {
        ' '
    } else {
        char::from_u32(0x2800 + bits).unwrap_or(' ')
    }
}

const ROCK_DOTS: &[(f32, f32)] = &[
    (-0.5, -1.0),
    (0.5, -1.0),
    (-0.5, 0.0),
    (0.0, 0.0),
    (0.5, 0.0),
    (-0.5, 1.0),
    (0.5, 1.0),
];
const ICE_DOTS: &[(f32, f32)] = &[
    (0.0, -1.0),
    (-1.0, 0.0),
    (-0.5, 0.0),
    (0.0, 0.0),
    (0.5, 0.0),
    (1.0, 0.0),
    (0.0, 1.0),
];
const STEEL_DOTS: &[(f32, f32)] = &[
    (-0.5, -1.0),
    (0.5, -1.0),
    (-1.0, 0.0),
    (0.0, 0.0),
    (1.0, 0.0),
    (-0.5, 1.0),
    (0.5, 1.0),
];
const CROSS4_DOTS: &[(f32, f32)] = &[
    (0.0, -1.5),
    (-1.0, -0.5),
    (-0.5, -0.5),
    (0.0, -0.5),
    (0.5, -0.5),
    (1.0, -0.5),
    (0.0, 0.5),
    (0.0, 1.5),
];
const TEE_DOTS: &[(f32, f32)] = &[
    (-1.0, -1.0),
    (-0.5, -1.0),
    (0.0, -1.0),
    (0.5, -1.0),
    (1.0, -1.0),
    (0.0, 0.0),
    (0.0, 1.0),
];
const VEE_DOTS: &[(f32, f32)] = &[(-1.0, -1.0), (1.0, -1.0), (-0.5, 0.0), (0.5, 0.0), (0.0, 1.0)];
const AY_DOTS: &[(f32, f32)] = &[(0.0, -1.0), (-0.5, 0.0), (0.5, 0.0), (-1.0, 1.0), (1.0, 1.0)];
const BAR_DOTS: &[(f32, f32)] = &[(-1.0, 0.0), (-0.5, 0.0), (0.0, 0.0), (0.5, 0.0), (1.0, 0.0)];
const COLUMNS_DOTS: &[(f32, f32)] = &[
    (-1.0, -1.5),
    (1.0, -1.5),
    (-1.0, -0.5),
    (0.0, -0.5),
    (1.0, -0.5),
    (-1.0, 0.5),
    (0.0, 0.5),
    (1.0, 0.5),
    (-1.0, 1.5),
    (1.0, 1.5),
];
const PILLARS_DOTS: &[(f32, f32)] = &[
    (-1.0, -1.0),
    (1.0, -1.0),
    (-1.0, 0.0),
    (1.0, 0.0),
    (-1.0, 1.0),
    (1.0, 1.0),
];
const CENTER_DOTS: &[(f32, f32)] = &[
    (0.0, -1.5),
    (0.0, -0.5),
    (0.0, 0.5),
    (-1.0, 1.5),
    (0.0, 1.5),
    (1.0, 1.5),
];
const INV_CROSS_DOTS: &[(f32, f32)] = &[
    (0.0, -1.5),
    (0.0, -0.5),
    (-1.0, 0.5),
    (-0.5, 0.5),
    (0.0, 0.5),
    (0.5, 0.5),
    (1.0, 0.5),
    (0.0, 1.5),
];
