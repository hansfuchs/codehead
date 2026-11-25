use super::{Entry, ImageKey, Line, Path, Preview, Source, View, mtime};

impl View {
    /// The encode of KEY out (the cache, the terminal's kitty store); a new request runs at once.
    pub(super) fn drop_image(&mut self, key: &ImageKey) {
        if let Some(Ok(Preview {
            kitty: Some(id),
            transmit: None,
            ..
        })) = self.image_cache.images.remove(key)
        {
            crate::ui::graphics::delete_kitty(id);
        }

        self.image_cache.image_order.retain(|cached| cached != key);
        self.image_cache.image_reads.forget(key);
    }
    /// The encoded images out: the next draw encodes (and sends) them again. After another
    /// program had the terminal (yazi, an editor): it may have deleted them.
    pub(crate) fn forget_images(&mut self) {
        for key in std::mem::take(&mut self.image_cache.image_order) {
            self.drop_image(&key);
        }

        self.image_cache.images.clear();
        self.image_cache.image_reads.invalidate();
    }
    /// FILES (a read of the list) + the named files: the list; the cursor stays on its file.
    /// True: the list changed.
    pub(super) fn apply(&mut self, files: Vec<Entry>) -> bool {
        apply_view(self, files)
    }

    /// Pick PATH (a file a message named): in the list, else added.
    pub(crate) fn show(&mut self, path: &Path) {
        show_view(self, path);
    }
    /// PATH as shown: relative to the work dir, else `~/…`.
    pub(super) fn shown(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.dir) {
            return relative.display().to_string();
        }

        let home = crate::infra::config::home();

        match path.strip_prefix(&home) {
            Ok(relative) => format!("~/{}", relative.display()),
            Err(_) => path.display().to_string(),
        }
    }
    /// The preview of P at WIDTH: from the cache, else None and a render off the UI thread
    /// (tests: at once).
    pub(super) fn preview_lines(&mut self, path: &Path, width: u16) -> Option<Vec<Line<'static>>> {
        preview_lines_view(self, path, width)
    }
}

fn keep_named(files: &mut Vec<Entry>, named: &std::path::PathBuf) {
    if !files.iter().any(|entry| entry.path == *named) {
        files.insert(
            0,
            Entry {
                path: named.clone(),
                source: Source::Named,
                at: mtime(named),
            },
        );
    }
}
/// The preview rows of P at WIDTH: markdown, else highlighted code.
pub(super) fn render_preview(path: &Path, width: u16) -> Vec<Line<'static>> {
    let text = std::fs::read(path).map_or_else(
        |error| format!("{error}"),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    );
    let columns = usize::from(width);

    if path.extension().is_some_and(|extension| extension == "md") {
        crate::ui::markdown::markdown(&text, columns, &[])
            .into_iter()
            .map(|row| row.line)
            .collect()
    } else {
        let lang = path
            .extension()
            .map_or_else(|| "text".into(), |extension| extension.to_string_lossy().into_owned());

        crate::ui::markdown::code_rows(&text, &lang, columns, &[], true, None)
            .into_iter()
            .skip(1)
            .map(|row| row.line)
            .collect()
    }
}

fn show_view(view: &mut View, path: &Path) {
    if !view.named.iter().any(|named| named == path) && !view.files.iter().any(|entry| entry.path == path) {
        view.named.push(path.to_path_buf());
    }

    view.filter_input.filter.set_text("");
    view.filter_input.filtering = false;

    // the named file at once; the rest of the list from the read
    if !view.files.iter().any(|entry| entry.path == path) {
        view.files.insert(
            0,
            Entry {
                path: path.to_path_buf(),
                source: Source::Named,
                at: mtime(path),
            },
        );
    }

    view.refresh(true);
    view.rebuild();
    view.at = view
        .visible
        .iter()
        .position(|(i, _)| view.files[*i].path == path)
        .unwrap_or(0);
    view.text_preview.scroll = 0;
}

fn restore_cursor(path: &Path, view: &mut View) {
    view.at = view
        .visible
        .iter()
        .position(|(i, _)| view.files[*i].path == *path)
        .unwrap_or(view.at);
}

fn keep_cursor_on(path: &Path, view: &mut View) {
    restore_cursor(path, view);
}

fn apply_view(view: &mut View, mut files: Vec<Entry>) -> bool {
    // named files (clicked links) stay
    for named in view.named.iter().rev() {
        keep_named(&mut files, named);
    }

    if files != view.files
        || view.visible.len() != view.files.len()
        || !view.filter_input.filter.text().trim().is_empty()
    {
        let cur = view.cur().map(|entry| entry.path.clone());
        let changed = files != view.files;

        view.files = files;
        view.rebuild();

        if let Some(path) = cur {
            keep_cursor_on(&path, view);
        }

        return changed;
    }

    false
}

fn preview_lines_view(view: &mut View, file: &Path, width: u16) -> Option<Vec<Line<'static>>> {
    let key = (file.to_path_buf(), width, mtime(file));

    if let Some(lines) = view.text_preview.cache.get(&key) {
        return Some(lines.clone());
    }

    if cfg!(test) {
        let lines = render_preview(file, width);

        view.text_preview.cache.insert(key, lines.clone());
        return Some(lines);
    }

    if !view.text_preview.renders.waiting(&key) {
        let owned = file.to_path_buf();

        view.text_preview
            .renders
            .request(key, move || render_preview(&owned, width));
    }

    None
}
