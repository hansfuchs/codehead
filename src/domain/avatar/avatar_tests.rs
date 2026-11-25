use super::*;

#[test]
fn ids_round_trip() {
    let geo = Avatar {
        cat: Cat::Geo,
        idx: 40,
        eyes: None,
        speed: 2.0,
    };

    assert_eq!(geo.id(), "geo:40 speed=2");
    assert_eq!(Avatar::parse(&geo.id()), Some(geo));

    let outline = Avatar {
        cat: Cat::GeoB,
        idx: 7,
        eyes: None,
        speed: 1.0,
    };

    assert_eq!(outline.id(), "geob:7");
    assert_eq!(Avatar::parse(&outline.id()), Some(outline));

    let regi = Avatar {
        cat: Cat::Regi,
        idx: 3,
        eyes: Some(5),
        speed: 0.5,
    };

    assert_eq!(Avatar::parse(&regi.id()), Some(regi));
    assert_eq!(Avatar::parse("pcb-042"), None);
    assert_eq!(Avatar::parse("regi:99999"), None);
}

#[test]
fn every_avatar_draws_7x4() {
    for cat in CATS {
        for idx in 0..cat.count() {
            let rows = Avatar {
                cat,
                idx,
                eyes: None,
                speed: 1.0,
            }
            .rows(Status::Working, 1.3);

            assert!(rows.iter().all(|row| row.chars().count() == WIDTH), "{cat:?} {idx}");
        }
    }
}

#[test]
fn spawns_at_once_differ() {
    let mut taken = Vec::new();

    for _ in 0..spawn::count() {
        let spawn = Spawn::random_except(&taken);

        assert!(!taken.contains(&spawn.anim));

        taken.push(spawn.anim);
    }
}

#[test]
fn a_spawn_ends_on_the_first_frame_of_the_status() {
    // the goo's last frame before the snap = the status animation's frame at the snap: 0 jump
    let blank = |ch: char| ch == ' ' || ch == '\u{2800}';

    for (cat, idx, eyes) in [
        (Cat::Regi, 0, None),
        (Cat::Regi, 7, Some(3)),
        (Cat::Regi, 21, None),
        (Cat::Geo, 2, None),
    ] {
        let avatar = Avatar {
            cat,
            idx,
            eyes,
            speed: 1.0,
        };

        for status in [
            Status::Idle,
            Status::Working,
            Status::Thinking,
            Status::Blocked,
            Status::Done,
        ] {
            assert_spawns_end_on_status_frame(blank, cat, idx, avatar, status);
        }
    }
}

fn assert_spawns_end_on_status_frame(
    blank: impl Fn(char) -> bool,
    cat: Cat,
    idx: usize,
    avatar: Avatar,
    status: Status,
) {
    for anim in 0..spawn::count() {
        let sp = Spawn { anim, start: 3.7 };
        let end = spawn::timing(anim).2;
        let target = sp.target(&avatar, status);

        assert_eq!(target, avatar.cells(status, sp.start + end));

        let goo = spawn::render(anim, &target, &|_| avatar.cells(status, sp.start + end), end - 0.01);
        let (ax, ay) = SPAWN_AT;

        let off = (0..WIDTH * HEIGHT).filter(|&cell| {
            let (formed, expected) = (
                goo[(ay + cell / WIDTH) * SPAWN_W + ax + cell % WIDTH].ch,
                target[cell].ch,
            );

            blank(formed) != blank(expected)
        });

        assert_eq!(
            off.count(),
            0,
            "{cat:?} {idx} {status:?} anim {anim} ({})",
            spawn::name(anim)
        );
    }
}

#[test]
fn a_spawn_ends() {
    let ended = Spawn {
        anim: 7,
        start: now() - 10.0,
    };

    assert!(ended.done() && ended.rows(&Avatar::default(), Status::Idle).is_none());

    let started = Spawn { anim: 7, start: now() };

    assert!(started.rows(&Avatar::default(), Status::Idle).is_some());
    assert_eq!(started.reveal(), 0.0);
}
