use super::{
    HOME_CHOICES, HomeChoice, KeyCode, KeyEvent, KeyModifiers, PRESETS, SCREENS, Screen, Wizard, clean_name, edit,
};

impl Wizard {
    pub(super) fn key(&mut self, key_value: KeyEvent) {
        dispatch_key(self, key_value);
    }
}

fn dispatch_key(widget_state: &mut Wizard, key_value: KeyEvent) {
    if key_value.code == KeyCode::Char('c') && key_value.modifiers.contains(KeyModifiers::CONTROL) {
        widget_state.quit = true;
        return;
    }

    let scr = SCREENS[widget_state.screen];

    match (scr, key_value.code) {
        // every screen, every state
        (_, KeyCode::Esc) => widget_state.back(),
        (Screen::Harness, _) => key_harness(widget_state, key_value),
        (Screen::Home, _) => key_home(widget_state, key_value),
        (Screen::Crew, _) => key_crew(widget_state, key_value),

        // the tutorial ended without its last step (or failed to start): again, or skip it
        (Screen::Try, KeyCode::Enter) => widget_state.launch = true,
        (Screen::Try, KeyCode::Right) => widget_state.go(widget_state.screen + 1),
        (Screen::Next, KeyCode::Enter) => widget_state.quit = true,
        _ => {}
    }
}

fn copy_install_command(widget_state: &mut Wizard) {
    if let Some(height) = widget_state.harness_rows().get(widget_state.harness_state.hcursor) {
        widget_state.harness_state.note = if crate::infra::clip::copy(height.install) {
            format!("copied: {}", height.install)
        } else {
            "0 clipboard".into()
        };
    }
}

fn accept_home_choice(widget_state: &mut Wizard) {
    let choice = HOME_CHOICES[widget_state.home_state.home].0;

    if choice != HomeChoice::Skip
        && widget_state.home_state.paths[widget_state.home_state.home]
            .trim()
            .is_empty()
    {
        widget_state.harness_state.note = if choice == HomeChoice::Clone {
            "type or paste the git URL".into()
        } else {
            "type a path".into()
        };

        return;
    }

    let skip_crew = choice == HomeChoice::Clone && !widget_state.dry;

    widget_state.go(if skip_crew { 3 } else { 2 });

    // clone: its crew comes from home (the dry run shows the crew screen anyway)
}

fn key_harness(widget_state: &mut Wizard, key_value: KeyEvent) {
    match key_value.code {
        KeyCode::Up => {
            widget_state.harness_state.hcursor = widget_state.harness_state.hcursor.saturating_sub(1);
        }
        KeyCode::Down => {
            widget_state.harness_state.hcursor =
                (widget_state.harness_state.hcursor + 1).min(widget_state.harness_rows().len().saturating_sub(1));
        }
        KeyCode::Char('r') => {
            widget_state.detect();
            widget_state.harness_state.note = format!("{} ready", widget_state.harness_state.installed.len());
        }
        KeyCode::Char('c') => {
            copy_install_command(widget_state);
        }
        KeyCode::Enter => {
            if widget_state.harness_state.installed.is_empty() && !widget_state.dry {
                widget_state.harness_state.note = "install 1 harness first, then r".into();
            } else {
                widget_state.go(1);
            }
        }
        _ => {}
    }
}

fn key_home(widget_state: &mut Wizard, key_value: KeyEvent) {
    match key_value.code {
        KeyCode::Up => widget_state.home_state.home = widget_state.home_state.home.saturating_sub(1),
        KeyCode::Down => {
            widget_state.home_state.home = (widget_state.home_state.home + 1).min(HOME_CHOICES.len() - 1);
        }
        KeyCode::Enter => accept_home_choice(widget_state),
        _ if HOME_CHOICES[widget_state.home_state.home].0 != HomeChoice::Skip => {
            edit(
                &mut widget_state.home_state.paths[widget_state.home_state.home],
                &key_value,
            );
        }
        _ => {}
    }
}

fn key_crew(widget_state: &mut Wizard, key_value: KeyEvent) {
    match key_value.code {
        KeyCode::Left => {
            widget_state.crew_state.preset = (widget_state.crew_state.preset + PRESETS.len() - 1) % PRESETS.len();
            widget_state.build_crew();
        }
        KeyCode::Right => {
            widget_state.crew_state.preset = (widget_state.crew_state.preset + 1) % PRESETS.len();
            widget_state.build_crew();
        }
        KeyCode::Down => {
            widget_state.crew_state.focus = (widget_state.crew_state.focus + 1) % widget_state.crew_state.crew.len();
        }
        KeyCode::Up => {
            widget_state.crew_state.focus = (widget_state.crew_state.focus + widget_state.crew_state.crew.len() - 1)
                % widget_state.crew_state.crew.len();
        }
        KeyCode::Char(' ') => {
            let agent = widget_state.random_avatar();
            let text = widget_state.spawn();
            let message = &mut widget_state.crew_state.crew[widget_state.crew_state.focus];

            message.avatar = agent;
            message.spawn = text;
        }
        KeyCode::Enter => {
            if let Some(why) = widget_state.crew_problem() {
                widget_state.harness_state.note = why;
            } else {
                widget_state.go(3);
            }
        }
        _ => {
            let mut name = std::mem::take(&mut widget_state.crew_state.crew[widget_state.crew_state.focus].name);

            if edit(&mut name, &key_value) {
                name = clean_name(&name);
            }

            widget_state.crew_state.crew[widget_state.crew_state.focus].name = name;
        }
    }
}
