use super::*;
use crate::engine::store::{Event, Kind};
use ratatui::crossterm::event::KeyModifiers;

fn thread(texts: &[&str]) -> chat::Thread {
    let mut thread_value = chat::Thread::default();

    for (i, x) in texts.iter().enumerate() {
        thread_value.push(&Event {
            ts: i as u64 + 1,
            agent: Some("ada".into()),
            kind: Kind::Text { text: x.to_string() },
        });
    }

    thread_value
}

fn typed(text_value: &mut ChatSearch, thread: &chat::Thread, text: &str) -> Act {
    let mut last = Act::None;

    for character in text.chars() {
        last = text_value.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE), thread);
    }

    last
}

#[test]
fn matches_cycle_from_the_newest_and_wrap() {
    let thread_value = thread(&["deploy the app", "unrelated", "the deploy failed", "redeployed it"]);
    let mut text = ChatSearch::default();

    text.open();

    assert_eq!(
        typed(&mut text, &thread_value, "deploy"),
        Act::Show(3),
        "the newest match first"
    );
    assert_eq!(text.label(), "3/3");

    let older = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(text.key(older, &thread_value), Act::Show(2));
    assert_eq!(text.key(older, &thread_value), Act::Show(0));
    assert_eq!(text.key(older, &thread_value), Act::Show(3), "wraps");
    assert_eq!(
        text.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &thread_value),
        Act::Show(0)
    );
    assert!(text.is_hit(2) && !text.is_hit(1));
}

#[test]
fn zero_matches_label_and_esc_closes() {
    let thread_value = thread(&["hello"]);
    let mut text_value = ChatSearch::default();

    text_value.open();
    typed(&mut text_value, &thread_value, "zzz");

    assert_eq!(text_value.label(), "0 matches");
    assert_eq!(
        text_value.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &thread_value),
        Act::Close
    );
    assert!(!text_value.open && text_value.ta.text().is_empty());
}

#[test]
fn new_items_keep_the_shown_match() {
    let mut thread_value = thread(&["fix a", "fix b"]);
    let mut text_value = ChatSearch::default();

    text_value.open();
    typed(&mut text_value, &thread_value, "fix");
    text_value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &thread_value);

    assert_eq!(text_value.current(), Some(0));

    thread_value.push(&Event {
        ts: 9,
        agent: Some("ada".into()),
        kind: Kind::Text { text: "fix c".into() },
    });

    assert_eq!(text_value.refresh(&thread_value), None, "a new message: 0 jump");
    assert_eq!((text_value.current(), text_value.label().as_str()), (Some(0), "1/3"));
}

/// A rebuilt stream (the log's head in front) and items that grow in place: the matches
/// follow (0 stale text by index).
#[test]
fn a_rebuilt_stream_and_grown_items_keep_the_matches_right() {
    let thread_value = thread(&["fix it", "other"]);
    let mut text_value = ChatSearch::default();

    text_value.open();
    typed(&mut text_value, &thread_value, "fix");

    assert!(text_value.is_hit(0));

    // the head comes in front: the hit is item 2 now
    let thread_2 = thread(&["head 1", "head 2", "fix it", "other"]);

    text_value.reset(2);
    text_value.refresh(&thread_2);

    assert!(text_value.is_hit(2) && !text_value.is_hit(0), "{:?}", text_value.hits);
    assert_eq!(text_value.label(), "1/1");
    assert_eq!(text_value.current(), Some(2), "the shown match stays");

    // an open run further back than the last items gets a call: found
    let mut thread_3 = chat::Thread::default();
    let tool = |id: &str, file: &str| Event {
        ts: 1,
        agent: Some("ada".into()),
        kind: Kind::Tool {
            id: id.into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": file}),
        },
    };

    thread_3.push(&tool("1", "src/a.rs"));

    for i in 0..6 {
        thread_3.push(&Event {
            ts: 2,
            agent: Some("zed".into()),
            kind: Kind::Text {
                text: format!("zed {i}"),
            },
        });
    }

    let mut text_2 = ChatSearch::default();

    text_2.open();
    typed(&mut text_2, &thread_3, "special");

    assert_eq!(text_2.label(), "0 matches");

    thread_3.push(&tool("2", "src/special.rs"));
    text_2.refresh(&thread_3);

    assert!(text_2.is_hit(0), "{} items", thread_3.items.len());
}

#[test]
fn mark_splits_spans_at_the_hits() {
    let line = Line::from(vec![Span::raw("the De"), Span::raw("ploy ok")]);
    let message = mark(&line, &["deploy".into()]);
    let text: Vec<&str> = message
        .spans
        .iter()
        .map(|text_value| text_value.content.as_ref())
        .collect();

    assert_eq!(text, vec!["the ", "De", "ploy", " ok"]);
    assert!(message.spans[1].style.bg.is_some() && message.spans[0].style.bg.is_none());
}
