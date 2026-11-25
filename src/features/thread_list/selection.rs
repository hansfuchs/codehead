use super::{Card, Focus, KeyEvent, Mode, Position, Rect, ThreadList, View, thread_card};

impl ThreadList {
    /// Section of card C: 0 open (working, a question, work not in the base yet, 0 code yet, 0
    /// landed state read yet), 1 stale (open, 0 code, idle 3 days), 2 shipped.
    pub(super) fn section(character: &Card) -> u8 {
        let stale = || character.stale(crate::engine::now_ms());

        if character.agents.busy || character.land.as_ref().is_some_and(crate::domain::landed::Land::open) {
            0
        } else if character.agents.blocked {
            u8::from(stale())
        } else if character.shipped() {
            2
        } else {
            u8::from(stale())
        }
    }
    /// The cards of the view (filter, repo and project tabs applied), match / activity order.
    pub(super) fn base(&self) -> Vec<usize> {
        base_thread_list(self)
    }
    /// The cards of the view, status tab applied (active: by section, then match / activity).
    pub(crate) fn visible(&self) -> Vec<usize> {
        let mut value = self.base();

        if self.has_stats() {
            let st = self.stat_tab();

            value.retain(|&i| st.keeps(Self::section(&self.cards[i])));
            value.sort_by_key(|&i| Self::section(&self.cards[i]));
        }

        value
    }
    pub(crate) fn current(&self) -> Option<&Card> {
        self.visible().get(self.cursor).map(|&i| &self.cards[i])
    }
    /// The targets of an action: the selection (manage), else the card under the cursor.
    pub(super) fn targets(&self) -> Vec<Card> {
        if self.mode == Mode::Manage && !self.sel.is_empty() {
            return self
                .visible()
                .into_iter()
                .map(|i| self.cards[i].clone())
                .filter(|character| self.sel.contains(&character.id))
                .collect();
        }

        self.current().cloned().into_iter().collect()
    }
    /// Rows of the view (the host's views: their count).
    pub(super) fn count(&self) -> usize {
        match self.view {
            View::Worktrees => self.extra[0].0,
            View::Claude => self.extra[1].0,
            _ => self.visible().len(),
        }
    }
    pub(super) fn type_filter(&mut self, key_value: KeyEvent) {
        let before = self.filter.text();

        self.filter.key(key_value);

        if self.filter.text() != before {
            self.cursor = 0;
            self.offset = 0;
        }
    }
    /// The host's top row: ↑ goes to the view tabs.
    pub(crate) fn focus_views(&mut self) {
        self.focus = Focus::Views;
    }
    /// A card / tab under AT (hover).
    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        self.hit_regions
            .act_rows
            .iter()
            .map(|(result, _)| *result)
            .chain(self.hit_regions.rows.iter().map(|(result_value, _)| *result_value))
            .find(|result_2| result_2.contains(at))
    }
}

fn base_thread_list(state: &ThreadList) -> Vec<usize> {
    if !state.view.cards() {
        return Vec::new();
    }

    let tab = state.repo_tab();
    let proj = state.proj_tab();
    let archived = state.view == View::Archived;

    thread_card::filter(&state.cards, &state.filter.text())
        .into_iter()
        .filter(|&i| {
            state.cards[i].archived() == archived
                && tab.keeps(state.cards[i].repo.as_deref())
                && proj.keeps(&state.cards[i].projects)
        })
        .collect()
}
