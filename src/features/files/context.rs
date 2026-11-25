use super::{Act, Entry, Rect, Row, View};

impl View {
    /// The context menu (ctrl+.): the actions of the file under the cursor.
    pub(crate) fn ctx_items(&mut self) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
        ctx_items_view(self)
    }
    /// Where the context menu opens: above / below the cursor's row of the list (the preview
    /// stays in view).
    pub(crate) fn ctx_place(&self) -> crate::ui::context_menu::Place {
        let i = self.visible.get(self.at).map(|(i, _)| *i);
        let y = self
            .drawn
            .iter()
            .find(|(_, row)| matches!(row, Row::File(file) if Some(*file) == i))
            .map_or(self.list.y, |(y, _)| *y);

        crate::ui::context_menu::Place::At(Rect {
            y,
            height: 1,
            ..self.list
        })
    }
    /// A context menu command with 0 key of its own.
    pub(crate) fn ctx_cmd(&mut self, cmd: &str) -> Act {
        match cmd {
            "yazi" => Act::Yazi(self.cur().map_or_else(|| self.dir.clone(), |entry| entry.path.clone())),
            "copy path" => match self.cur() {
                Some(entry) => copy_path(entry),
                None => Act::None,
            },
            _ => Act::None,
        }
    }
}

fn copy_path(entry: &Entry) -> Act {
    let path = entry.path.display().to_string();
    let ok = crate::infra::clip::copy(&path);

    Act::Toast(if ok {
        format!("copied {path}")
    } else {
        "0 clipboard tool".into()
    })
}

fn ctx_items_view(view: &mut View) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
    use crate::ui::context_menu::{Act as ContextAction, Item, key};

    let title = view.cur().map_or_else(
        || "files".to_string(),
        |entry| {
            entry
                .path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        },
    );

    let mut items = Vec::new();

    if view.cur().is_some() {
        items.push(Item::new("open in the IDE", "e", key('e')));
        items.push(Item::new("copy the path", "", ContextAction::Cmd("copy path")));
    }

    items.push(Item::new(
        "yazi on the file (its pick: into the prompt)",
        "",
        ContextAction::Cmd("yazi"),
    ));
    items.push(Item::new("share: commit + push the home repo files", "S S", key('S')));
    items.push(Item::new("read the list again", "r", key('r')));

    (title, items)
}
