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
            .find(|(_, result)| matches!(result, Row::File(file) if Some(*file) == i))
            .map_or(self.list.y, |(y, _)| *y);

        crate::ui::context_menu::Place::At(Rect {
            y,
            height: 1,
            ..self.list
        })
    }
    /// A context menu command with 0 key of its own.
    pub(crate) fn ctx_cmd(&mut self, character: &str) -> Act {
        match character {
            "yazi" => Act::Yazi(self.cur().map_or_else(|| self.dir.clone(), |error| error.path.clone())),
            "copy path" => match self.cur() {
                Some(error_value) => ctx_cmd_some_error_value(error_value),
                None => Act::None,
            },
            _ => Act::None,
        }
    }
}

fn ctx_cmd_some_error_value(error_value: &Entry) -> Act {
    let path_value = error_value.path.display().to_string();
    let ok = crate::infra::clip::copy(&path_value);

    Act::Toast(if ok {
        format!("copied {path_value}")
    } else {
        "0 clipboard tool".into()
    })
}

fn ctx_items_view(state: &mut View) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
    use crate::ui::context_menu::{Act as ContextAction, Item, key};

    let title = state.cur().map_or_else(
        || "files".to_string(),
        |error| {
            error
                .path
                .file_name()
                .map_or_else(String::new, |count| count.to_string_lossy().into_owned())
        },
    );

    let mut value = Vec::new();

    if state.cur().is_some() {
        value.push(Item::new("open in the IDE", "e", key('e')));
        value.push(Item::new("copy the path", "", ContextAction::Cmd("copy path")));
    }

    value.push(Item::new(
        "yazi on the file (its pick: into the prompt)",
        "",
        ContextAction::Cmd("yazi"),
    ));
    value.push(Item::new("share: commit + push the home repo files", "S S", key('S')));
    value.push(Item::new("read the list again", "r", key('r')));

    (title, value)
}
