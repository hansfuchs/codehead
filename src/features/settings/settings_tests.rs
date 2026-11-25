use super::*;

fn fixture(name: &str) -> (PathBuf, Form) {
    let directory = std::env::temp_dir().join(format!("codehead-settings-{}-{name}", std::process::id()));

    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("codehead.toml");

    std::fs::write(&path, "# config\nnotes = '~/notes'\n").unwrap();
    let form = Form::new(path);

    (directory, form)
}

fn key(form: &mut Form, code: KeyCode) -> Action {
    form.key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn file_aliases(directory: &std::path::Path) -> usize {
    config::read_file(&directory.join("codehead.toml"))
        .unwrap()
        .1
        .aliases
        .len()
}

#[test]
fn keyboard_entry_starts_before_existing_field_text() {
    let (directory, mut form) = fixture("cursor-start");

    std::fs::write(directory.join("codehead.toml"), "notes = '~/notes/'\nstyles = '~/styles/'\nagent = 'amon'\nprefix = 'task'\nmodels = 'opus'\nmessages = 'lines'\n[aliases]\ndocs = '~/docs/'\n").unwrap();
    super::reload(&mut form).unwrap();

    let at_start = |form: &mut Form, i: usize| {
        assert_eq!(super::field(form), Some(i));
        assert!(form.fields[i].path_word().is_none());

        let text = form.fields[i].text();

        for _ in text.chars() {
            form.fields[i].key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        }

        assert_eq!(form.fields[i].selected_text(), Some(text));

        form.fields[i].move_start();
    };

    at_start(&mut form, 0);

    for i in 1..PLAIN {
        form.fields[i].move_end();
        key(&mut form, KeyCode::Down);

        if form.fields[i].text().is_empty() {
            assert_eq!(super::field(&form), Some(i));
        } else {
            at_start(&mut form, i);
        }
    }

    key(&mut form, KeyCode::Down);

    for i in PLAIN..PLAIN + 2 {
        form.fields[i].move_end();
    }

    key(&mut form, KeyCode::Enter);
    at_start(&mut form, PLAIN);
    key(&mut form, KeyCode::Down);
    at_start(&mut form, PLAIN + 1);
    key(&mut form, KeyCode::Up);
    at_start(&mut form, PLAIN);
    key(&mut form, KeyCode::Enter);

    // pgup: the start of the section before (colors, keys, general)
    for _ in 0..SECTIONS.len() {
        key(&mut form, KeyCode::PageUp);
    }

    at_start(&mut form, 0);
    form.fields[0].move_end();
    form.focus_input();
    at_start(&mut form, 0);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn fields_sit_in_groups_under_a_heading() {
    let (directory, mut form) = fixture("groups");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 130)).unwrap();

    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let lines: Vec<String> = (0..130)
        .map(|y| (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect();

    let at = |text: &str| {
        lines
            .iter()
            .position(|line| line.trim_start().starts_with(text))
            .unwrap_or_else(|| panic!("{text}:\n{}", lines.join("\n")))
    };

    // headings in order, each with a rule; its fields below it, indented
    let heads: Vec<usize> = [
        "paths ─",
        "new threads ─",
        "thread view ─",
        "keys ─",
        "colors ─",
        "path aliases ─",
    ]
    .iter()
    .map(|head| at(head))
    .collect();

    assert!(heads.windows(2).all(|pair| pair[0] < pair[1]), "{heads:?}");

    for (field, group) in [("notes", 0), ("agent", 1), ("thoughts", 2), ("menu", 3), ("scheme", 4)] {
        let y = at(field);

        assert!(heads[group] < y && y < heads[group + 1], "{field}");
        assert!(
            lines[y].starts_with(&format!("{}{field}", " ".repeat(INDENT as usize))),
            "{field}: {:?}",
            lines[y]
        );
    }

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn ctrl_d_and_the_reset_mark_set_a_field_back_to_its_default() {
    let (directory, mut form) = fixture("reset");
    let file = directory.join("codehead.toml");

    std::fs::write(
        &file,
        "messages = 'lines'\n[keys]\nmenu = 'ctrl+p'\n[colors]\nscheme = 'amber'\n",
    )
    .unwrap();
    super::reload(&mut form).unwrap();
    form.focus = 7;
    form.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));

    assert_eq!(form.fields[7].text(), "");

    let saved = config::read_file(&file).unwrap().1;

    assert_eq!((saved.messages, saved.menu_key.as_deref()), (None, Some("ctrl+p")));

    // a click on ↺ of the scheme field
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 100)).unwrap();

    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();
    let (reset_row, _) = *form
        .rows
        .iter()
        .find(|(_, hit)| *hit == Hit::Reset(17))
        .expect("↺ on the set scheme field");

    form.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: reset_row.x,
        row: reset_row.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(config::read_file(&file).unwrap().1.scheme, None);
    assert!(!form.rows.iter().any(|(_, hit)| *hit == Hit::Reset(7)));

    // an invalid value: 0 write, the error shows
    form.focus = 18;
    form.fields[18].set_text("red");
    form.commit();

    assert!(form.error && form.status.contains("#RRGGBB"));

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn alias_boxes_lock_apply_on_leave_and_delete_with_d() {
    let (directory, mut form) = fixture("save");

    assert!(!super::dirty(&form));

    // enter on + add: a new box, open on its name
    form.focus = super::add_item(&form);
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!((form.focus, form.editing), (PLAIN, Some(PLAIN)));

    form.paste("project:platform");

    // enter locks the box: 0 target → not applied, the error shows
    key(&mut form, KeyCode::Enter);

    assert_eq!(form.editing, None);
    assert!(form.error && form.take_applied().is_none());
    assert_eq!(file_aliases(&directory), 0);

    // a locked box: ↑↓ pass it, typing does not reach its fields
    key(&mut form, KeyCode::Char('x'));

    assert_eq!(form.fields[PLAIN].text(), "project:platform");

    key(&mut form, KeyCode::Enter);
    key(&mut form, KeyCode::Down);

    assert_eq!(form.editing, Some(PLAIN + 1));

    form.paste("~/projects/exampleorg/project/notes/platform/");
    key(&mut form, KeyCode::Enter);
    let file = form.take_applied().expect("applied on lock");

    assert_eq!(
        file.aliases["project:platform"],
        "~/projects/exampleorg/project/notes/platform/"
    );
    assert!(!super::dirty(&form) && !form.error);
    assert!(
        std::fs::read_to_string(directory.join("codehead.toml"))
            .unwrap()
            .contains("# config")
    );

    // d d on the highlighted box: deleted + applied (1 d: armed only)
    key(&mut form, KeyCode::Char('d'));

    assert_eq!(super::aliases(&form), 1);

    key(&mut form, KeyCode::Char('d'));

    assert_eq!(super::aliases(&form), 0);
    assert_eq!(file_aliases(&directory), 0);
    assert!(form.take_applied().is_some());

    // a plain field applies when the cursor leaves it; d there is text
    form.focus = 0;
    form.fields[0].set_text("");
    form.fields[0].move_end();
    key(&mut form, KeyCode::Char('d'));

    assert_eq!(form.fields[0].text(), "d");

    form.fields[0].set_text("");
    key(&mut form, KeyCode::Down);

    assert_eq!(form.focus, 1);
    assert_eq!(
        config::read_file(&directory.join("codehead.toml")).unwrap().1.notes,
        None
    );

    // ↑ from the 1st field: the tabs, the edit applied
    form.focus = 0;

    assert!(matches!(key(&mut form, KeyCode::Up), Action::Header));

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn form_scrolls_to_controls_and_handles_invalid_reload() {
    let (directory, mut form) = fixture("render");

    for _ in 0..10 {
        form.editing = None;
        form.focus = super::add_item(&form);
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    form.editing = None;
    form.focus = super::add_item(&form);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 12)).unwrap();

    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();

    assert!(form.rows.iter().any(|(_, hit)| *hit == Hit::Add));
    assert!(form.offset > 0);

    std::fs::write(directory.join("codehead.toml"), "notes = 4\n").unwrap();

    assert!(super::reload(&mut form).is_none());
    assert!(form.error);
    assert_eq!(form.fields.len(), PLAIN + 20);

    let mut invalid = Form::new(directory.join("codehead.toml"));

    terminal.draw(|frame| invalid.draw(frame, frame.area(), true)).unwrap();

    assert!(invalid.error);

    invalid.fields[0].set_text("~/x");
    invalid.commit();

    assert!(invalid.take_applied().is_none());
    assert_eq!(
        std::fs::read_to_string(directory.join("codehead.toml")).unwrap(),
        "notes = 4\n"
    );

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn alias_box_draws_a_border_and_highlights_when_locked() {
    let (directory, mut form) = fixture("box");

    form.focus = super::add_item(&form);
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    form.paste("docs");
    key(&mut form, KeyCode::Down);
    form.paste("~/docs/");
    key(&mut form, KeyCode::Enter);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 140)).unwrap();

    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let line = |y: u16| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    let top = (0..140)
        .find(|&y| line(y).contains("alias 1 · enter: edit · d: delete"))
        .expect("highlighted box");

    // locked + highlighted: the far left column an accent bar
    assert!(line(top).trim_start().starts_with('┌'));

    let x = (0..80)
        .find(|&x| buffer[(x, top + 1)].bg == theme::pal().accent)
        .expect("accent bar");

    assert_eq!(buffer[(x, top + 2)].bg, theme::pal().accent);
    assert!(buffer[(x, top)].bg != theme::pal().accent && buffer[(x, top + 3)].bg != theme::pal().accent);
    assert!(line(top + 1).contains("docs") && line(top + 2).contains("~/docs/"));
    assert!(line(top + 3).trim_start().starts_with('└'));

    // another item focused: gray border, plain title
    key(&mut form, KeyCode::Down);
    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let x = (0..80).find(|&x| buffer[(x, top)].symbol() == "┌").unwrap();

    assert_eq!(buffer[(x, top)].fg, theme::pal().overlay0);

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn alias_targets_share_prompt_path_completion() {
    let (directory, mut form) = fixture("paths");

    std::fs::create_dir_all(directory.join("browse/child")).unwrap();
    std::fs::write(directory.join("browse/guide.md"), "guide").unwrap();
    form.focus = super::add_item(&form);
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    form.paste("notes");
    form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    form.paste(&format!("{}/browse/", directory.display()));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 100)).unwrap();

    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();
    let buffer = terminal.backend().buffer();
    let line = |y| (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>();

    assert!((0..100).any(|y| line(y).contains("guide.md")));

    let target = form.fields[PLAIN + 1].text();

    assert!(matches!(
        form.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::None
    ));
    assert_eq!(form.fields[PLAIN + 1].text(), target);
    assert_eq!((form.focus, form.editing), (PLAIN, Some(PLAIN + 1)));

    complete_alias_target(&directory, &mut form, terminal, &target);
}

fn complete_alias_target(
    directory: &std::path::Path,
    form: &mut Form,
    mut terminal: ratatui::Terminal<ratatui::backend::TestBackend>,
    target: &str,
) {
    // enter: the alias locked; ↓: the next item (the add row); ↑: back on the alias
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(
        form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        Action::None
    ));
    assert_eq!((form.focus, form.editing), (PLAIN + 1, None));

    form.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    assert_eq!((form.focus, form.editing), (PLAIN, None));

    // back into the box: a new field resets the dropdown's Escape state
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    form.fields[PLAIN + 1].move_end();

    assert!(matches!(
        form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Action::None
    ));
    assert_eq!(
        form.fields[PLAIN + 1].text(),
        format!("{}/browse/child/", directory.display())
    );

    form.fields[PLAIN + 1].set_text(target);
    form.fields[PLAIN + 1].move_end();

    // ↑↓ + Enter chooses a file, just like the prompt.
    form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        form.fields[PLAIN + 1].text(),
        format!("{}/browse/guide.md ", directory.display())
    );

    form.fields[PLAIN + 1].set_text(target);
    form.fields[PLAIN + 1].move_end();
    terminal.draw(|frame| form.draw(frame, frame.area(), true)).unwrap();
    let buffer = terminal.backend().buffer();

    let (x, y) = (0..100)
        .find_map(|y| {
            let line = (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>();

            // a byte offset: box chars (`│`) take 3 bytes, 1 column
            line.find("guide.md").map(|i| (line[..i].chars().count() as u16, y))
        })
        .unwrap();

    assert!(matches!(
        form.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE
        }),
        Action::None
    ));
    assert_eq!(
        form.fields[PLAIN + 1].text(),
        format!("{}/browse/guide.md ", directory.display())
    );

    let _ = std::fs::remove_dir_all(directory);
}
