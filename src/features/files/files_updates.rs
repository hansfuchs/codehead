use super::{Instant, View, load};

impl View {
    /// Read the list: the 1st time, and after a change (NOW: a bus topic named this list).
    pub(crate) fn refresh(&mut self, now: bool) {
        refresh_view(self, now);
    }
}

fn refresh_view(view: &mut View, now: bool) {
    view.source_reads.again |= now;

    if !view.source_reads.again && view.source_reads.loaded.is_some() {
        return;
    }

    view.source_reads.loaded = Some(Instant::now());

    // tests: at once
    if cfg!(test) {
        view.source_reads.again = false;
        let files = load(&view.id, &view.dir);
        let _ = view.apply(files);

        return;
    }

    // a read under way: 1 more after it (the change came after its start)
    if !view.source_reads.reads.waiting(&()) {
        view.source_reads.again = false;
        view.source_reads.reads.forget(&());
        let (id, dir) = (view.id.clone(), view.dir.clone());

        view.source_reads.reads.request((), move || {
            let files = load(&id, &dir);

            crate::infra::bus::wake();
            files
        });
    }
}
