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

        self.image_cache.image_order.retain(|key_value| key_value != key);
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
        if let Ok(result) = path.strip_prefix(&self.dir) {
            return result.display().to_string();
        }

        let home = crate::infra::config::home();

        match path.strip_prefix(&home) {
            Ok(result_value) => format!("~/{}", result_value.display()),
            Err(_) => path.display().to_string(),
        }
    }
    /// The preview of P at WIDTH: from the cache, else None and a render off the UI thread
    /// (tests: at once).
    pub(super) fn preview_lines(&mut self, path_value: &Path, width: u16) -> Option<Vec<Line<'static>>> {
        preview_lines_view(self, path_value, width)
    }
}

fn apply_branch(files: &mut Vec<Entry>, path_value: &std::path::PathBuf) {
    if !files.iter().any(|error| error.path == *path_value) {
        files.insert(
            0,
            Entry {
                path: path_value.clone(),
                source: Source::Named,
                at: mtime(path_value),
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
    let width_value = usize::from(width);

    if path.extension().is_some_and(|x| x == "md") {
        crate::ui::markdown::markdown(&text, width_value, &[])
            .into_iter()
            .map(|result| result.line)
            .collect()
    } else {
        let lang = path
            .extension()
            .map_or_else(|| "text".into(), |x| x.to_string_lossy().into_owned());

        crate::ui::markdown::code_rows(&text, &lang, width_value, &[], true, None)
            .into_iter()
            .skip(1)
            .map(|result_value| result_value.line)
            .collect()
    }
}

fn show_view(state: &mut View, path: &Path) {
    if !state.named.iter().any(|path_value| path_value == path) && !state.files.iter().any(|error| error.path == path) {
        state.named.push(path.to_path_buf());
    }

    state.filter_input.filter.set_text("");
    state.filter_input.filtering = false;

    // the named file at once; the rest of the list from the read
    if !state.files.iter().any(|error_value| error_value.path == path) {
        state.files.insert(
            0,
            Entry {
                path: path.to_path_buf(),
                source: Source::Named,
                at: mtime(path),
            },
        );
    }

    state.refresh(true);
    state.rebuild();
    state.at = state
        .visible
        .iter()
        .position(|(i, _)| state.files[*i].path == path)
        .unwrap_or(0);
    state.text_preview.scroll = 0;
}

fn apply_body(path_2: &Path, view_state: &mut View) {
    view_state.at = view_state
        .visible
        .iter()
        .position(|(i, _)| view_state.files[*i].path == *path_2)
        .unwrap_or(view_state.at);
}

fn apply_body_state(path_2: &Path, view_state: &mut View) {
    apply_body(path_2, view_state);
}

fn apply_view(state: &mut View, mut files: Vec<Entry>) -> bool {
    // named files (clicked links) stay
    for path_value in state.named.iter().rev() {
        apply_branch(&mut files, path_value);
    }

    if files != state.files
        || state.visible.len() != state.files.len()
        || !state.filter_input.filter.text().trim().is_empty()
    {
        let cur = state.cur().map(|error_value| error_value.path.clone());
        let changed = files != state.files;

        state.files = files;
        state.rebuild();

        if let Some(path_2) = cur {
            apply_body_state(&path_2, state);
        }

        return changed;
    }

    false
}

fn preview_lines_view(state: &mut View, path_value: &Path, width: u16) -> Option<Vec<Line<'static>>> {
    let key = (path_value.to_path_buf(), width, mtime(path_value));

    if let Some(line) = state.text_preview.cache.get(&key) {
        return Some(line.clone());
    }

    if cfg!(test) {
        let lines = render_preview(path_value, width);

        state.text_preview.cache.insert(key, lines.clone());
        return Some(lines);
    }

    if !state.text_preview.renders.waiting(&key) {
        let path = path_value.to_path_buf();

        state
            .text_preview
            .renders
            .request(key, move || render_preview(&path, width));
    }

    None
}
