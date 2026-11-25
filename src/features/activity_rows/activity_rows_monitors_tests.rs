use super::*;
use serde_json::json;

fn monitor(id: &str) -> Value {
    json!({"session":"session","agent":"amon","harness":"claude","at":1000,"content":{
        "title":"Monitor","raw_input":{"command":"tail -f smoke.log | grep result","description":"smoke job progress","timeout_ms":30000},
        "raw_output":format!("Monitor started (task {id}, expires in 30m)")}})
}

#[test]
fn monitors_survive_idle_registration_then_expire_or_stop() {
    let row = monitor("watch1");
    let watches = parse(std::slice::from_ref(&row), 2000);

    assert_eq!(watches.len(), 1);
    assert_eq!(watches[0].description, "smoke job progress");
    assert_eq!(watches[0].command, "tail -f smoke.log | grep result");
    assert!(parse(std::slice::from_ref(&row), 31000).is_empty());

    let stop = json!({"session":"session","content":{"title":"TaskStop","raw_input":{"task_id":"watch1"}}});

    assert!(parse(&[row, stop], 2000).is_empty());
    assert!(
        parse(
            &[json!({"content":{"title":"ToolSearch","raw_input":{"query":"select:Monitor"}}})],
            2000
        )
        .is_empty()
    );
}

/// A sub-thread's host: a Monitor of a live agent keeps it from its end; 0 live agent: 0 watch.
#[test]
fn a_thread_host_sees_the_monitors_of_its_live_agents() {
    use crate::engine::store::{self, Event, Kind};
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("kid", "x", std::path::Path::new("/tmp"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let ev = |kind| Event {
        ts: crate::engine::now_ms(),
        agent: Some("ada".into()),
        kind,
    };

    store::append(
        &thread.id,
        &ev(Kind::Tool {
            id: "c1".into(),
            name: "Monitor".into(),
            input: json!({"command":"gh run watch","description":"CI run"}),
        }),
    )
    .unwrap();
    store::append(
        &thread.id,
        &ev(Kind::ToolResult {
            id: "c1".into(),
            output: "Monitor started (task w1, expires in 30m)".into(),
            ok: true,
        }),
    )
    .unwrap();

    assert!(watching(&thread.id, &["ada".into()]));
    assert!(!watching(&thread.id, &[]));

    let _ = store::delete(&thread.id);
}

/// A session resumed in another dir: 2 transcript copies, the end notice only in the newer one
/// (the older one sorts 1st by name). The watch ends: 0 stale monitors row, the host may exit.
#[test]
fn a_background_task_ends_by_the_newest_transcript_copy() {
    use crate::engine::store::{self, Event, Kind};
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut thread = store::Thread::new("moved", "x", std::path::Path::new("/tmp"), vec!["ada".into()]);

    thread.sessions.insert("ada".into(), "sid-moved".into());
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let ev = |kind| Event {
        ts: crate::engine::now_ms(),
        agent: Some("ada".into()),
        kind,
    };

    store::append(
        &thread.id,
        &ev(Kind::Tool {
            id: "c1".into(),
            name: "Bash".into(),
            input: json!({"command":"run ci","description":"local CI","run_in_background":true}),
        }),
    )
    .unwrap();
    store::append(
        &thread.id,
        &ev(Kind::ToolResult {
            id: "c1".into(),
            output: "Command running in background with ID: bg1. Output is being written to: /tmp/bg1.output".into(),
            ok: true,
        }),
    )
    .unwrap();

    // the stale copy: 1st dir by name, 1 h old, 0 notice; the live copy: the end notice
    let root = std::env::temp_dir().join(format!("codehead-moved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (old, new) = (root.join("-a-codehead-x"), root.join("-b-platform-x"));

    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let notice = json!({"type":"queue-operation","content":"<task-notification>\n<task-id>bg1</task-id>\n<status>completed</status>\n</task-notification>"});

    std::fs::write(new.join("sid-moved.jsonl"), format!("{notice}\n")).unwrap();
    std::fs::write(old.join("sid-moved.jsonl"), "{\"type\":\"user\"}\n").unwrap();
    let file = std::fs::File::options()
        .write(true)
        .open(old.join("sid-moved.jsonl"))
        .unwrap();

    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();

    assert!(watches_at(&thread, &["ada".into()], &root).is_empty());

    // 0 transcript of the session: the watch stays
    thread.sessions.insert("ada".into(), "sid-none".into());

    assert_eq!(watches_at(&thread, &["ada".into()], &root).len(), 1);

    let _ = store::delete(&thread.id);

    std::fs::remove_dir_all(&root).unwrap();
}

/// A notice delivered at once (0 queue): only a `user` line with origin task-notification.
#[test]
fn a_user_line_notice_ends_the_task() {
    let dir = std::env::temp_dir().join(format!("codehead-user-notice-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.jsonl");
    let plain = json!({"type":"user","message":{"content":"<task-notification><task-id>fake</task-id><status>completed</status></task-notification>"}});
    let notice = json!({"type":"user","origin":{"kind":"task-notification"},"message":{"content":"<task-notification>\n<task-id>m1</task-id>\n<status>completed</status>\n</task-notification>"}});

    std::fs::write(&path, format!("{plain}\n{notice}\n")).unwrap();

    assert_eq!(completed(path.to_str().unwrap()), HashSet::from(["m1".into()]));

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn codex_process_completion_overrides_stale_input_status() {
    let row = json!({"session":"codex","agent":"paimon","harness":"codex","status":"in_progress","content":{
        "title":"run tests","raw_input":{"type":"commandExecution","processId":"123","status":"inProgress","command":"cargo test"}}});

    assert_eq!(parse(std::slice::from_ref(&row), 2000).len(), 1);

    let mut done = row.clone();

    done["status"] = json!("completed");

    assert!(parse(&[row, done], 2000).is_empty());
}

#[test]
fn claude_background_tasks_register_by_structured_task_id() {
    let row = json!({"session":"claude","content":{"title":"Bash","raw_input":{"command":"run smoke","run_in_background":true},"raw_output":{"task_id":"job1"}}});
    let watches = parse(&[row], 2000);

    assert_eq!(watches.len(), 1);
    assert_eq!(watches[0].id, "job1");
    assert!(watches[0].expires.is_none());

    let row = json!({"session":"claude","content":{"title":"`run smoke`","raw_input":{"command":"run smoke","run_in_background":true},"raw_output":"Command running in background with ID: bg-job. Output is being written to: /tmp/job"}});

    assert_eq!(parse(&[row], 2000)[0].id, "bg-job");
}

#[test]
fn task_notifications_end_only_the_named_task_and_support_incremental_reads() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!(
        "codehead-watch-{}-{}",
        std::process::id(),
        (crate::engine::now_ms() / 1000)
    ));

    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("transcript.jsonl");
    let mut file = std::fs::File::create(&path).unwrap();

    writeln!(file, "{}", json!({"type":"assistant","content":"<task-notification><task-id>not-real</task-id><status>completed</status></task-notification>"})).unwrap();

    assert!(completed(path.to_str().unwrap()).is_empty());

    writeln!(file, "{}", json!({"type":"queue-operation","content":"<task-notification><task-id>job1</task-id><status>running</status></task-notification>"})).unwrap();

    assert!(completed(path.to_str().unwrap()).is_empty());

    writeln!(file, "{}", json!({"type":"queue-operation","content":"<task-notification><task-id>job1</task-id><status>completed</status></task-notification>"})).unwrap();

    assert_eq!(completed(path.to_str().unwrap()), HashSet::from(["job1".into()]));
    assert_eq!(completed(path.to_str().unwrap()), HashSet::from(["job1".into()]));

    drop(file);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn monitor_bar_click_opens_details_and_escape_closes_without_navigation() {
    use ratatui::crossterm::event::KeyModifiers;
    let mut panel = Monitors {
        watches: parse(&[monitor("watch1")], 2000),
        ..Default::default()
    };
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    assert_eq!(panel.rows(100, 20), 1);

    term.draw(|frame| panel.render(frame, Rect::new(0, 24, 100, 1), 0))
        .unwrap();

    assert!(panel.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 24,
        modifiers: KeyModifiers::NONE
    }));

    // open: the box takes rows above the monitors row (the view's layout gives them)
    let rows = panel.rows(100, 20);

    assert!(rows > 4, "{rows}");

    let area = Rect::new(0, 25 - rows, 100, rows);

    term.draw(|frame| panel.render(frame, area, 1)).unwrap();

    assert!(!panel.popup.is_empty() && panel.popup.bottom() == 24);

    let buffer = term.backend().buffer();
    let text = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();

    assert!(text.contains("smoke job progress"));
    assert!(text.contains("tail -f smoke.log"));
    assert!(text.contains("d d end"));
    assert!(panel.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(!panel.open);
    assert_eq!(panel.rows(100, 20), 1);
}

#[test]
fn d_ends_the_selected_watch_and_its_process() {
    use ratatui::crossterm::event::KeyModifiers;

    // a real process whose command line holds the watch command
    let tag = format!(
        "codehead-monitor-end-{}-{}",
        std::process::id(),
        (crate::engine::now_ms() / 1000)
    );

    // `; echo`: sh keeps running (0 exec of the last command), like a harness shell
    let mut child = Command::new("sh")
        .args(["-c", &format!("sleep 30; echo {tag}")])
        .spawn()
        .unwrap();
    let mut watch = monitor("watch1");

    watch["content"]["raw_input"]["command"] = json!(format!("sleep 30; echo {tag}"));
    let mut panel = Monitors {
        watches: parse(&[watch], 2000),
        open: true,
        ..Default::default()
    };

    std::thread::sleep(std::time::Duration::from_millis(100));

    // d: asks (the watch stays); d again: ends it
    assert!(panel.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)));
    assert!(panel.armed && panel.watches.len() == 1);

    assert!(panel.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)));
    assert!(panel.watches.is_empty() && !panel.open);
    assert!(panel.take_note().unwrap().starts_with("monitor ended"));

    let status = child.wait().unwrap();

    assert!(!status.success(), "SIGTERM ended it");

    // a later read: still hidden
    panel.watches = parse(&[monitor("watch1")], 2000);
    panel.watches.retain(|watch| !panel.ended.contains(&watch.key()));

    assert!(panel.watches.is_empty());
}
