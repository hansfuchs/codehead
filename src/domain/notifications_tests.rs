use super::*;
use serde_json::json;

fn ev(agent: &str, ts: u64, kind: Ev) -> Event {
    Event {
        ts,
        agent: (!agent.is_empty()).then(|| agent.to_string()),
        kind,
    }
}

fn status(agent: &str, ts: u64, text: &str) -> Event {
    ev(
        agent,
        ts,
        Ev::Status {
            state: text.into(),
            note: String::new(),
        },
    )
}

fn kinds(notes: &[(Kind, Vec<String>, u64, String)]) -> Vec<Kind> {
    notes.iter().map(|note| note.0).collect()
}

#[test]
fn done_only_when_every_agent_is_done() {
    let members = vec!["ada".to_string(), "bob".to_string()];
    let mut events = vec![ev("ada", 1, Ev::Join), ev("bob", 1, Ev::Join), status("ada", 2, "done")];

    assert!(!kinds(&derive(&events, &members)).contains(&Kind::Done));

    events.push(status("bob", 3, "done"));
    let notes = derive(&events, &members);

    assert_eq!(kinds(&notes), vec![Kind::Done]);
    assert_eq!((notes[0].1.len(), notes[0].2), (2, 3));

    events.push(ev("ada", 4, Ev::TurnStart));

    assert!(derive(&events, &members).is_empty(), "a turn after done: working again");
}

/// review: a step note, never done (the thread is not shipped); a turn after it clears it.
#[test]
fn review_is_a_step_not_done() {
    let members = vec!["ada".to_string()];
    let mut events = vec![status("ada", 1, "review")];

    assert_eq!(kinds(&derive(&events, &members)), vec![Kind::Step]);

    events.push(ev("ada", 2, Ev::TurnStart));
    events.push(status("ada", 3, "done"));

    assert_eq!(kinds(&derive(&events, &members)), vec![Kind::Done]);
}

#[test]
fn blocked_and_asks_drop_once_resolved() {
    let members = vec!["ada".to_string()];
    let mut events = vec![
        status("ada", 1, "blocked"),
        ev(
            "ada",
            2,
            Ev::Ask {
                id: "q".into(),
                tool: "AskUserQuestion".into(),
                input: json!({"question": "which db?"}),
            },
        ),
    ];

    assert_eq!(kinds(&derive(&events, &members)), vec![Kind::Ask, Kind::Blocked]);

    events.push(ev(
        "",
        3,
        Ev::Answer {
            id: "q".into(),
            allow: true,
            label: "pg".into(),
        },
    ));
    events.push(ev("ada", 4, Ev::TurnStart));

    assert_eq!(
        derive(&events, &members),
        [] as [(
            crate::domain::notifications::Kind,
            std::vec::Vec<std::string::String>,
            u64,
            std::string::String
        ); 0]
    );
}

#[test]
fn limit_gate_context_mention() {
    let members = vec!["ada".to_string()];
    let end = |ctx| Ev::TurnEnd {
        cost: 0.0,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        ctx,
        window: 100,
        ms: 0,
        error: None,
    };
    let events = vec![
        ev("ada", 1, end(95)),
        ev("ada", 2, Ev::Resume { at: 9_000 }),
        ev(
            "",
            3,
            Ev::Flow {
                op: "wait".into(),
                text: "⏸ gate plan approval".into(),
            },
        ),
        ev(
            "ada",
            4,
            Ev::Text {
                text: "@owner please check the plan".into(),
            },
        ),
    ];

    verify_limit_gate_context_mention(&members, end, &events);
}

fn verify_limit_gate_context_mention(members: &[String], end: impl Fn(u64) -> Ev, events: &[Event]) {
    assert_eq!(
        kinds(&derive(events, members)),
        vec![Kind::Gate, Kind::Limit, Kind::Mention, Kind::Context]
    );

    let mut answered = events.to_owned();

    answered.push(ev(
        "",
        5,
        Ev::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "ok".into(),
        },
    ));
    answered.push(ev(
        "",
        6,
        Ev::Flow {
            op: "answer".into(),
            text: String::new(),
        },
    ));
    answered.push(ev("ada", 7, Ev::TurnStart));
    answered.push(ev("ada", 8, end(10)));

    assert_eq!(
        derive(&answered, members),
        [] as [(
            crate::domain::notifications::Kind,
            std::vec::Vec<std::string::String>,
            u64,
            std::string::String
        ); 0]
    );
    assert!(!mentions_owner("@owners"));
}

#[test]
fn errors_skip_usage_limits_and_clear_on_owner_message() {
    let members = vec!["ada".to_string()];
    let mut events = vec![ev("ada", 1, Ev::Error { message: "boom".into() })];

    assert_eq!(kinds(&derive(&events, &members)), vec![Kind::Error]);

    events.push(ev(
        "",
        2,
        Ev::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "retry".into(),
        },
    ));

    assert_eq!(
        derive(&events, &members),
        [] as [(
            crate::domain::notifications::Kind,
            std::vec::Vec<std::string::String>,
            u64,
            std::string::String
        ); 0]
    );
}

#[test]
fn reply_notes_hold_past_the_seen_mark_until_the_reply_is_seen() {
    let mut thread = store::Thread::new("replies", "x", std::path::Path::new("/tmp"), vec!["REV".into()]);

    thread.id = format!("notify-replies-{}", std::process::id());
    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));

    crate::domain::changes::agent_comments(
        &thread.id,
        "REV",
        None,
        &serde_json::json!([{"file": "a.rs", "line": 3, "severity": "low", "text": "nit"}]),
    )
    .unwrap();

    assert!(reply_notes(&thread).is_empty(), "a new comment: 0 reply note");

    crate::domain::changes::threads::reply(&thread.id, "REV", "c1", "done in a.rs:4", false).unwrap();

    let notes = reply_notes(&thread);

    assert_eq!(notes.len(), 1);
    assert_eq!(
        (
            notes[0].kind,
            notes[0].comment.as_str(),
            notes[0].agents.clone(),
            notes[0].text.as_str()
        ),
        (Kind::Reply, "c1", vec!["REV".to_string()], "c1 a.rs: done in a.rs:4")
    );

    // the chat's seen mark past it: the reply note stays (0 chat row), out of the chat's ↓ count
    let done = Note {
        kind: Kind::Done,
        comment: String::new(),
        ..notes[0].clone()
    };

    *NOTES.lock().unwrap() = vec![notes[0].clone(), done];
    mark_seen(&thread.id, notes[0].ts + 1);
    let kinds: Vec<Kind> = unseen(None)
        .into_iter()
        .filter(|note| note.thread == thread.id)
        .map(|note| note.kind)
        .collect();

    assert_eq!(kinds, vec![Kind::Reply]);
    assert_eq!(unseen_of(&thread.id), [] as [crate::domain::notifications::Note; 0]);

    // seen in the changes view: 0 note
    assert!(crate::domain::changes::threads::mark_seen(&thread.id, "c1"));
    assert_eq!(reply_notes(&thread), [] as [crate::domain::notifications::Note; 0]);

    NOTES.lock().unwrap().clear();
    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}
