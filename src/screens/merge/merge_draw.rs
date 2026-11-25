use super::{Frame, Merge, PrRef, Rect, cache_read};

impl Merge {
    /// The merge tab in AREA: the cached PR at once, a read's new data dissolves in.
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        draw_merge(self, frame, area, focused);
    }
}

fn draw_merge(merge: &mut Merge, frame: &mut Frame, area: Rect, focused: bool) {
    if let Some(pr) = merge.current().cloned()
        && !cfg!(test)
    {
        load_cached_pull_request(merge, &pr);
    }

    merge.draw_now(frame, area, focused);
    merge.fx.apply(frame, area, merge.version);
}

fn load_cached_pull_request(merge: &mut Merge, pr: &PrRef) {
    let tab = merge.tabs.entry(pr.clone()).or_default();

    if !tab.reads.cached {
        tab.reads.cached = true;

        if tab.pr.is_none() {
            tab.pr = cache_read(pr);
        }
    }
}
