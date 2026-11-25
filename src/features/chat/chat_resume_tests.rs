use super::*;
use crate::engine::store::Event;

#[test]
fn the_last_limit_note_counts_down_until_a_message_or_a_turn() {
    let mut thread = Thread::default();
    let ev = |agent: Option<&str>, kind: Kind| Event {
        ts: 1,
        agent: agent.map(String::from),
        kind,
    };
    let end = |err: &str| Kind::TurnEnd {
        cost: 0.0,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        ctx: 0,
        window: 0,
        ms: 0,
        error: Some(err.into()),
    };

    thread.push(&ev(Some("ada"), end("You've hit your 5-hour limit · resets 1:50pm")));
    thread.push(&ev(Some("ada"), end("You've hit your 5-hour limit · resets 6:50pm")));
    let at = crate::engine::now_ms() + 12 * 60_000 + 30_000;

    thread.push(&ev(Some("ada"), Kind::Resume { at }));
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };

    verify_the_last_limit_note_counts_down_until_a_message_or_a_turn(thread, ev, fold);
}

fn verify_the_last_limit_note_counts_down_until_a_message_or_a_turn(
    mut thread: Thread,
    ev: impl Fn(Option<&str>, Kind) -> Event,
    fold: Fold<'_>,
) {
    let roster = vec![("ada".to_string(), "claude".to_string())];
    let dump = text(&draw(
        &thread,
        Look::Bubbles,
        120,
        &Colors::new(),
        &roster,
        &fold,
        &Picker::default(),
    ));

    assert_eq!(
        dump.matches("Resumes automatically in 12m 30s").count()
            + dump.matches("Resumes automatically in 12m 29s").count(),
        1,
        "{dump}"
    );

    let last = dump.rfind("6:50pm").unwrap();

    assert!(
        dump.rfind("Resumes automatically").unwrap() > last,
        "the last note: {dump}"
    );

    // the owner writes: 0 resume
    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "go".into(),
        },
    ));

    assert!(thread.resumes.is_empty());
    assert_eq!(countdown(3_725_000), "1h 02m 05s");
}

fn text(drawn: &Drawn) -> String {
    drawn
        .rows
        .iter()
        .map(|row| {
            row.line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                + "\n"
        })
        .collect()
}
