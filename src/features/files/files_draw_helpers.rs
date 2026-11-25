use super::{Frame, IMAGE_REST, ImageKey, Instant, Line, Paragraph, Path, PathBuf, Rect, Span, View, mtime, theme};

impl View {
    /// The image preview of P in BODY (cols × rows): drawn (true), or a line while it encodes or
    /// why 0 image. False: 0 image preview for P (a text file, 0 graphics terminal, 0 room).
    pub(super) fn draw_image(&mut self, frame: &mut Frame, path: &Path, head: Rect, body: Rect) -> bool {
        draw_image_view(self, frame, path, head, body)
    }
}

fn request_image(
    context: ImageViewport<'_, '_>,
    file: &Path,
    kind: crate::ui::graphics::Kind,
    picker: crate::ui::graphics::Picker,
    key: (PathBuf, u64, u16, u16),
    line: impl Fn(&str) -> Line<'static>,
    view: &mut View,
) {
    let ImageViewport { frame, head, body } = context;

    let rested = view
        .image_cache
        .image_want
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed() >= IMAGE_REST);

    if rested && !view.image_cache.image_reads.waiting(&key) {
        let (file, cols, rows) = (file.to_path_buf(), body.width, body.height);

        view.image_cache.image_reads.request(key, move || {
            crate::ui::graphics::build(&file, kind, &picker, cols, rows)
        });
    }

    frame.render_widget(Paragraph::new(line("reading…")), head);
}

fn draw_cached_image(
    frame: &mut Frame<'_>,
    head: Rect,
    body: Rect,
    key: &(PathBuf, u64, u16, u16),
    line: impl Fn(&str) -> Line<'static>,
    view: &mut View,
) -> Option<bool> {
    // in use: the newest of the kept encodes (LRU)
    if view.image_cache.image_order.last() != Some(key) {
        view.image_cache.image_order.retain(|cached| *cached != *key);
        view.image_cache.image_order.push(key.clone());
    }

    let Some(Ok(pre)) = view.image_cache.images.get_mut(key) else {
        return Some(true);
    };

    // kitty: the image data 1x straight to the terminal, the cells: placeholders
    if let Some(transmit) = pre.transmit.take() {
        crate::ui::graphics::send(&transmit);
    }

    frame.render_widget(Paragraph::new(line(&pre.info)), head);
    frame.render_widget(ratatui_image::Image::new(&pre.protocol), body);

    None
}

struct ImageViewport<'frame, 'buffer> {
    frame: &'frame mut Frame<'buffer>,
    head: Rect,
    body: Rect,
}

fn draw_image_view(view: &mut View, frame: &mut Frame, path: &Path, head: Rect, body: Rect) -> bool {
    let kind = crate::ui::graphics::kind(path);

    let picker = if kind.is_some() && body.width > 0 && body.height > 0 {
        (view.image_cache.picker)()
    } else {
        None
    };

    let (Some(kind), Some(picker)) = (kind, picker) else {
        // 0 image wanted: 0 wake-ups for one
        view.image_cache.image_want = None;
        view.image_cache.image_kick = false;
        return false;
    };

    let key: ImageKey = (path.to_path_buf(), mtime(path), body.width, body.height);
    let hints = "e: IDE · ctrl+.: yazi, copy path · ctrl+f: filter";
    let shown = view.shown(path);

    let line = |info: &str| {
        Line::from(vec![
            Span::styled(format!(" {shown} "), theme::bold()),
            Span::styled(format!("{info} · {hints}"), theme::muted()),
        ])
    };

    // a new file or area: wait for the cursor (or the resize) to rest
    if view
        .image_cache
        .image_want
        .as_ref()
        .is_none_or(|(wanted, _)| *wanted != key)
    {
        view.image_cache.image_want = Some((key.clone(), Instant::now()));
        view.image_cache.image_kick = true;
    }

    match view.image_cache.images.get_mut(&key) {
        Some(Ok(_)) => {
            if let Some(drawn) = draw_cached_image(frame, head, body, &key, line, view) {
                return drawn;
            }
        }
        Some(Err(why)) => frame.render_widget(Paragraph::new(line(why)), head),
        None => {
            request_image(ImageViewport { frame, head, body }, path, kind, picker, key, line, view);
        }
    }

    true
}
