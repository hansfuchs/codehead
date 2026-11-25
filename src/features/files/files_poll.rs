use super::{IMAGE_REST, MAX_IMAGES, View};

impl View {
    /// A finished read of the list: in. True: the list changed.
    pub(crate) fn poll(&mut self) -> bool {
        poll_view(self)
    }
}

fn keep_wanted_image(
    changed: &mut bool,
    key: (std::path::PathBuf, u64, u16, u16),
    pre: Result<crate::ui::graphics::Preview, String>,
    view: &mut View,
) {
    if view
        .image_cache
        .image_want
        .as_ref()
        .is_some_and(|(wanted, _)| *wanted == key)
    {
        view.image_cache.images.insert(key.clone(), pre);
        view.image_cache.image_order.retain(|cached| *cached != key);
        view.image_cache.image_order.push(key);
        *changed = true;
    } else {
        // stale: 0 kept, 0 sent; back on it later: a new read at once
        view.image_cache.image_reads.forget(&key);
    }
}

fn store_renders(
    changed: &mut bool,
    renders: Vec<((std::path::PathBuf, u16, u64), Vec<ratatui::prelude::Line<'static>>)>,
    view: &mut View,
) {
    if view.text_preview.cache.len() > 64 {
        view.text_preview.cache.clear();
    }

    view.text_preview.cache.extend(renders);
    view.text_preview.preview_version += 1;
    *changed = true;
}

fn poll_view(view: &mut View) -> bool {
    let reads = view.source_reads.reads.poll();
    let mut changed = false;

    for ((), files) in reads {
        changed |= view.apply(files);
    }

    if changed {
        view.source_reads.list_version += 1;
    }

    let renders = view.text_preview.renders.poll();

    if !renders.is_empty() {
        store_renders(&mut changed, renders, view);
    }

    // encodes in: only the wanted one (fast j k, a resize drag: the others are stale)
    for (key, pre) in view.image_cache.image_reads.poll() {
        keep_wanted_image(&mut changed, key, pre, view);
    }

    // the last 4 encodes stay (back and forth between files: 0 new encode); the oldest out
    while view.image_cache.image_order.len() > MAX_IMAGES {
        let old = view.image_cache.image_order.remove(0);

        view.drop_image(&old);
    }

    // the cursor rested on an image: 1 draw, which requests its encode (1x: 0 redraw per tick
    // after yazi, a pane change, `previews = off`)
    if view.image_cache.image_kick
        && view
            .image_cache
            .image_want
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= IMAGE_REST)
    {
        view.image_cache.image_kick = false;
        changed |= view.image_cache.image_want.as_ref().is_some_and(|(wanted, _)| {
            !view.image_cache.images.contains_key(wanted) && !view.image_cache.image_reads.waiting(wanted)
        });
    }

    changed
}
