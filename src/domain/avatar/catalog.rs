use std::ops::ControlFlow;

use super::{
    Kind, MORE_SOLIDS, MOTIONS, Motion, NEW_SOLIDS, PATTERNS, Pattern, Regi, SOLIDS, Solid, TALKS, Talk, YAWS, hash,
};

/// The kept ones of the 1st sets (by their old number), then the new angular solids, then the
/// 3rd round. Out: the flat frames, the obelisks, the arches.
pub(crate) fn catalog() -> Vec<Regi> {
    let mut out = Vec::new();

    // the phase seed, as before the flat frames went out (the kept ones keep their phase)
    let mut phase_seed = 10u64;

    // 3d: 1 per solid and pattern of the regis, the dropped ones out (old 032, 039-044, 051-053)

    let count = 0;

    append_original_solids(&mut out, &mut phase_seed, count);

    // new: 2 per angular solid, the sprite patterns spread over them; kept: old 066, 076-080
    append_new_solids(out, phase_seed)
}

fn append_original_solids(out: &mut Vec<Regi>, phase_seed: &mut u64, mut count: usize) {
    for index in 0..SOLIDS.len() * 3 {
        let (si, pi) = (index / 3, index % 3);
        let (solid, pattern) = (SOLIDS[si], PATTERNS[pi]);

        if let ControlFlow::Break(()) = skip_disk_pattern(solid, pattern) {
            continue;
        }

        let idx = si * 3 + pi;

        count += 1;

        if let ControlFlow::Break(()) = skip_original_variant(count) {
            continue;
        }

        *phase_seed += 1;

        if let ControlFlow::Break(()) = skip_flat_solid(solid) {
            continue;
        }

        let kind = Kind::Solid(solid, MOTIONS[(si + pi * 2) % 5]);

        out.push(Regi {
            kind,
            pattern,
            talk: TALKS[(idx * 5 + si) % 8],
            blink: idx % 3,
            phase: hash(*phase_seed - 1) * 10.0,
            yaw: YAWS[(si + pi) % 4],
            star: false,
        });
    }
}

fn skip_flat_solid(solid: Solid) -> ControlFlow<()> {
    if matches!(solid, Solid::Obelisk | Solid::Arch) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn append_new_solids(mut out: Vec<Regi>, mut phase_seed: u64) -> Vec<Regi> {
    for index in 0..NEW_SOLIDS.len() * 2 {
        let (si, j) = (index / 2, index % 2);
        let solid = NEW_SOLIDS[si];
        let idx = si * 2 + j;

        if let ControlFlow::Break(()) = skip_new_variant(idx) {
            continue;
        }

        let kind = Kind::Solid(solid, MOTIONS[(si + j * 3 + 1) % 5]);
        let pattern = PATTERNS[(idx * 7 + 3) % 11];

        out.push(Regi {
            kind,
            pattern,
            talk: TALKS[(idx * 3 + j) % 8],
            blink: idx % 3,
            phase: hash(phase_seed) * 10.0,
            yaw: YAWS[(si + j) % 4],
            star: false,
        });
        phase_seed += 1;
    }

    // 3rd round: a star, an ellipse, a wide trapezoid
    // (eyes, motion, talk, turn)
    let more = [
        (Pattern::Steel, Motion::Nod, Talk::Chase, 0.3),
        (Pattern::Steel, Motion::Hover, Talk::Morse, 0.35),
        (Pattern::Rock, Motion::Nod, Talk::Halves, 0.0),
    ];

    for (si, (&solid, (pattern, motion, talk, yaw))) in MORE_SOLIDS.iter().zip(more).enumerate() {
        out.push(Regi {
            kind: Kind::Solid(solid, motion),
            pattern,
            talk,
            blink: si % 3,
            phase: hash(phase_seed) * 10.0,
            yaw,
            star: false,
        });
        phase_seed += 1;
    }

    out
}

fn skip_new_variant(idx: usize) -> ControlFlow<()> {
    if ![9, 19, 20, 21, 22, 23].contains(&idx) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn skip_original_variant(count: usize) -> ControlFlow<()> {
    const DROP: [usize; 10] = [0, 7, 8, 9, 10, 11, 12, 19, 20, 21];

    if DROP.contains(&(count - 1)) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn skip_disk_pattern(solid: Solid, pattern: Pattern) -> ControlFlow<()> {
    if solid == Solid::Disk && pattern == Pattern::Ice {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}
pub(crate) fn all() -> &'static [Regi] {
    static CATALOG: std::sync::OnceLock<Vec<Regi>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(catalog)
}
