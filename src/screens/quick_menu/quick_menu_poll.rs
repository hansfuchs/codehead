use super::{Instant, MSGS_EVERY, Menu, Tab};

impl Menu {
    pub(super) fn poll(&mut self) {
        poll_messages(self);
    }
}

fn poll_messages(widget_state: &mut Menu) {
    if widget_state.tab == Tab::Merge {
        widget_state.prs.ensure();
        widget_state.prs.poll();
    }

    // the threads tab: a new read every 2s while it shows
    widget_state.list.poll(widget_state.tab == Tab::Threads);
    let due = |at: Option<Instant>, every| at.is_none_or(|thread_value: Instant| thread_value.elapsed() >= every);

    if let Some(rx) = &widget_state.search_state.msgs_rx
        && let Ok(message) = rx.try_recv()
    {
        widget_state.search_state.msgs_rx = None;
        widget_state.set_msgs(message);
    }

    // the search tab: a new read on its 1st show, then every 5s while it shows
    if widget_state.tab == Tab::Search
        && widget_state.search_state.msgs_rx.is_none()
        && due(widget_state.search_state.msgs_at, MSGS_EVERY)
        && !cfg!(test)
    {
        let (tx, rx) = crate::infra::bus::channel();

        std::thread::spawn(move || {
            let _ = tx.send(crate::domain::search::load());
        });

        widget_state.search_state.msgs_rx = Some(rx);
        widget_state.search_state.msgs_at = Some(Instant::now());
    }

    widget_state.refresh_hits();
}
