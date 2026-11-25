use super::{Frame, IMAGE_REST, ImageKey, Instant, Line, Paragraph, Path, PathBuf, Rect, Span, View, mtime, theme};

impl View {
    /// The image preview of P in BODY (cols × rows): drawn (true), or a line while it encodes or
    /// why 0 image. False: 0 image preview for P (a text file, 0 graphics terminal, 0 room).
    pub(super) fn draw_image(&mut self, frame: &mut Frame, path_value: &Path, head: Rect, body: Rect) -> bool {
        draw_image_view(self, frame, path_value, head, body)
    }
}

fn draw_image_none(
    context: ImageViewport<'_, '_>,
    path_value: &Path,
    kind: crate::ui::graphics::Kind,
    picker: crate::ui::graphics::Picker,
    key: (PathBuf, u64, u16, u16),
    line: impl Fn(&str) -> Line<'static>,
    view_state: &mut View,
) {
    let ImageViewport { frame, head, body } = context;

    let rested = view_state
        .image_cache
        .image_want
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed() >= IMAGE_REST);

    if rested && !view_state.image_cache.image_reads.waiting(&key) {
        let (path, cols, rows) = (path_value.to_path_buf(), body.width, body.height);

        view_state.image_cache.image_reads.request(key, move || {
            crate::ui::graphics::build(&path, kind, &picker, cols, rows)
        });
    }

    frame.render_widget(Paragraph::new(line("reading…")), head);
}

fn draw_image_some_ok(
    frame: &mut Frame<'_>,
    head: Rect,
    body: Rect,
    key: &(PathBuf, u64, u16, u16),
    line: impl Fn(&str) -> Line<'static>,
    view_state: &mut View,
) -> Option<bool> {
    if view_state.image_cache.image_order.last() != Some(key) {
        view_state.image_cache.image_order.retain(|key_2| *key_2 != *key);
        view_state.image_cache.image_order.push(key.clone());
    }

    let Some(Ok(pre)) = view_state.image_cache.images.get_mut(key) else {
        return Some(true);
    };

    if let Some(thread) = pre.transmit.take() {
        crate::ui::graphics::send(&thread);
    }

    frame.render_widget(Paragraph::new(line(&pre.info)), head);
    frame.render_widget(ratatui_image::Image::new(&pre.protocol), body);

    // in use: the newest of the kept encodes (LRU)

    // kitty: the image data 1x straight to the terminal, the cells: placeholders

    None
}

struct ImageViewport<'reference_0, 'reference_1> {
    frame: &'reference_0 mut Frame<'reference_1>,
    head: Rect,
    body: Rect,
}

fn draw_image_view(state: &mut View, frame: &mut Frame, path_value: &Path, head: Rect, body: Rect) -> bool {
    let kind = crate::ui::graphics::kind(path_value);

    let picker = if kind.is_some() && body.width > 0 && body.height > 0 {
        (state.image_cache.picker)()
    } else {
        None
    };

    let (Some(kind), Some(picker)) = (kind, picker) else {
        // 0 image wanted: 0 wake-ups for one
        state.image_cache.image_want = None;
        state.image_cache.image_kick = false;
        return false;
    };

    let key: ImageKey = (path_value.to_path_buf(), mtime(path_value), body.width, body.height);
    let hints = "e: IDE · ctrl+.: yazi, copy path · ctrl+f: filter";
    let shown = state.shown(path_value);

    let line = |info: &str| {
        Line::from(vec![
            Span::styled(format!(" {shown} "), theme::bold()),
            Span::styled(format!("{info} · {hints}"), theme::muted()),
        ])
    };

    // a new file or area: wait for the cursor (or the resize) to rest
    if state
        .image_cache
        .image_want
        .as_ref()
        .is_none_or(|(key_value, _)| *key_value != key)
    {
        state.image_cache.image_want = Some((key.clone(), Instant::now()));
        state.image_cache.image_kick = true;
    }

    match state.image_cache.images.get_mut(&key) {
        Some(Ok(_)) => {
            if let Some(value) = draw_image_some_ok(frame, head, body, &key, line, state) {
                return value;
            }
        }
        Some(Err(why)) => frame.render_widget(Paragraph::new(line(why)), head),
        None => {
            draw_image_none(
                ImageViewport { frame, head, body },
                path_value,
                kind,
                picker,
                key,
                line,
                state,
            );
        }
    }

    true
}
