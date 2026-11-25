use super::{
    HOME_CHOICES, HomeChoice, KeyCode, KeyEvent, KeyModifiers, PRESETS, SCREENS, Screen, Wizard, clean_name, edit,
};

impl Wizard {
    pub(super) fn key(&mut self, key: KeyEvent) {
        dispatch_key(self, key);
    }
}

fn dispatch_key(wizard: &mut Wizard, key: KeyEvent) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        wizard.quit = true;
        return;
    }

    let scr = SCREENS[wizard.screen];

    match (scr, key.code) {
        // every screen, every state
        (_, KeyCode::Esc) => wizard.back(),
        (Screen::Harness, _) => key_harness(wizard, key),
        (Screen::Home, _) => key_home(wizard, key),
        (Screen::Crew, _) => key_crew(wizard, key),

        // the tutorial ended without its last step (or failed to start): again, or skip it
        (Screen::Try, KeyCode::Enter) => wizard.launch = true,
        (Screen::Try, KeyCode::Right) => wizard.go(wizard.screen + 1),
        (Screen::Next, KeyCode::Enter) => wizard.quit = true,
        _ => {}
    }
}

fn copy_install_command(wizard: &mut Wizard) {
    if let Some(harness) = wizard.harness_rows().get(wizard.harness_state.hcursor) {
        wizard.harness_state.note = if crate::infra::clip::copy(harness.install) {
            format!("copied: {}", harness.install)
        } else {
            "0 clipboard".into()
        };
    }
}

fn accept_home_choice(wizard: &mut Wizard) {
    let choice = HOME_CHOICES[wizard.home_state.home].0;

    if choice != HomeChoice::Skip && wizard.home_state.paths[wizard.home_state.home].trim().is_empty() {
        wizard.harness_state.note = if choice == HomeChoice::Clone {
            "type or paste the git URL".into()
        } else {
            "type a path".into()
        };

        return;
    }

    // clone: its crew comes from home (the dry run shows the crew screen anyway)
    let skip_crew = choice == HomeChoice::Clone && !wizard.dry;

    wizard.go(if skip_crew { 3 } else { 2 });
}

fn key_harness(wizard: &mut Wizard, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            wizard.harness_state.hcursor = wizard.harness_state.hcursor.saturating_sub(1);
        }
        KeyCode::Down => {
            wizard.harness_state.hcursor =
                (wizard.harness_state.hcursor + 1).min(wizard.harness_rows().len().saturating_sub(1));
        }
        KeyCode::Char('r') => {
            wizard.detect();
            wizard.harness_state.note = format!("{} ready", wizard.harness_state.installed.len());
        }
        KeyCode::Char('c') => {
            copy_install_command(wizard);
        }
        KeyCode::Enter => {
            if wizard.harness_state.installed.is_empty() && !wizard.dry {
                wizard.harness_state.note = "install 1 harness first, then r".into();
            } else {
                wizard.go(1);
            }
        }
        _ => {}
    }
}

fn key_home(wizard: &mut Wizard, key: KeyEvent) {
    match key.code {
        KeyCode::Up => wizard.home_state.home = wizard.home_state.home.saturating_sub(1),
        KeyCode::Down => {
            wizard.home_state.home = (wizard.home_state.home + 1).min(HOME_CHOICES.len() - 1);
        }
        KeyCode::Enter => accept_home_choice(wizard),
        _ if HOME_CHOICES[wizard.home_state.home].0 != HomeChoice::Skip => {
            edit(&mut wizard.home_state.paths[wizard.home_state.home], &key);
        }
        _ => {}
    }
}

fn key_crew(wizard: &mut Wizard, key: KeyEvent) {
    match key.code {
        KeyCode::Left => {
            wizard.crew_state.preset = (wizard.crew_state.preset + PRESETS.len() - 1) % PRESETS.len();
            wizard.build_crew();
        }
        KeyCode::Right => {
            wizard.crew_state.preset = (wizard.crew_state.preset + 1) % PRESETS.len();
            wizard.build_crew();
        }
        KeyCode::Down => {
            wizard.crew_state.focus = (wizard.crew_state.focus + 1) % wizard.crew_state.crew.len();
        }
        KeyCode::Up => {
            wizard.crew_state.focus =
                (wizard.crew_state.focus + wizard.crew_state.crew.len() - 1) % wizard.crew_state.crew.len();
        }
        KeyCode::Char(' ') => {
            let avatar = wizard.random_avatar();
            let spawn = wizard.spawn();
            let member = &mut wizard.crew_state.crew[wizard.crew_state.focus];

            member.avatar = avatar;
            member.spawn = spawn;
        }
        KeyCode::Enter => {
            if let Some(why) = wizard.crew_problem() {
                wizard.harness_state.note = why;
            } else {
                wizard.go(3);
            }
        }
        _ => {
            let mut name = std::mem::take(&mut wizard.crew_state.crew[wizard.crew_state.focus].name);

            if edit(&mut name, &key) {
                name = clean_name(&name);
            }

            wizard.crew_state.crew[wizard.crew_state.focus].name = name;
        }
    }
}
