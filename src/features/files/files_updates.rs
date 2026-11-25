use super::{Instant, View, load};

impl View {
    /// Read the list: the 1st time, and after a change (NOW: a bus topic named this list).
    pub(crate) fn refresh(&mut self, now: bool) {
        refresh_view(self, now);
    }
}

fn refresh_view(state: &mut View, now: bool) {
    state.source_reads.again |= now;

    if !state.source_reads.again && state.source_reads.loaded.is_some() {
        return;
    }

    state.source_reads.loaded = Some(Instant::now());

    // tests: at once
    if cfg!(test) {
        state.source_reads.again = false;
        let files = load(&state.id, &state.dir);
        let _ = state.apply(files);

        return;
    }

    // a read under way: 1 more after it (the change came after its start)
    if !state.source_reads.reads.waiting(&()) {
        state.source_reads.again = false;
        state.source_reads.reads.forget(&());
        let (id, dir) = (state.id.clone(), state.dir.clone());

        state.source_reads.reads.request((), move || {
            let value = load(&id, &dir);

            crate::infra::bus::wake();
            value
        });
    }
}
