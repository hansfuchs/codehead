use super::{Frame, Merge, PrRef, Rect, cache_read};

impl Merge {
    /// The merge tab in AREA: the cached PR at once, a read's new data dissolves in.
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        draw_merge(self, frame, area, focused);
    }
}

fn draw_merge(widget_state: &mut Merge, frame: &mut Frame, area: Rect, focused: bool) {
    if let Some(pr) = widget_state.current().cloned()
        && !cfg!(test)
    {
        load_cached_pull_request(widget_state, &(pr));
    }

    widget_state.draw_now(frame, area, focused);
    widget_state.fx.apply(frame, area, widget_state.version);
}

fn load_cached_pull_request(widget_state: &mut Merge, pr: &PrRef) {
    let thread = widget_state.tabs.entry(pr.clone()).or_default();

    if !thread.reads.cached {
        thread.reads.cached = true;

        if thread.pr.is_none() {
            thread.pr = cache_read(pr);
        }
    }
}
