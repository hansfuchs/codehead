use super::Menu;

impl Menu {
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
            .map(|msgs| crate::domain::search::find(msgs, &query))
            .unwrap_or_default();
        self.search_state.hits_for = query;
        self.search_state.scursor = 0;
        self.search_state.soffset = 0;
    }
}
