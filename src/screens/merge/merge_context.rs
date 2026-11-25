use super::{Act, KeyCode, KeyEvent, KeyModifiers, Merge, Rect, Stop, Sub, Verdict, check_name};
use std::fmt::Write;

impl Merge {
    /// The context of the PR in view for a message to the agents.
    pub(crate) fn context(&self) -> Option<String> {
        let path_value = self.tab_ref()?.pr.as_ref()?;
        let (title, _) = path_value.ready().text();
        let mut text_value = format!(
            "[codehead: context from the merge tab]\nPull request {} ({}): “{}” · {} · {} · `{}` → `{}`",
            path_value.url,
            path_value.pr.label(),
            path_value.title,
            path_value.state.label(),
            title,
            path_value.head,
            path_value.base
        );

        if let Some(directory) = self.dirs.get(&path_value.pr.repo) {
            let _ = write!(text_value, "\nCheckout: {}", directory.display());
        }

        let failed: Vec<String> = path_value
            .checks
            .iter()
            .filter(|character| character.verdict == Verdict::Fail)
            .map(|character_value| format!("- {} ({})", check_name(character_value), character_value.url))
            .collect();

        if !failed.is_empty() {
            let _ = write!(text_value, "\nFailed checks:\n{}", failed.join("\n"));
        }

        let open: Vec<String> = path_value
            .threads
            .iter()
            .filter(|thread| thread.path.is_some() && !thread.resolved)
            .map(|thread_value| {
                format!(
                    "- {}:{} {}: {}",
                    thread_value.path.as_deref().unwrap_or_default(),
                    thread_value.line.unwrap_or(0),
                    thread_value.root.author,
                    thread_value.root.body.lines().next().unwrap_or_default()
                )
            })
            .collect();

        if !open.is_empty() {
            let _ = write!(text_value, "\nOpen review threads:\n{}", open.join("\n"));
        }

        Some(text_value)
    }
    /// Where the context menu opens: the PR diff: as in the changes view; the overview: above /
    /// below the stop in focus.
    pub(crate) fn ctx_place(&mut self, area: Rect) -> crate::ui::context_menu::Place {
        if self.tab_ref().is_some_and(|changes| changes.sub == Sub::Changes)
            && let Some(character) = self.changes_view()
        {
            return character.ctx_place();
        }

        let cur = self.tab_ref().and_then(|thread| thread.focus);
        let area_value = self
            .layout
            .hits
            .iter()
            .find(|(_, text)| Some(*text) == cur)
            .map_or(Rect { height: 1, ..area }, |(result, _)| *result);

        crate::ui::context_menu::Place::At(Rect {
            height: 1,
            ..area_value
        })
    }
    /// A context menu command of the PR diff (0 key of its own).
    pub(crate) fn ctx_cmd(&mut self, character: &str) -> Act {
        match self.changes_view().map(|value| value.ctx_cmd(character)) {
            Some(crate::features::changes::Act::Toast(thread)) => Act::Toast(thread),
            _ => Act::None,
        }
    }
    /// The context menu (ctrl+.): the actions of the stop in focus (the diff: of its row).
    pub(crate) fn ctx_items(&mut self) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
        use crate::ui::context_menu::{Act as ContextAction, Item, key};

        if self.prs.is_empty() {
            return ("merge".into(), vec![Item::new("read again", "r", key('r'))]);
        }

        let diff = self
            .tab_ref()
            .is_some_and(|changes_value| changes_value.sub == Sub::Changes);

        let (title, mut key_value) = if diff {
            self.changes_view().map_or_else(
                || (String::new(), Vec::new()),
                crate::features::changes::View::ctx_items,
            )
        } else {
            (
                "pull request".to_string(),
                vec![
                    Item::new(
                        "act on the stop",
                        "enter",
                        ContextAction::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                    ),
                    Item::new("open on GitHub", "o", key('o')),
                ],
            )
        };

        key_value.extend([
            Item::new("previous PR", "[", key('[')),
            Item::new("next PR", "]", key(']')),
            Item::new(
                if diff { "overview" } else { "diff" },
                if diff { "<" } else { ">" },
                key(if diff { '<' } else { '>' }),
            ),
            Item::new("read the PR again", "r", key('r')),
        ]);

        (title, key_value)
    }
    /// The changes view of the tab in view (PR mode), for the host's hints.
    pub(crate) fn hints(&self) -> String {
        overview_hints(self)
    }
}

fn overview_hints(widget_state: &Merge) -> String {
    let Some(thread) = widget_state.tab_ref() else {
        return "r: read again · tab: next ".into();
    };

    if thread.sub == Sub::Changes {
        if matches!(thread.focus, Some(Stop::Tabs | Stop::Subs)) {
            return "←→ PR / view · ↑↓ row · ↓ ⏎: the diff · tab: next ".into();
        }

        return thread
            .changes
            .as_ref()
            .map(|character| format!("{}· ↑ on top: views ", character.hints()))
            .unwrap_or_default();
    }

    if thread.input.edit.is_some() {
        return "ctrl+⏎ save · esc: close (edited: esc esc) ".into();
    }

    if thread.input.menu.is_some() {
        return "↑↓ pick · ⏎ take · esc close ".into();
    }

    "j k ↑↓ move · ←→ along · ⏎ act · o GitHub · [ ] PR · < > view · r read · tab next · ctrl+. actions ".into()
}
