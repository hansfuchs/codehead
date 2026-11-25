use super::{Rect, View, back_for, chat, item_ts, top_anchor};
impl View {
    pub(super) fn refresh_stream(&mut self, area: Rect) {
        let height = usize::from(area.height);

        let anchor = (self.stream.back > 0)
            .then(|| {
                top_anchor(
                    &self.stream.drawn.starts,
                    self.stream.drawn.rows.len(),
                    self.stream.back,
                    height,
                )
            })
            .flatten();

        self.stream.width = area.width;
        self.find.refresh(&self.thread);
        let logged = self.thread.items.len();

        append_live_text(self);
        let fold = chat::Fold {
            runs: &self.stream.open,
            all_runs: self.stream.all_runs,
            thoughts: self.stream.thoughts,
            open_thoughts: &self.stream.open_thoughts,
            all_thoughts: self.stream.all_thoughts,
        };

        self.stream.drawn = chat::draw(
            &self.thread,
            self.stream.look,
            area.width,
            &self.colors_now(),
            &self.agents.harness_of,
            &fold,
            &self.ask,
        );
        self.thread.items.truncate(logged);
        self.stream.dirty = false;

        if let Some(anchor) = anchor {
            self.stream.back = back_for(&self.stream.drawn.starts, self.stream.drawn.rows.len(), anchor, height);
        }

        apply_message_jump(self, height);
        apply_search_jump(self, height);
    }
}

fn append_live_text(view: &mut View) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_millis() as u64);

    for (agent, text) in &view.stream.live {
        view.thread.items.push(chat::Item::Text {
            agent: agent.clone(),
            text: text.clone(),
            ts: now,
            thoughts: Vec::new(),
            key: 0,
        });
    }
}

fn apply_message_jump(view: &mut View, height: usize) {
    let Some(ts) = view.stream.jump.take() else {
        return;
    };

    // exact (a search hit), else the 1st message at / after it (a notification's event)
    let items = &view.thread.items;

    let at = items
        .iter()
        .position(|item| item_ts(item) == Some(ts))
        .or_else(|| items.iter().position(|item| item_ts(item).is_some_and(|at| at >= ts)))
        .or_else(|| items.len().checked_sub(1));

    match at {
        Some(i) => {
            view.stream.back = back_for(&view.stream.drawn.starts, view.stream.drawn.rows.len(), (i, 0), height)
                .saturating_add(2)
                .min(view.stream.drawn.rows.len().saturating_sub(height));
            view.stream.flash = Some(i);
        }
        None => view.toast("search: the message is gone from the stream"),
    }
}

fn apply_search_jump(view: &mut View, height: usize) {
    if let Some(i) = view.find_to.take().filter(|&i| i < view.stream.drawn.starts.len()) {
        let total = view.stream.drawn.rows.len();

        view.stream.back = back_for(&view.stream.drawn.starts, total, (i, 0), height)
            .saturating_add(height / 3)
            .min(total.saturating_sub(height));
    }
}
