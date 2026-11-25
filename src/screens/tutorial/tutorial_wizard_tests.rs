use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key(character: KeyCode) -> KeyEvent {
    KeyEvent::new(character, KeyModifiers::NONE)
}

fn text(width: &Wizard) -> String {
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 36)).unwrap();

    thread.draw(|file| width.draw(file)).unwrap();
    thread
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<Vec<_>>()
        .chunks(90)
        .map(<[&str]>::concat)
        .collect::<Vec<_>>()
        .join("\n")
}

fn esc(width: &mut Wizard) {
    width.key(key(KeyCode::Esc));
}

fn typed(width: &mut Wizard, text: &str) {
    for character in text.chars() {
        width.key(key(KeyCode::Char(character)));
    }
}

#[test]
fn dry_run_walks_every_screen_and_the_tutorial() {
    let mut width = Wizard::new(true);

    assert_eq!(width.screen, 0, "the dry run shows the harness screen");
    assert!(text(&width).contains("harnesses") && text(&width).contains("dry run"));

    width.key(key(KeyCode::Enter));

    assert!(text(&width).contains("home repo") && text(&width).contains("codehead/agents/"));

    width.key(key(KeyCode::Down));

    // clone: 0 URL → stays, says why
    width.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[width.screen], Screen::Home);
    assert!(width.harness_state.note.contains("git URL"));

    width.paste("git@github.com:me/home.git\nignored 2nd line");

    assert_eq!(width.home_state.paths[1], "git@github.com:me/home.git");

    width.key(key(KeyCode::Enter));

    assert_eq!(
        SCREENS[width.screen],
        Screen::Crew,
        "dry run: clone still shows the crew"
    );
    assert_eq!(width.crew_state.crew.len(), 2, "pair by default");

    width.key(key(KeyCode::Right));

    assert_eq!(width.crew_state.crew.len(), 3);

    width.key(key(KeyCode::Left));

    // typing renames the focused agent at once
    width.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut width, "1ada x");

    assert_eq!(width.crew_state.crew[0].name, "ADAX", "letter 1st, upper case, 0 space");

    width.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[width.screen], Screen::Try);
    assert!(width.launch, "the tutorial process starts next");

    // its spec: the crew as typed
    let spec: crate::screens::tutorial::Spec = serde_json::from_str(&width.spec()).unwrap();

    assert!(spec.dry && spec.crew[0].name == "ADAX" && spec.crew[1].role == "reviewer");

    // the tutorial ran through: the next screen
    width.launch = false;
    width.tutorial_ended(Some(crate::screens::tutorial::DONE));

    assert_eq!(SCREENS[width.screen], Screen::Next);
    assert!(
        width
            .plan()
            .iter()
            .any(|line| line.starts_with("home: git clone git@github.com:me/home.git"))
    );

    width.key(key(KeyCode::Enter));

    assert!(width.quit);
}

#[test]
fn esc_goes_back_1_screen_from_every_screen_and_state() {
    let mut width = Wizard::new(true);

    for i in 1..SCREENS.len() {
        width.go(i);
    }

    // the tutorial left early (esc esc in its thread): back, as esc; a typed name, a typed
    // path on the way: esc still goes back
    width.go(3);
    width.tutorial_ended(Some(crate::screens::tutorial::BACK));

    assert_eq!(SCREENS[width.screen], Screen::Next);

    width.go(3);
    let mut seen = vec![width.screen];

    while !width.quit {
        assert_screen_back_path(&mut width, &mut seen);
    }

    // 0 → 1 → 2 → 3 → 4 → (go 3 again) → back the same way
    assert_eq!(seen, vec![3, 4, 3, 2, 1, 0], "{seen:?}");
    assert!(width.quit, "esc on the 1st screen quits");
}

fn assert_screen_back_path(width: &mut Wizard, seen: &mut Vec<usize>) {
    esc(width);

    if !width.quit {
        seen.push(width.screen);

        if SCREENS[width.screen] == Screen::Crew {
            typed(width, "Q");
        }

        if SCREENS[width.screen] == Screen::Home {
            typed(width, "/x");
        }
    }
}

#[test]
fn a_real_run_starting_on_home_quits_on_esc_and_skips_crew_on_clone() {
    let mut width = Wizard::new(false);

    if width.harness_state.installed.is_empty() {
        return; // 0 harness on this machine: the harness screen comes 1st
    }

    assert_eq!(SCREENS[width.screen], Screen::Home);

    width.home_state.home = 1;
    width.paste("https://github.com/me/home.git");
    width.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[width.screen], Screen::Try, "clone: the crew comes from home");

    esc(&mut width);

    assert_eq!(SCREENS[width.screen], Screen::Home, "back over the skipped screen");

    esc(&mut width);

    assert!(width.quit);
}

#[test]
fn crew_needs_names_that_differ() {
    let mut width = Wizard::new(true);

    width.go(2);
    width.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    width.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[width.screen], Screen::Crew);
    assert!(
        width.harness_state.note.contains("needs a name"),
        "{}",
        width.harness_state.note
    );

    let other = width.crew_state.crew[1].name.clone();

    typed(&mut width, &other);
    width.key(key(KeyCode::Enter));

    assert!(
        width.harness_state.note.starts_with("2 agents named"),
        "{}",
        width.harness_state.note
    );

    width.key(key(KeyCode::Backspace));
    width.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[width.screen], Screen::Try);

    // space: a new avatar, the name stays
    let mut width_value = Wizard::new(true);

    width_value.go(2);
    let name = width_value.crew_state.crew[0].name.clone();

    width_value.key(key(KeyCode::Char(' ')));

    assert_eq!(width_value.crew_state.crew[0].name, name);
}

#[test]
fn the_tutorial_screen_starts_again_skips_or_quits() {
    let mut width = Wizard::new(true);

    width.go(3);
    width.launch = false;

    // a start that failed: the reason stays on the screen, enter starts again
    width.tutorial_ended(None);

    assert!(width.harness_state.note.starts_with("tutorial: exit") && text(&width).contains("did not start"));

    width.key(key(KeyCode::Enter));

    assert!(width.launch);

    // tab: skip it
    width.key(key(KeyCode::Right));

    assert_eq!(SCREENS[width.screen], Screen::Next);

    // ctrl+c ctrl+c in the tutorial's thread: the wizard ends too
    width.go(3);
    width.tutorial_ended(Some(crate::screens::tutorial::QUIT));

    assert!(width.quit);
}
