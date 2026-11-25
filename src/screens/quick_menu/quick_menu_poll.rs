use super::{Instant, MSGS_EVERY, Menu, Tab};

impl Menu {
    pub(super) fn poll(&mut self) {
        poll_tabs(self);
    }
}

fn poll_tabs(menu: &mut Menu) {
    if menu.tab == Tab::Merge {
        menu.prs.ensure();
        menu.prs.poll();
    }

    // the threads tab: a new read every 2s while it shows
    menu.list.poll(menu.tab == Tab::Threads);
    let due = |at: Option<Instant>, every| at.is_none_or(|read_at: Instant| read_at.elapsed() >= every);

    if let Some(rx) = &menu.search_state.msgs_rx
        && let Ok(msgs) = rx.try_recv()
    {
        menu.search_state.msgs_rx = None;
        menu.set_msgs(msgs);
    }

    // the search tab: a new read on its 1st show, then every 5s while it shows
    if menu.tab == Tab::Search
        && menu.search_state.msgs_rx.is_none()
        && due(menu.search_state.msgs_at, MSGS_EVERY)
        && !cfg!(test)
    {
        let (tx, rx) = crate::infra::bus::channel();

        std::thread::spawn(move || {
            let _ = tx.send(crate::domain::search::load());
        });

        menu.search_state.msgs_rx = Some(rx);
        menu.search_state.msgs_at = Some(Instant::now());
    }

    menu.refresh_hits();
}
