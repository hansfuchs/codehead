use super::*;

#[test]
fn ids_round_trip() {
    let agent = Avatar {
        cat: Cat::Geo,
        idx: 40,
        eyes: None,
        speed: 2.0,
    };

    assert_eq!(agent.id(), "geo:40 speed=2");
    assert_eq!(Avatar::parse(&agent.id()), Some(agent));

    let group = Avatar {
        cat: Cat::GeoB,
        idx: 7,
        eyes: None,
        speed: 1.0,
    };

    assert_eq!(group.id(), "geob:7");
    assert_eq!(Avatar::parse(&group.id()), Some(group));

    let bytes = Avatar {
        cat: Cat::Regi,
        idx: 3,
        eyes: Some(5),
        speed: 0.5,
    };

    assert_eq!(Avatar::parse(&bytes.id()), Some(bytes));
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

            assert!(
                rows.iter().all(|result| result.chars().count() == WIDTH),
                "{cat:?} {idx}"
            );
        }
    }
}

#[test]
fn spawns_at_once_differ() {
    let mut taken = Vec::new();

    for _ in 0..spawn::count() {
        let text = Spawn::random_except(&taken);

        assert!(!taken.contains(&text.anim));

        taken.push(text.anim);
    }
}

#[test]
fn a_spawn_ends_on_the_first_frame_of_the_status() {
    // the goo's last frame before the snap = the status animation's frame at the snap: 0 jump
    let blank = |character: char| character == ' ' || character == '\u{2800}';

    for (cat, idx, eyes) in [
        (Cat::Regi, 0, None),
        (Cat::Regi, 7, Some(3)),
        (Cat::Regi, 21, None),
        (Cat::Geo, 2, None),
    ] {
        let agent = Avatar {
            cat,
            idx,
            eyes,
            speed: 1.0,
        };

        for text in [
            Status::Idle,
            Status::Working,
            Status::Thinking,
            Status::Blocked,
            Status::Done,
        ] {
            a_spawn_ends_on_the_first_frame_of_the_status_for_anim_in_0_spawn_count(blank, cat, idx, agent, text);
        }
    }
}

fn a_spawn_ends_on_the_first_frame_of_the_status_for_anim_in_0_spawn_count(
    blank: impl Fn(char) -> bool,
    cat: Cat,
    idx: usize,
    agent: Avatar,
    text: Status,
) {
    for anim in 0..spawn::count() {
        let sp = Spawn { anim, start: 3.7 };
        let end = spawn::timing(anim).2;
        let target = sp.target(&agent, text);

        assert_eq!(target, agent.cells(text, sp.start + end));

        let goo = spawn::render(anim, &target, &|_| agent.cells(text, sp.start + end), end - 0.01);
        let (ax, ay) = SPAWN_AT;

        let off = (0..WIDTH * HEIGHT).filter(|&key| {
            let (group, thread) = (goo[(ay + key / WIDTH) * SPAWN_W + ax + key % WIDTH].ch, target[key].ch);

            blank(group) != blank(thread)
        });

        assert_eq!(
            off.count(),
            0,
            "{cat:?} {idx} {text:?} anim {anim} ({})",
            spawn::name(anim)
        );
    }
}

#[test]
fn a_spawn_ends() {
    let text = Spawn {
        anim: 7,
        start: now() - 10.0,
    };

    assert!(text.done() && text.rows(&Avatar::default(), Status::Idle).is_none());

    let text_value = Spawn { anim: 7, start: now() };

    assert!(text_value.rows(&Avatar::default(), Status::Idle).is_some());
    assert_eq!(text_value.reveal(), 0.0);
}
