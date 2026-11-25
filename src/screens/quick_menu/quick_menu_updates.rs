use super::{Instant, Menu, Tab};

impl Menu {
    /// Tokens tab: a new read every 2s while it shows.
    pub(super) fn poll_detail(&mut self) {
        poll_token_details(self);
    }

    /// The hits of the query, if it changed (or the messages came).
    pub(super) fn refresh_hits(&mut self) {
        let query = self.search_state.search.text();

        if query == self.search_state.hits_for {
            return;
        }

        self.search_state.hits = self
            .search_state
            .msgs
            .as_deref()
            .map(|message| crate::domain::search::find(message, &query))
            .unwrap_or_default();
        self.search_state.hits_for = query;
        self.search_state.scursor = 0;
        self.search_state.soffset = 0;
    }
}

fn poll_token_details(widget_state: &mut Menu) {
    if let Some(rx) = &widget_state.tokens.detail_rx
        && let Ok(directory) = rx.try_recv()
    {
        widget_state.tokens.detail = Some(directory);
        widget_state.tokens.detail_rx = None;
    }

    let due = widget_state
        .tokens
        .detail_at
        .is_none_or(|thread_value| thread_value.elapsed() >= std::time::Duration::from_secs(2));

    if widget_state.tab == Tab::Tokens && widget_state.tokens.detail_rx.is_none() && due && !cfg!(test) {
        let (tx, rx) = crate::infra::bus::channel();
        let id = widget_state.thread.clone();

        std::thread::spawn(move || {
            let _ = tx.send(crate::features::cost::detail(id.as_deref()));
        });

        widget_state.tokens.detail_rx = Some(rx);
        widget_state.tokens.detail_at = Some(Instant::now());
    }
}
