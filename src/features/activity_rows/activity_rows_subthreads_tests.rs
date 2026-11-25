use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn kid(id: &str) -> Kid {
    Kid {
        id: id.into(),
        title: format!("job {id}"),
        agents: vec!["ADA".into()],
        start: crate::engine::now_ms(),
        busy: true,
        asking: false,
        watches: 0,
        steps: vec!["ADA: Bash git status".into()],
    }
}

#[test]
fn task_of_reads_the_sub_prefix_only() {
    assert_eq!(task_of(">sub @ADA #commit").as_deref(), Some("@ADA #commit"));
    assert_eq!(
        task_of("  >sub  split the diff\nmore").as_deref(),
        Some("split the diff\nmore")
    );
    assert_eq!(task_of(">sub").as_deref(), Some(""));
    assert_eq!(task_of(">subway"), None);
    assert_eq!(task_of("go >sub x"), None);
}

#[test]
fn the_row_shows_running_ones_and_goes_with_the_last() {
    let mut subthreads = SubThreads::default();

    assert_eq!(subthreads.rows(80, 20), 0);

    subthreads.started(kid("a"));
    subthreads.started(kid("b"));

    assert_eq!(subthreads.rows(80, 20), 1);

    // a click on the row opens the box: title rows + steps, a blank row between
    subthreads.bar = Rect::new(0, 5, 80, 1);

    assert!(subthreads.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 5,
        modifiers: KeyModifiers::NONE
    }));

    let lines: Vec<String> = subthreads
        .lines(76)
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.to_string()).collect())
        .collect();

    assert!(
        lines[0].starts_with("› job a") && lines[0].contains("@ADA") && lines[1].contains("↳ ADA: Bash git status"),
        "{lines:?}"
    );
    assert_eq!(subthreads.rows(80, 20), 1 + 2 + 5);
    assert_eq!(
        subthreads.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        Some(Act::None)
    );
    assert_eq!(
        subthreads.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
        Some(Act::Stop("b".into()))
    );

    // the last one ends: the box closes, the row goes
    subthreads.gone("a");
    subthreads.gone("b");

    assert!(!subthreads.open && subthreads.rows(80, 20) == 0);
}

#[test]
fn an_open_ask_shows_and_enter_opens_the_sub_thread() {
    let ev = |kind: Kind| store::Event {
        ts: 0,
        agent: Some("ADA".into()),
        kind,
    };

    let ask = || {
        ev(Kind::Ask {
            id: "q1".into(),
            tool: "Bash".into(),
            input: serde_json::Value::Null,
        })
    };

    assert!(asking(&[ask()]));
    assert!(!asking(&[
        ask(),
        ev(Kind::Answer {
            id: "q1".into(),
            allow: true,
            label: String::new()
        })
    ]));

    let mut subthreads = SubThreads::default();

    subthreads.started(Kid {
        asking: true,
        ..kid("a")
    });
    subthreads.open = true;
    let lines: Vec<String> = subthreads
        .lines(90)
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.to_string()).collect())
        .collect();

    assert!(lines[0].contains("asks you · enter: open"), "{lines:?}");
    assert_eq!(
        subthreads.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Act::Open("a".into()))
    );
}

#[test]
fn a_sub_thread_that_waits_on_a_monitor_says_so() {
    let mut subthreads = SubThreads::default();

    subthreads.started(Kid {
        busy: false,
        watches: 1,
        ..kid("a")
    });
    subthreads.open = true;
    let lines: Vec<String> = subthreads
        .lines(90)
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.to_string()).collect())
        .collect();

    assert!(lines[0].contains("waits on 1 monitor"), "{lines:?}");
}

#[test]
fn steps_keep_the_last_texts_and_tools() {
    let ev = |agent: &str, kind: Kind| store::Event {
        ts: 0,
        agent: Some(agent.into()),
        kind,
    };

    let events: Vec<store::Event> = (0..6)
        .map(|i| {
            ev(
                "ADA",
                Kind::Text {
                    text: format!("step {i}\nmore"),
                },
            )
        })
        .chain([ev("ADA", Kind::TurnStart)])
        .collect();

    let kept = steps(&events);

    assert_eq!(kept.len(), STEPS);
    assert_eq!(kept.last().map(String::as_str), Some("ADA: step 5"));
}
