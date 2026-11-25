use super::{Act, KeyCode, KeyEvent, KeyModifiers, PathBuf, View, share, store};

impl View {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Act {
        key_view(self, key_value)
    }

    fn key_body_state(&mut self) {
        key_body(self);
    }
}

fn key_keycode_esc(view_state: &mut View) {
    view_state.filter_input.filtering = false;
    view_state.filter_input.filter.set_text("");
    view_state.rebuild();
}

fn key_match(key_value: KeyEvent, view_state: &mut View) {
    let before = view_state.filter_input.filter.text();

    view_state.filter_input.filter.key(key_value);

    if view_state.filter_input.filter.text() != before {
        view_state.rebuild();
        view_state.at = 0;
        view_state.text_preview.scroll = 0;
    }
}

fn key_body(view_state: &mut View) {
    key_keycode_esc(view_state);
}

fn key_view(state: &mut View, key_value: KeyEvent) -> Act {
    // the filter input: typing filters; ↑↓ move; enter keeps the matches; esc clears
    if state.filter_input.filtering {
        match key_value.code {
            KeyCode::Esc => {
                state.key_body_state();
            }
            KeyCode::Enter => state.filter_input.filtering = false,
            KeyCode::Down => state.step(1),
            KeyCode::Up => state.step(-1),
            _ => {
                key_match(key_value, state);
            }
        }

        return Act::None;
    }

    // ctrl+f: the filter (the only key to it, in every list)
    if crate::ui::textarea::is_filter_key(&key_value) {
        state.filter_input.filtering = true;
        return Act::None;
    }

    // a letter with ctrl / alt / ⌘: 0 view key (alt+S never shares)
    if matches!(key_value.code, KeyCode::Char(_))
        && key_value
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return Act::None;
    }

    let armed = state
        .share_task
        .share_armed
        .take()
        .filter(|thread_value| thread_value.elapsed() < std::time::Duration::from_secs(3));
    let page = usize::from(state.preview.height.saturating_sub(3)).max(1);

    finish_key_view(state, key_value, armed, page)
}

fn finish_key_view(state: &mut View, key_value: KeyEvent, armed: Option<std::time::Instant>, page: usize) -> Act {
    match key_value.code {
        KeyCode::Down | KeyCode::Char('j') => state.step(1),
        KeyCode::Up | KeyCode::Char('k') => state.step(-1),
        KeyCode::PageDown | KeyCode::Char(' ') => state.text_preview.scroll += page,
        KeyCode::PageUp => state.text_preview.scroll = state.text_preview.scroll.saturating_sub(page),
        KeyCode::Char('g') | KeyCode::Home => state.text_preview.scroll = 0,
        // e: the file in the IDE, as on every screen
        KeyCode::Char('e') => return state.cur().map_or(Act::None, |error| Act::Open(error.path.clone())),
        KeyCode::Char('r') => state.refresh(true),
        // S S: share the thread's home repo files (commit those paths, push if tracked)
        KeyCode::Char('S') => {
            let Some(home) = crate::infra::config::home_repo() else {
                return Act::Toast("0 home repo (setting home)".into());
            };

            if armed.is_none() {
                state.share_task.share_armed = Some(std::time::Instant::now());
                return Act::Toast("S again: share (commit + push the thread's home repo files)".into());
            }

            let paths: Vec<PathBuf> = state.files.iter().map(|error_value| error_value.path.clone()).collect();
            let title = store::load(&state.id)
                .map(|thread_2| thread_2.title)
                .unwrap_or_default();
            let run = move || share(&home, &paths, &title).unwrap_or_else(|error_2| format!("share: {error_2}"));

            // 4 git runs: off the UI thread (tests: at once)
            if cfg!(test) {
                return Act::Toast(run());
            }

            let (tx, rx) = crate::infra::bus::channel();

            state.share_task.shared = Some(rx);

            std::thread::spawn(move || {
                let _ = tx.send(run());
            });

            return Act::Toast("sharing…".into());
        }
        _ => {}
    }

    Act::None
}
