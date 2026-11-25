use super::*;
use ratatui::backend::TestBackend;
use std::fmt::Write;

fn screen(view: &mut View) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..40)
        .map(|y| {
            (0..100)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn ev(agent: &str, ts: u64, kind: store::Kind) -> store::Event {
    store::Event {
        ts,
        agent: Some(agent.into()),
        kind,
    }
}

#[test]
fn a_click_on_an_attachment_opens_the_files_tab_on_it() {
    let dir = std::env::temp_dir().join(format!("codehead-attclick-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    let img = dir.join("shot.png");

    std::fs::write(&img, b"x").unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("selecttest-0001", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into()];
    let text = format!(
        "see [Image #1]\n\nAttachments (read these files):\n- [Image #1]: {}",
        img.display()
    );

    view.thread.push(&store::Event {
        ts: 1,
        agent: None,
        kind: store::Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text,
        },
    });

    // the marker in the body, then the 📎 row: each opens the files tab on the file
    for (needle, back) in [("[Image #1]", true), ("📎", false)] {
        let rows = screen(&mut view);
        let y = rows
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{rows:#?}"));
        let x = rows[y]
            .find(needle)
            .map(|byte| rows[y][..byte].chars().count())
            .unwrap() as u16;

        click(&mut view, x + 1, y as u16);

        assert_eq!(view.panes.pane, Pane::Files, "{needle}");
        assert!(view.panes.files.files.iter().any(|file| file.path == img), "{needle}");

        if back {
            view.set_pane(Pane::Chat);
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

fn start(id: &str, parent: Option<&str>, what: &str, background: bool) -> store::Kind {
    store::Kind::SubStart {
        id: id.into(),
        parent: parent.map(String::from),
        description: what.into(),
        agent_type: "Explore".into(),
        background,
        prompt: format!("do {what}"),
    }
}

fn click(view: &mut View, x: u16, y: u16) {
    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn running_subagents_show_grouped_by_master_and_stop_with_d() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("subtest-0000", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into(), "bob".into()];
    let now = crate::engine::now_ms();

    view.thread
        .push(&ev("ada", now, start("t1", None, "scan the repo", true)));
    view.thread
        .push(&ev("ada", now, start("t2", Some("t1"), "read the docs", false)));
    view.thread
        .push(&ev("bob", now, start("t3", None, "run the tests", false)));
    view.subagents.set_now("t2", "Read docs/a.md");

    // ada's turn ended: her subagents run, so she still works
    assert!(view.thread.working("ada") && !view.thread.busy.contains("ada"));
    assert!(view.animating());

    let text = screen(&mut view);
    let row = text
        .iter()
        .position(|row| row.contains("subagents (3)"))
        .unwrap_or_else(|| panic!("{text:#?}"));

    assert!(
        text[row].contains("ada: scan the repo") && text[row].contains("bob: run the tests"),
        "{}",
        text[row]
    );
    assert!(
        text.iter().any(|row| row.contains("subagents…")),
        "the agent card: {text:#?}"
    );

    click(&mut view, 2, row as u16);
    let text = screen(&mut view);

    let at = |needle: &str| {
        text.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle}: {text:#?}"))
    };

    assert_subagent_stop(view, &text, at);
}

fn assert_subagent_stop(mut view: View, text: &[String], at: impl Fn(&str) -> usize) {
    let (ada, scan, docs, now_row, bob) = (
        at("ada · 2 subagents"),
        at("scan the repo · Explore · background"),
        at("read the docs"),
        at("↳ Read docs/a.md"),
        at("bob · 1 subagent"),
    );

    assert!(
        ada < scan && scan < docs && docs < now_row && now_row < bob,
        "{text:#?}"
    );

    let col = |row: &str, needle: &str| row[..row.find(needle).unwrap()].chars().count();

    assert!(col(&text[docs], "read") > col(&text[scan], "scan"), "nested: indented");
    assert!(
        text[bob - 1]
            .trim_matches(|character| character == '│' || character == ' ')
            .is_empty(),
        "a blank row between masters: {text:#?}"
    );

    // ↓ ↓: bob's; d: asks; d again: stop it
    view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(
        !view.ops.borrow().iter().any(|recorded| recorded["op"] == "sub_stop"),
        "1 d: 0 stop"
    );

    view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    let last = view.ops.borrow().last().cloned().unwrap();

    assert_eq!(
        (last["op"].as_str(), last["id"].as_str()),
        (Some("sub_stop"), Some("t3")),
        "{last:?}"
    );
    assert_eq!(view.prompt.input.text(), "", "d went to the box, not the prompt");
}

#[test]
fn a_report_is_a_message_of_the_master_folded_and_a_lost_one_can_be_picked_up() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("subtest-0001", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into()];
    view.thread
        .push(&ev("ada", 1_000, start("t1", None, "scan the repo", false)));
    view.thread.push(&ev(
        "ada",
        1_000,
        store::Kind::SubInfo {
            id: "t1".into(),
            transcript: "/x/agent-1.jsonl".into(),
            task: "1".into(),
        },
    ));
    let mut report = String::new();

    for i in 1..=9 {
        let _ = writeln!(report, "line {i}");
    }

    view.thread.push(&ev(
        "ada",
        71_000,
        store::Kind::SubEnd {
            id: "t1".into(),
            state: "done".into(),
            result: report,
        },
    ));

    assert!(!view.thread.working("ada"));

    let text = screen(&mut view);
    let at = |text: &[String], needle: &str| text.iter().position(|row| row.contains(needle));

    assert!(
        at(&text, "↳ subagent “scan the repo” · done · 1m10s").is_some(),
        "{text:#?}"
    );
    assert!(
        at(&text, "line 6").is_some() && at(&text, "line 7").is_none(),
        "folded after 6 lines: {text:#?}"
    );

    let more = at(&text, "3 more lines").unwrap();

    assert!(!text.iter().any(|row| row.contains("subagents (")), "0 running: 0 row");

    click(&mut view, 6, more as u16);
    let text = screen(&mut view);

    assert!(at(&text, "line 9").is_some(), "opened: {text:#?}");

    // a lost one: the error and its pick up
    assert_subagent_recovery(view, at);
}

fn assert_subagent_recovery(mut view: View, at: impl Fn(&[String], &str) -> Option<usize>) {
    view.thread
        .push(&ev("ada", 80_000, start("t2", None, "fix the build", true)));
    view.thread.push(&ev(
        "ada",
        80_000,
        store::Kind::SubInfo {
            id: "t2".into(),
            transcript: "/x/agent-2.jsonl".into(),
            task: "2".into(),
        },
    ));
    view.thread.push(&ev(
        "ada",
        90_000,
        store::Kind::SubEnd {
            id: "t2".into(),
            state: "lost".into(),
            result: "the harness process of ada ended".into(),
        },
    ));
    view.stream.dirty = true;
    let text = screen(&mut view);

    assert!(
        at(&text, "ended without a report: the harness process of ada ended").is_some(),
        "{text:#?}"
    );

    let pick = at(&text, "pick up: ada goes on").unwrap();

    click(&mut view, 6, pick as u16);
    let last = view.ops.borrow().last().cloned().unwrap();
    let text = last["text"].as_str().unwrap_or_default();

    assert_eq!(last["to"], serde_json::json!(["ada"]));
    assert!(
        text.contains("“fix the build”") && text.contains("/x/agent-2.jsonl") && text.contains("> do fix the build"),
        "{text}"
    );
}

/// A message of the master resumes an ended subagent: the card shows `subagents…`, the
/// running row lists it; the old block keeps the 1st report; the rerun's end: a 2nd block
/// at the bottom.
#[test]
fn a_resumed_subagent_reports_in_a_2nd_block_at_the_bottom() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("subtest-0002", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into()];
    view.thread
        .push(&ev("ada", 1_000, start("t1", None, "fix the flakes", true)));
    view.thread.push(&ev(
        "ada",
        61_000,
        store::Kind::SubEnd {
            id: "t1".into(),
            state: "done".into(),
            result: "fixed 12 of 13".into(),
        },
    ));

    assert!(!view.thread.working("ada"));

    view.thread
        .push(&ev("ada", 100_000, store::Kind::SubResume { id: "t1".into() }));

    assert!(
        view.thread.working("ada") && !view.thread.busy.contains("ada"),
        "resumed: ada works"
    );

    view.stream.dirty = true;
    let text = screen(&mut view);
    let count = |text: &[String], needle: &str| text.iter().filter(|row| row.contains(needle)).count();

    assert!(
        text.iter().any(|row| row.contains("subagents…")),
        "the agent card: {text:#?}"
    );
    assert!(
        text.iter().any(|row| row.contains("subagents (1)")),
        "the running row: {text:#?}"
    );
    assert_eq!(
        count(&text, "↳ subagent “fix the flakes” · done · 1m00s"),
        1,
        "the old block, as it was: {text:#?}"
    );
    assert_eq!(
        count(&text, "↳ subagent “fix the flakes”"),
        1,
        "0 2nd block while it runs: {text:#?}"
    );

    assert_subagent_resume(view, count);
}

fn assert_subagent_resume(mut view: View, count: impl Fn(&[String], &str) -> usize) {
    view.thread.push(&ev(
        "ada",
        130_000,
        store::Kind::SubEnd {
            id: "t1".into(),
            state: "done".into(),
            result: "fixed the 13th".into(),
        },
    ));

    assert!(!view.thread.working("ada"));

    view.stream.dirty = true;
    let text = screen(&mut view);

    let at = |needle: &str| {
        text.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle}: {text:#?}"))
    };

    let (old, first, new, second) = (
        at("“fix the flakes” · done · 1m00s"),
        at("fixed 12 of 13"),
        at("“fix the flakes” · resumed · done · 30s"),
        at("fixed the 13th"),
    );

    assert!(
        old < first && first < new && new < second,
        "old block, then the rerun's block below: {text:#?}"
    );
    assert_eq!(count(&text, "fixed 12 of 13"), 1, "{text:#?}");
    assert!(!text.iter().any(|row| row.contains("subagents (")), "0 running: 0 row");
}
