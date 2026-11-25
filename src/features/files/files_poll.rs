use super::{IMAGE_REST, MAX_IMAGES, View};

impl View {
    pub(crate) fn poll(&mut self) -> bool {
        poll_view(self)
    }
}

fn poll_branch(
    changed: &mut bool,
    key: (std::path::PathBuf, u64, u16, u16),
    pre: Result<crate::ui::graphics::Preview, String>,
    view_state: &mut View,
) {
    if view_state
        .image_cache
        .image_want
        .as_ref()
        .is_some_and(|(key_value, _)| *key_value == key)
    {
        view_state.image_cache.images.insert(key.clone(), pre);
        view_state.image_cache.image_order.retain(|key_2| *key_2 != key);
        view_state.image_cache.image_order.push(key);
        *changed = true;
    } else {
        // stale: 0 kept, 0 sent; back on it later: a new read at once
        view_state.image_cache.image_reads.forget(&key);
    }
}

fn poll_branch_state(
    changed: &mut bool,
    renders: Vec<((std::path::PathBuf, u16, u64), Vec<ratatui::prelude::Line<'static>>)>,
    view_state: &mut View,
) {
    if view_state.text_preview.cache.len() > 64 {
        view_state.text_preview.cache.clear();
    }

    view_state.text_preview.cache.extend(renders);
    view_state.text_preview.preview_version += 1;
    *changed = true;
}

fn poll_view(state: &mut View) -> bool {
    let reads = state.source_reads.reads.poll();
    let mut changed = false;

    for ((), files) in reads {
        changed |= state.apply(files);
    }

    if changed {
        state.source_reads.list_version += 1;
    }

    let renders = state.text_preview.renders.poll();

    if !renders.is_empty() {
        poll_branch_state(&mut changed, renders, state);
    }

    // encodes in: only the wanted one (fast j k, a resize drag: the others are stale)
    for (key, pre) in state.image_cache.image_reads.poll() {
        poll_branch(&mut changed, key, pre, state);
    }

    // the last 4 encodes stay (back and forth between files: 0 new encode); the oldest out
    while state.image_cache.image_order.len() > MAX_IMAGES {
        let old = state.image_cache.image_order.remove(0);

        state.drop_image(&old);
    }

    // the cursor rested on an image: 1 draw, which requests its encode (1x: 0 redraw per tick
    // after yazi, a pane change, `previews = off`)
    if state.image_cache.image_kick
        && state
            .image_cache
            .image_want
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= IMAGE_REST)
    {
        state.image_cache.image_kick = false;
        changed |= state.image_cache.image_want.as_ref().is_some_and(|(key_3, _)| {
            !state.image_cache.images.contains_key(key_3) && !state.image_cache.image_reads.waiting(key_3)
        });
    }

    changed
}
