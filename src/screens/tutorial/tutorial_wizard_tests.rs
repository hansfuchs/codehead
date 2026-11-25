use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn text(wizard: &Wizard) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 36)).unwrap();

    terminal.draw(|frame| wizard.draw(frame)).unwrap();
    terminal
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

fn esc(wizard: &mut Wizard) {
    wizard.key(key(KeyCode::Esc));
}

fn typed(wizard: &mut Wizard, text: &str) {
    for character in text.chars() {
        wizard.key(key(KeyCode::Char(character)));
    }
}

#[test]
fn dry_run_walks_every_screen_and_the_tutorial() {
    let mut wizard = Wizard::new(true);

    assert_eq!(wizard.screen, 0, "the dry run shows the harness screen");
    assert!(text(&wizard).contains("harnesses") && text(&wizard).contains("dry run"));

    wizard.key(key(KeyCode::Enter));

    assert!(text(&wizard).contains("home repo") && text(&wizard).contains("codehead/agents/"));

    wizard.key(key(KeyCode::Down));

    // clone: 0 URL → stays, says why
    wizard.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[wizard.screen], Screen::Home);
    assert!(wizard.harness_state.note.contains("git URL"));

    wizard.paste("git@github.com:me/home.git\nignored 2nd line");

    assert_eq!(wizard.home_state.paths[1], "git@github.com:me/home.git");

    wizard.key(key(KeyCode::Enter));

    assert_eq!(
        SCREENS[wizard.screen],
        Screen::Crew,
        "dry run: clone still shows the crew"
    );
    assert_eq!(wizard.crew_state.crew.len(), 2, "pair by default");

    wizard.key(key(KeyCode::Right));

    assert_eq!(wizard.crew_state.crew.len(), 3);

    wizard.key(key(KeyCode::Left));

    // typing renames the focused agent at once
    wizard.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut wizard, "1ada x");

    assert_eq!(
        wizard.crew_state.crew[0].name, "ADAX",
        "letter 1st, upper case, 0 space"
    );

    wizard.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[wizard.screen], Screen::Try);
    assert!(wizard.launch, "the tutorial process starts next");

    // its spec: the crew as typed
    let spec: crate::screens::tutorial::Spec = serde_json::from_str(&wizard.spec()).unwrap();

    assert!(spec.dry && spec.crew[0].name == "ADAX" && spec.crew[1].role == "reviewer");

    // the tutorial ran through: the next screen
    wizard.launch = false;
    wizard.tutorial_ended(Some(crate::screens::tutorial::DONE));

    assert_eq!(SCREENS[wizard.screen], Screen::Next);
    assert!(
        wizard
            .plan()
            .iter()
            .any(|line| line.starts_with("home: git clone git@github.com:me/home.git"))
    );

    wizard.key(key(KeyCode::Enter));

    assert!(wizard.quit);
}

#[test]
fn esc_goes_back_1_screen_from_every_screen_and_state() {
    let mut wizard = Wizard::new(true);

    for i in 1..SCREENS.len() {
        wizard.go(i);
    }

    // the tutorial left early (esc esc in its terminal): back, as esc; a typed name, a typed
    // path on the way: esc still goes back
    wizard.go(3);
    wizard.tutorial_ended(Some(crate::screens::tutorial::BACK));

    assert_eq!(SCREENS[wizard.screen], Screen::Next);

    wizard.go(3);
    let mut seen = vec![wizard.screen];

    while !wizard.quit {
        assert_screen_back_path(&mut wizard, &mut seen);
    }

    // 0 → 1 → 2 → 3 → 4 → (go 3 again) → back the same way
    assert_eq!(seen, vec![3, 4, 3, 2, 1, 0], "{seen:?}");
    assert!(wizard.quit, "esc on the 1st screen quits");
}

fn assert_screen_back_path(wizard: &mut Wizard, seen: &mut Vec<usize>) {
    esc(wizard);

    if !wizard.quit {
        seen.push(wizard.screen);

        if SCREENS[wizard.screen] == Screen::Crew {
            typed(wizard, "Q");
        }

        if SCREENS[wizard.screen] == Screen::Home {
            typed(wizard, "/x");
        }
    }
}

#[test]
fn a_real_run_starting_on_home_quits_on_esc_and_skips_crew_on_clone() {
    let mut wizard = Wizard::new(false);

    if wizard.harness_state.installed.is_empty() {
        return; // 0 harness on this machine: the harness screen comes 1st
    }

    assert_eq!(SCREENS[wizard.screen], Screen::Home);

    wizard.home_state.home = 1;
    wizard.paste("https://github.com/me/home.git");
    wizard.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[wizard.screen], Screen::Try, "clone: the crew comes from home");

    esc(&mut wizard);

    assert_eq!(SCREENS[wizard.screen], Screen::Home, "back over the skipped screen");

    esc(&mut wizard);

    assert!(wizard.quit);
}

#[test]
fn crew_needs_names_that_differ() {
    let mut wizard = Wizard::new(true);

    wizard.go(2);
    wizard.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    wizard.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[wizard.screen], Screen::Crew);
    assert!(
        wizard.harness_state.note.contains("needs a name"),
        "{}",
        wizard.harness_state.note
    );

    let other = wizard.crew_state.crew[1].name.clone();

    typed(&mut wizard, &other);
    wizard.key(key(KeyCode::Enter));

    assert!(
        wizard.harness_state.note.starts_with("2 agents named"),
        "{}",
        wizard.harness_state.note
    );

    wizard.key(key(KeyCode::Backspace));
    wizard.key(key(KeyCode::Enter));

    assert_eq!(SCREENS[wizard.screen], Screen::Try);

    // space: a new avatar, the name stays
    let mut wizard = Wizard::new(true);

    wizard.go(2);
    let name = wizard.crew_state.crew[0].name.clone();

    wizard.key(key(KeyCode::Char(' ')));

    assert_eq!(wizard.crew_state.crew[0].name, name);
}

#[test]
fn the_tutorial_screen_starts_again_skips_or_quits() {
    let mut wizard = Wizard::new(true);

    wizard.go(3);
    wizard.launch = false;

    // a start that failed: the reason stays on the screen, enter starts again
    wizard.tutorial_ended(None);

    assert!(wizard.harness_state.note.starts_with("tutorial: exit") && text(&wizard).contains("did not start"));

    wizard.key(key(KeyCode::Enter));

    assert!(wizard.launch);

    // tab: skip it
    wizard.key(key(KeyCode::Right));

    assert_eq!(SCREENS[wizard.screen], Screen::Next);

    // ctrl+c ctrl+c in the tutorial's terminal: the wizard ends too
    wizard.go(3);
    wizard.tutorial_ended(Some(crate::screens::tutorial::QUIT));

    assert!(wizard.quit);
}
