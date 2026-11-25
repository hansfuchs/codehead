use super::*;

fn view() -> View {
    let (tx, _rx) = crate::infra::bus::channel();

    View::new("panetest-0000", Path::new("/tmp"), "thread p", tx)
}

fn press(view: &mut View, key: KeyCode, message: KeyModifiers) -> bool {
    view.key(KeyEvent::new(key, message))
}

fn screen(view: &mut View) -> String {
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..30)
        .map(|y| (0..100).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Row 0: `⌂ home` left of the views; a click on it: the start screen (the view ends).
#[test]
fn the_home_pill_goes_to_the_start_screen() {
    let mut value = view();
    let text = screen(&mut value);
    let row0 = text.lines().next().unwrap_or_default();

    assert!(
        row0.contains("⌂ home") && row0.find("⌂ home") < row0.find(" chat "),
        "{row0}"
    );

    let result = value.layout.home_rect;

    value.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: result.x + 1,
        row: result.y,
        modifiers: KeyModifiers::NONE,
    });

    assert!(value.mouse_exit && matches!(value.exit, Exit::Back));

    // other threads that need the owner: their count on the pill
    let note = |thread: &str, kind| crate::domain::notifications::Note {
        thread: thread.into(),
        title: thread.into(),
        dir: PathBuf::new(),
        kind,
        agents: vec![],
        ts: 1,
        text: String::new(),
        comment: String::new(),
    };
    let (label, on) = home_label(&[
        note("a", crate::domain::notifications::Kind::Ask),
        note("a", crate::domain::notifications::Kind::Error),
        note("b", crate::domain::notifications::Kind::Blocked),
        note("c", crate::domain::notifications::Kind::Done),
    ]);

    assert_eq!((label.as_str(), on), (" ⌂ home ?2 ", true));
}

/// ctrl+p → threads → the card of this thread → ctrl+o → a → y: archived, the view goes to
/// the start screen's threads tab; the prompt's draft stays.
#[test]
fn archiving_this_thread_from_the_menu_goes_home() {
    let thread = crate::engine::store::Thread::new("archive from view", "task", Path::new("/tmp"), vec!["ada".into()]);

    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, Path::new("/tmp"), "archive from view", tx);

    view.prompt.input.set_text("half a message");
    view.open_menu();
    view.menu
        .as_mut()
        .unwrap()
        .list
        .set_cards(vec![crate::ui::thread_card::Card {
            id: thread.id.clone(),
            title: "archive from view".into(),
            state: "open".into(),
            ..Default::default()
        }]);

    // quick → threads (tab bar), ↓ repos, ↓ projects, ↓ views, ↓ status, ↓ the list
    for key in [
        KeyCode::Right,
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Down,
    ] {
        press(&mut view, key, KeyModifiers::NONE);
    }

    press(&mut view, KeyCode::Char('.'), KeyModifiers::CONTROL);
    let text = screen(&mut view);

    assert!(text.contains("archive: worktree goes"), "{text}");

    press(&mut view, KeyCode::Char('a'), KeyModifiers::NONE);
    let text = screen(&mut view);

    assert!(text.contains("archive 1 thread(s)?"), "{text}");
    assert!(
        press(&mut view, KeyCode::Char('y'), KeyModifiers::NONE),
        "the view ends"
    );
    assert!(matches!(
        view.exit,
        Exit::Home(crate::screens::quick_menu::Home::Threads)
    ));
    assert_eq!(crate::engine::store::load(&thread.id).unwrap().state, "archived");
    assert_eq!(view.prompt.input.text(), "half a message");

    let _ = crate::engine::store::delete(&thread.id);
}

/// A spawn under way (the thread just opened): the `>` dropdown stays over its goo.
#[test]
fn spawn_never_draws_over_the_command_dropdown() {
    fn buffer(view: &mut View, text: &str) -> ratatui::buffer::Buffer {
        view.prompt.input.set_text(text);
        let mut thread = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

        thread.draw(|file| view.draw(file)).unwrap();
        thread.backend().buffer().clone()
    }

    let mut value = view();

    value.agents.members = vec!["ada".into()];
    value.sync_order();
    let key = crate::infra::config::command_key().to_string();

    // frozen clocks, not the process clock: just before the snap the goo inside the sprite's
    // slot can match the idle sprite of that moment (28 % of clock values), so the goo's
    // mark is checked over its whole canvas (header rows); several clocks, each the same
    // every run
    for now in [3.0, 7.4, 8.14, 9.99, 42.0, 77.7] {
        crate::domain::avatar::freeze(Some(now));
        value.agents.spawns.clear();
        let (calm, calm_menu) = (buffer(&mut value, ""), buffer(&mut value, &key));
        let areas = value.prompt.command.areas().to_vec();

        assert!(!areas.is_empty(), "the dropdown shows");

        // the goo at its densest: just before the snap
        let start = now - crate::domain::avatar::spawn::timing(0).2 * 0.95;

        value
            .agents
            .spawns
            .insert("ada".into(), crate::domain::avatar::Spawn { anim: 0, start });
        let (goo, goo_menu) = (buffer(&mut value, ""), buffer(&mut value, &key));
        let slot = value.agents.slots.borrow()[0].1;

        assert!(slot.intersects(areas[0]), "the dropdown covers the sprite");

        let (ax, ay) = crate::domain::avatar::SPAWN_AT;
        let (x0, y0) = (slot.x.saturating_sub(ax as u16), slot.y.saturating_sub(ay as u16));
        let x1 = (x0 + crate::domain::avatar::SPAWN_W as u16).min(calm.area.right());
        let differs = (y0..slot.bottom()).any(|y| (x0..x1).any(|x| calm[(x, y)].symbol() != goo[(x, y)].symbol()));

        assert!(differs, "the spawn draws (clock {now})");

        for result in &areas {
            assert_dropdown_unchanged(&calm_menu, &goo_menu, *result, now);
        }
    }

    crate::domain::avatar::freeze(None);
}

#[test]
fn view_bar_shows_both_views_and_alt_digits_switch() {
    let mut value = view();
    let text = screen(&mut value);

    // the changes label may carry a count (other tests share the thread id)
    assert!(
        text.contains(" chat   changes") && text.contains("files  │ thread p"),
        "{text}"
    );
    assert!(text.contains("F1-F4: views"), "{text}");

    // the view bar on row 0
    assert!(text.lines().next().unwrap().contains(" chat "), "{text}");

    press(&mut value, KeyCode::Char('2'), KeyModifiers::ALT);

    assert_eq!(value.panes.pane, Pane::Changes);

    let text = screen(&mut value);

    assert!(
        text.contains("space: select") && text.contains("v: reviewed") && text.contains("tab: next"),
        "{text}"
    );

    press(&mut value, KeyCode::Char('1'), KeyModifiers::ALT);

    assert_eq!(value.panes.pane, Pane::Chat);
}

/// Ctrl+.: the panel's menu right of the panel with its ship steps; the review's never over
/// the cursor's row; the prompt's above the prompt; esc closes it.
#[test]
fn the_context_menu_opens_beside_the_thing_in_focus() {
    let mut value = view();

    press(&mut value, KeyCode::Char('2'), KeyModifiers::ALT);
    screen(&mut value);

    // the panel: right of it, its ship steps under "ship"
    press(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(value.panes.changes.panel_focus());

    press(&mut value, KeyCode::Char('.'), KeyModifiers::CONTROL);
    let text = screen(&mut value);
    let message = value.ctx.as_ref().expect("the panel's menu");

    assert!(message.items.iter().any(|i| i.section.starts_with("ship")), "{text}");
    assert!(text.contains("─ ship ─"), "{text}");

    let crate::ui::context_menu::Place::Beside(path, _) = value.panes.changes.ctx_place() else {
        panic!("beside the panel")
    };

    assert!(
        path.width > 0
            && text
                .lines()
                .any(|line| line.chars().nth(usize::from(path.right())) == Some('┌')),
        "{text}"
    );

    press(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(
        value.ctx.is_none() && value.panes.changes.panel_focus(),
        "esc: the menu only"
    );

    // S: 0 ship menu of its own
    press(&mut value, KeyCode::Char('S'), KeyModifiers::SHIFT);

    assert!(!screen(&mut value).contains("─ ship ─"));

    // the review: above / below its cursor's row (shift+tab from the panel)
    press(&mut value, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(!value.panes.changes.panel_focus() && !value.panes.prompt_focus);

    press(&mut value, KeyCode::Char('.'), KeyModifiers::CONTROL);
    screen(&mut value);

    assert!(value.ctx.is_some());

    let crate::ui::context_menu::Place::At(row) = value.panes.changes.ctx_place() else {
        panic!("at the cursor's row")
    };

    assert!(!text.is_empty() && row.height == 1);

    press(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    // the prompt: above the prompt
    press(&mut value, KeyCode::BackTab, KeyModifiers::SHIFT);
    press(&mut value, KeyCode::Char('.'), KeyModifiers::CONTROL);
    let text = screen(&mut value);

    assert!(
        text.contains("search the chat") && text.contains("IDE on the worktree"),
        "{text}"
    );
}

#[test]
fn tab_moves_between_the_view_and_the_prompt_ctrl_p_is_the_menu() {
    let mut value = view();

    press(&mut value, KeyCode::Char('2'), KeyModifiers::ALT);

    // the ring: prompt → review → panel → … → prompt; shift+tab: back
    press(&mut value, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(
        value.panes.prompt_focus && value.menu.is_none(),
        "shift+tab: the prompt, 0 menu"
    );

    press(&mut value, KeyCode::Char('h'), KeyModifiers::NONE);
    press(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(
        !value.panes.prompt_focus && !value.panes.changes.panel_focus() && value.prompt.input.text() == "h",
        "tab: back to the review, the text stays"
    );

    press(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(value.panes.changes.panel_focus(), "tab: the panel");

    press(&mut value, KeyCode::BackTab, KeyModifiers::SHIFT);

    // the chat: tab never opens the menu; ctrl+p does, from every field
    press(&mut value, KeyCode::Char('1'), KeyModifiers::ALT);
    press(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(value.menu.is_none());

    press(&mut value, KeyCode::Char('p'), KeyModifiers::CONTROL);

    assert!(value.menu.is_some());

    press(&mut value, KeyCode::Char('p'), KeyModifiers::CONTROL);

    assert!(value.menu.is_none(), "ctrl+p again: closed");

    // the changes view with the keys: ctrl+p opens the menu there too
    press(&mut value, KeyCode::Char('2'), KeyModifiers::ALT);
    press(&mut value, KeyCode::Char('p'), KeyModifiers::CONTROL);

    assert!(value.menu.is_some());
}

#[test]
fn changes_view_keeps_letters_from_the_prompt_until_ctrl_m() {
    let mut value = view();

    press(&mut value, KeyCode::Char('2'), KeyModifiers::ALT);
    press(&mut value, KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(value.prompt.input.text(), "", "j: a view key");

    press(&mut value, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(value.panes.prompt_focus);

    press(&mut value, KeyCode::Char('h'), KeyModifiers::NONE);

    assert_eq!(value.prompt.input.text(), "h");

    // esc: the keys back to the view, the text stays
    press(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(!value.panes.prompt_focus && value.prompt.input.text() == "h");
    assert!(value.last_esc.is_none(), "0 interrupt / back timer");
}

#[test]
fn changes_view_selects_lines_comments_on_them_and_filters_with_i_and_esc() {
    let mut value = view();

    press(&mut value, KeyCode::Char('2'), KeyModifiers::ALT);
    value.panes.changes.set_files(crate::features::changes::parse(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n",
    ));
    let text = screen(&mut value);

    assert!(text.contains("▾ medium") && text.contains("┌─ ☐ src/a.rs"), "{text}");

    // ↓ ↓ ↓: head → hunk → ` x` → `- a`; space selects 2 lines (the cursor goes down)
    for _ in 0..3 {
        press(&mut value, KeyCode::Down, KeyModifiers::NONE);
    }

    press(&mut value, KeyCode::Char(' '), KeyModifiers::NONE);
    press(&mut value, KeyCode::Char(' '), KeyModifiers::NONE);

    // c: the modal takes the comment on both lines
    press(&mut value, KeyCode::Char('c'), KeyModifiers::NONE);

    assert!(!value.panes.prompt_focus && value.panes.changes.typing());

    let text = screen(&mut value);

    assert!(text.contains("comment on src/a.rs:2 · removed 2"), "{text}");

    press(&mut value, KeyCode::Char('n'), KeyModifiers::NONE);
    press(&mut value, KeyCode::Enter, KeyModifiers::CONTROL);

    assert_eq!(value.panes.changes.comment_count(), 1);

    let text = screen(&mut value);

    assert!(text.contains("2 · removed 2 · you: n"), "{text}");

    // ctrl+f: the filter takes letters, i too; esc leaves it (0 interrupt timer)
    press(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);
    press(&mut value, KeyCode::Char('i'), KeyModifiers::NONE);

    assert!(!value.panes.prompt_focus && value.prompt.input.text().is_empty());
    assert!(screen(&mut value).contains(" f  i "));

    press(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(!value.panes.changes.typing() && value.last_esc.is_none());

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("panetest-0000"));
}

fn assert_dropdown_unchanged(
    calm_menu: &ratatui::buffer::Buffer,
    goo_menu: &ratatui::buffer::Buffer,
    result: Rect,
    now: f32,
) {
    for y in result.top()..result.bottom() {
        for x in result.left()..result.right() {
            assert_eq!(
                calm_menu[(x, y)].symbol(),
                goo_menu[(x, y)].symbol(),
                "spawn over the dropdown at {x},{y} (clock {now})"
            );
        }
    }
}
