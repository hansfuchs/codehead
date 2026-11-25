use super::*;

fn card(id: &str, state: &str, activity: u64) -> Card {
    Card {
        id: id.into(),
        title: id.into(),
        state: state.into(),
        activity,
        ..Default::default()
    }
}

#[test]
fn a_card_with_an_open_question_shows_blocked_also_while_its_turn_runs() {
    let base = card("t", "open", 0);
    let card = Card {
        agents: crate::domain::thread_card::AgentActivity {
            busy: true,
            blocked: true,
            live: 1,
            ..base.agents
        },
        ..base
    };
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, CARD_H));
    let area = buf.area;

    draw_card(
        &mut buf,
        area,
        &card,
        Look {
            cur: false,
            here: false,
            age: false,
        },
        0,
    );
    let text: String = (0..CARD_H)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|cell| buf[cell].symbol().to_string())
        .collect();

    assert!(text.contains("? blocked") && !text.contains("working"), "{text}");
}

/// The projects of a thread: own line below the title, above the status line; the bottom border stays plain.
#[test]
fn a_card_shows_its_projects_below_the_title() {
    let base = card("t", "open", 0);
    let card = Card {
        projects: vec!["bug-fixes".into(), "ux".into()],
        agents: crate::domain::thread_card::AgentActivity { live: 1, ..base.agents },
        ..base
    };
    let height = card_h(&card);

    assert_eq!(height, CARD_H + 1);

    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, height));
    let area = buf.area;

    draw_card(
        &mut buf,
        area,
        &card,
        Look {
            cur: false,
            here: false,
            age: false,
        },
        0,
    );
    let row = |y: u16| (0..40).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>();

    assert!(row(1).contains('t') && !row(1).contains("bug-fixes"), "{}", row(1));
    assert!(
        row(2).contains("bug-fixes · ux") && !row(2).contains("idle"),
        "{}",
        row(2)
    );
    assert!(row(3).contains("◦ idle") && !row(3).contains("bug-fixes"), "{}", row(3));
    assert!(!row(height - 1).contains("bug-fixes"), "{}", row(height - 1));
}

/// A monitor or background command runs, 0 turn: `monitoring…`, not `idle` or `working…`.
#[test]
fn a_card_with_a_running_monitor_shows_monitoring() {
    let base = card("t", "open", 0);
    let card = Card {
        agents: crate::domain::thread_card::AgentActivity {
            busy: true,
            monitoring: true,
            live: 1,
            ..base.agents
        },
        ..base
    };
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, CARD_H));
    let area = buf.area;

    draw_card(
        &mut buf,
        area,
        &card,
        Look {
            cur: false,
            here: false,
            age: false,
        },
        0,
    );
    let text: String = (0..CARD_H)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|cell| buf[cell].symbol().to_string())
        .collect();

    assert!(
        text.contains("monitoring…") && !text.contains("idle") && !text.contains("working"),
        "{text}"
    );
}

/// An unsent message in the thread's prompt: the card says so (bottom border).
#[test]
fn a_card_with_an_unsent_prompt_shows_it() {
    let id = "drafttest-0000";
    let dir = crate::engine::store::thread_dir(id);
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    assert!(!has_draft(id));

    crate::domain::drafts::save_prompt_draft(id, "   ");

    assert!(!has_draft(id), "only spaces: 0 message");

    crate::domain::drafts::save_prompt_draft(id, "fix the deploy");

    assert!(has_draft(id));

    let base = card(id, "open", 0);
    let card = Card {
        draft: true,
        agents: crate::domain::thread_card::AgentActivity { live: 1, ..base.agents },
        ..base
    };
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, CARD_H));
    let area = buf.area;

    draw_card(
        &mut buf,
        area,
        &card,
        Look {
            cur: false,
            here: false,
            age: false,
        },
        0,
    );
    let bottom: String = (0..40).map(|x| buf[(x, CARD_H - 1)].symbol().to_string()).collect();

    assert!(bottom.contains("✎ unsent"), "{bottom}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn filter_matches_title_and_branch() {
    let mut cards = vec![card("x", "open", 1), card("y", "open", 2)];

    cards[0].title = "fix the deploy".into();
    cards[1].branch = "task/menu".into();

    assert_eq!(filter(&cards, ""), [0, 1]);
    assert_eq!(filter(&cards, "deploy"), [0]);
    assert_eq!(filter(&cards, "menu"), [1]);
}
