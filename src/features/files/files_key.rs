use super::{Act, KeyCode, KeyEvent, KeyModifiers, PathBuf, View, share, store};

impl View {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Act {
        key_view(self, key)
    }

    fn close_filter(&mut self) {
        close_filter(self);
    }
}

fn clear_filter(view: &mut View) {
    view.filter_input.filtering = false;
    view.filter_input.filter.set_text("");
    view.rebuild();
}

fn type_in_filter(key: KeyEvent, view: &mut View) {
    let before = view.filter_input.filter.text();

    view.filter_input.filter.key(key);

    if view.filter_input.filter.text() != before {
        view.rebuild();
        view.at = 0;
        view.text_preview.scroll = 0;
    }
}

fn close_filter(view: &mut View) {
    clear_filter(view);
}

fn key_view(view: &mut View, key: KeyEvent) -> Act {
    // the filter input: typing filters; ↑↓ move; enter keeps the matches; esc clears
    if view.filter_input.filtering {
        match key.code {
            KeyCode::Esc => {
                view.close_filter();
            }
            KeyCode::Enter => view.filter_input.filtering = false,
            KeyCode::Down => view.step(1),
            KeyCode::Up => view.step(-1),
            _ => {
                type_in_filter(key, view);
            }
        }

        return Act::None;
    }

    // ctrl+f: the filter (the only key to it, in every list)
    if crate::ui::textarea::is_filter_key(&key) {
        view.filter_input.filtering = true;
        return Act::None;
    }

    // a letter with ctrl / alt / ⌘: 0 view key (alt+S never shares)
    if matches!(key.code, KeyCode::Char(_))
        && key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return Act::None;
    }

    let armed = view
        .share_task
        .share_armed
        .take()
        .filter(|armed_at| armed_at.elapsed() < std::time::Duration::from_secs(3));
    let page = usize::from(view.preview.height.saturating_sub(3)).max(1);

    list_key(view, key, armed, page)
}

fn list_key(view: &mut View, key: KeyEvent, armed: Option<std::time::Instant>, page: usize) -> Act {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => view.step(1),
        KeyCode::Up | KeyCode::Char('k') => view.step(-1),
        KeyCode::PageDown | KeyCode::Char(' ') => view.text_preview.scroll += page,
        KeyCode::PageUp => view.text_preview.scroll = view.text_preview.scroll.saturating_sub(page),
        KeyCode::Char('g') | KeyCode::Home => view.text_preview.scroll = 0,
        // e: the file in the IDE, as on every screen
        KeyCode::Char('e') => return view.cur().map_or(Act::None, |entry| Act::Open(entry.path.clone())),
        KeyCode::Char('r') => view.refresh(true),
        // S S: share the thread's home repo files (commit those paths, push if tracked)
        KeyCode::Char('S') => {
            let Some(home) = crate::infra::config::home_repo() else {
                return Act::Toast("0 home repo (setting home)".into());
            };

            if armed.is_none() {
                view.share_task.share_armed = Some(std::time::Instant::now());
                return Act::Toast("S again: share (commit + push the thread's home repo files)".into());
            }

            let paths: Vec<PathBuf> = view.files.iter().map(|entry| entry.path.clone()).collect();
            let title = store::load(&view.id).map(|thread| thread.title).unwrap_or_default();
            let run = move || share(&home, &paths, &title).unwrap_or_else(|error| format!("share: {error}"));

            // 4 git runs: off the UI thread (tests: at once)
            if cfg!(test) {
                return Act::Toast(run());
            }

            let (tx, rx) = crate::infra::bus::channel();

            view.share_task.shared = Some(rx);

            std::thread::spawn(move || {
                let _ = tx.send(run());
            });

            return Act::Toast("sharing…".into());
        }
        _ => {}
    }

    Act::None
}
