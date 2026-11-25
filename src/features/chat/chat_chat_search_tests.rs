use super::*;
use crate::engine::store::{Event, Kind};
use ratatui::crossterm::event::KeyModifiers;

fn thread_of(texts: &[&str]) -> chat::Thread {
    let mut thread = chat::Thread::default();

    for (i, text) in texts.iter().enumerate() {
        thread.push(&Event {
            ts: i as u64 + 1,
            agent: Some("ada".into()),
            kind: Kind::Text { text: text.to_string() },
        });
    }

    thread
}

fn typed(search: &mut ChatSearch, thread: &chat::Thread, text: &str) -> Act {
    let mut last = Act::None;

    for character in text.chars() {
        last = search.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE), thread);
    }

    last
}

#[test]
fn matches_cycle_from_the_newest_and_wrap() {
    let thread = thread_of(&["deploy the app", "unrelated", "the deploy failed", "redeployed it"]);
    let mut search = ChatSearch::default();

    search.open();

    assert_eq!(
        typed(&mut search, &thread, "deploy"),
        Act::Show(3),
        "the newest match first"
    );
    assert_eq!(search.label(), "3/3");

    let older = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(search.key(older, &thread), Act::Show(2));
    assert_eq!(search.key(older, &thread), Act::Show(0));
    assert_eq!(search.key(older, &thread), Act::Show(3), "wraps");
    assert_eq!(
        search.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &thread),
        Act::Show(0)
    );
    assert!(search.is_hit(2) && !search.is_hit(1));
}

#[test]
fn zero_matches_label_and_esc_closes() {
    let thread = thread_of(&["hello"]);
    let mut search = ChatSearch::default();

    search.open();
    typed(&mut search, &thread, "zzz");

    assert_eq!(search.label(), "0 matches");
    assert_eq!(
        search.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &thread),
        Act::Close
    );
    assert!(!search.open && search.ta.text().is_empty());
}

#[test]
fn new_items_keep_the_shown_match() {
    let mut thread = thread_of(&["fix a", "fix b"]);
    let mut search = ChatSearch::default();

    search.open();
    typed(&mut search, &thread, "fix");
    search.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &thread);

    assert_eq!(search.current(), Some(0));

    thread.push(&Event {
        ts: 9,
        agent: Some("ada".into()),
        kind: Kind::Text { text: "fix c".into() },
    });

    assert_eq!(search.refresh(&thread), None, "a new message: 0 jump");
    assert_eq!((search.current(), search.label().as_str()), (Some(0), "1/3"));
}

/// A rebuilt stream (the log's head in front) and items that grow in place: the matches
/// follow (0 stale text by index).
#[test]
fn a_rebuilt_stream_and_grown_items_keep_the_matches_right() {
    let thread = thread_of(&["fix it", "other"]);
    let mut search = ChatSearch::default();

    search.open();
    typed(&mut search, &thread, "fix");

    assert!(search.is_hit(0));

    // the head comes in front: the hit is item 2 now
    let rebuilt = thread_of(&["head 1", "head 2", "fix it", "other"]);

    search.reset(2);
    search.refresh(&rebuilt);

    assert!(search.is_hit(2) && !search.is_hit(0), "{:?}", search.hits);
    assert_eq!(search.label(), "1/1");
    assert_eq!(search.current(), Some(2), "the shown match stays");

    // an open run further back than the last items gets a call: found
    let mut with_run = chat::Thread::default();
    let tool = |id: &str, file: &str| Event {
        ts: 1,
        agent: Some("ada".into()),
        kind: Kind::Tool {
            id: id.into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": file}),
        },
    };

    with_run.push(&tool("1", "src/a.rs"));

    for i in 0..6 {
        with_run.push(&Event {
            ts: 2,
            agent: Some("zed".into()),
            kind: Kind::Text {
                text: format!("zed {i}"),
            },
        });
    }

    let mut run_search = ChatSearch::default();

    run_search.open();
    typed(&mut run_search, &with_run, "special");

    assert_eq!(run_search.label(), "0 matches");

    with_run.push(&tool("2", "src/special.rs"));
    run_search.refresh(&with_run);

    assert!(run_search.is_hit(0), "{} items", with_run.items.len());
}

#[test]
fn mark_splits_spans_at_the_hits() {
    let line = Line::from(vec![Span::raw("the De"), Span::raw("ploy ok")]);
    let marked = mark(&line, &["deploy".into()]);
    let pieces: Vec<&str> = marked.spans.iter().map(|span| span.content.as_ref()).collect();

    assert_eq!(pieces, vec!["the ", "De", "ploy", " ok"]);
    assert!(marked.spans[1].style.bg.is_some() && marked.spans[0].style.bg.is_none());
}
