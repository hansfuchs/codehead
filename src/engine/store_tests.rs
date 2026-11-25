use super::*;

#[test]
fn a_tail_from_the_end_starts_at_a_whole_line_and_the_head_holds_the_rest() {
    let thread = Thread::new("t", "do it", Path::new("/tmp"), vec!["ada".into()]);

    thread.save().unwrap();

    for i in 0..20 {
        append(
            &thread.id,
            &Event {
                ts: i,
                agent: Some("ada".into()),
                kind: Kind::Text {
                    text: format!("line {i} {}", "x".repeat(40)),
                },
            },
        )
        .unwrap();
    }

    let len = std::fs::metadata(events_path(&thread.id)).unwrap().len();
    let (mut tail, end) = Tail::from_end(&thread.id, len / 3);

    assert!(end > 0 && end < len);

    let (head, rest) = (head(&thread.id, end), tail.read());

    assert!(!head.is_empty() && !rest.is_empty());

    let ts: Vec<u64> = head.iter().chain(&rest).map(|event| event.ts).collect();

    assert_eq!(ts, (0..20).collect::<Vec<_>>(), "every event once, in order");

    // a short log: the whole of it from the tail, 0 head
    let (_, end) = Tail::from_end(&thread.id, len + 10);

    assert_eq!(end, 0);

    let _ = std::fs::remove_dir_all(thread_dir(&thread.id));
}

#[test]
fn events_round_trip_and_tail_reads_only_new_lines() {
    // the test process's data dir: 0 $CODEHEAD_HOME (a set var moves every other test's
    // threads mid-run: a stash written before it, read after it, is gone)
    let thread = Thread::new("t", "do it", Path::new("/tmp"), vec!["ada".into()]);

    thread.save().unwrap();
    let mut tail = Tail::new(&thread.id);

    append(
        &thread.id,
        &Event::new(
            None,
            Kind::Prompt {
                from: "owner".into(),
                to: vec!["ada".into()],
                text: "hi".into(),
            },
        ),
    )
    .unwrap();
    append(
        &thread.id,
        &Event::new(Some("ada"), Kind::Text { text: "hello".into() }),
    )
    .unwrap();

    assert_eq!(tail.read().len(), 2);
    assert_eq!(tail.read(), [] as [crate::engine::store::Event; 0]);

    append(&thread.id, &Event::new(Some("ada"), Kind::TurnStart)).unwrap();
    let events = tail.read();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, Kind::TurnStart);
    assert_eq!(load(&thread.id[..14]).unwrap().agents, vec!["ada".to_string()]);

    // 8 writers at once (host, views, CLI): the lock keeps each update
    let writers: Vec<_> = (0..8)
        .map(|writer| {
            let id = thread.id.clone();

            std::thread::spawn(move || {
                add_ten_agents(writer, &id);
            })
        })
        .collect();

    for handle in writers {
        handle.join().unwrap();
    }

    assert_eq!(load(&thread.id).unwrap().agents.len(), 81);

    let _ = std::fs::remove_dir_all(thread_dir(&thread.id));
}

fn add_ten_agents(writer: i32, id: &str) {
    for i in 0..10 {
        update(id, |thread| thread.agents.push(format!("a{writer}-{i}"))).unwrap();
    }
}

#[test]
fn owner_choices_keep_the_legacy_json_fields() {
    let mut thread = Thread::new("title", "task", std::path::Path::new("/tmp"), Vec::new());

    thread.owner_choices.projects_set = true;
    thread.owner_choices.title_set = true;
    let mut value = serde_json::to_value(&thread).unwrap();

    assert_eq!(value["projects_set"], true);
    assert_eq!(value["title_set"], true);
    assert!(value.get("owner_choices").is_none());
    assert_eq!(serde_json::from_value::<Thread>(value.clone()).unwrap(), thread);

    value.as_object_mut().unwrap().remove("projects_set");
    value.as_object_mut().unwrap().remove("title_set");
    let restored: Thread = serde_json::from_value(value).unwrap();

    assert!(!restored.owner_choices.projects_set);
    assert!(!restored.owner_choices.title_set);
}
