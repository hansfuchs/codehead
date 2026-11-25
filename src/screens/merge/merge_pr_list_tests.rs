use super::*;

fn entry(repo: &str, number: u64, created: u64, state: State) -> Entry {
    Entry {
        item: Item {
            pr: PrRef::new(repo, number),
            title: format!("pr {number}"),
            state,
            created,
            head: format!("b{number}"),
        },
        thread: None,
    }
}

#[test]
fn grouped_by_repo_newest_first() {
    let mut list = PrList::with(vec![
        entry("a/x", 1, 10, State::Open),
        entry("b/y", 2, 30, State::Merged),
        entry("a/x", 3, 20, State::Closed),
        entry("b/y", 4, 5, State::Draft),
    ]);

    list.filter = Filter::All;
    let (rows, prs) = list.layout();

    assert_eq!(
        rows,
        vec![
            RowKind::Repo("b/y".into(), 2),
            RowKind::Pr(1),
            RowKind::Pr(3),
            RowKind::Repo("a/x".into(), 2),
            RowKind::Pr(2),
            RowKind::Pr(0)
        ]
    );
    assert_eq!(prs, vec![1, 3, 2, 0]);
}

#[test]
fn the_state_filter_and_enter() {
    let mut list = PrList::with(vec![
        entry("a/x", 1, 10, State::Open),
        entry("a/x", 2, 20, State::Merged),
        entry("a/x", 3, 30, State::Draft),
    ]);

    // the 1st tab: open (drafts too); ←→ walk open, merged, closed, all
    assert_eq!(list.layout().1, vec![2, 0], "open: drafts too");

    list.key(KeyEvent::from(KeyCode::Right));

    assert_eq!(list.layout().1, vec![1]);

    list.key(KeyEvent::from(KeyCode::Left));
    list.key(KeyEvent::from(KeyCode::Left));

    assert_eq!(list.layout().1.len(), 3, "all");

    list.key(KeyEvent::from(KeyCode::Right));

    assert!(matches!(list.key(KeyEvent::from(KeyCode::Down)), Pick::None));
    assert_eq!(
        list.key(KeyEvent::from(KeyCode::Enter)),
        Pick::Open(list.entries[0].clone())
    );
}

/// `CODEHEAD_GH_LIVE=1` cargo test `live_prlist` -- --ignored --nocapture
#[test]
#[ignore = "live GitHub access; explicit run required"]
fn live_prlist_draws() {
    let entries = load().unwrap();

    assert_ne!(entries.len(), 0);

    let mut list = PrList::with(entries);
    let backend = ratatui::backend::TestBackend::new(120, 16);
    let mut term = ratatui::Terminal::new(backend).unwrap();

    term.draw(|frame| list.draw(frame, frame.area(), true)).unwrap();
    let buf = term.backend().buffer().clone();

    println!(
        "{}",
        (0..16)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn items_round_trip_the_cache() {
    let item = entry("a/x", 7, 99, State::Draft).item;

    assert_eq!(item_of(&item_json(&item)), Some(item));
}
