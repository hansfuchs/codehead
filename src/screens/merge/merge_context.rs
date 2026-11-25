use super::{Act, KeyCode, KeyEvent, KeyModifiers, Merge, Rect, Stop, Sub, Verdict, check_name};
use std::fmt::Write;

impl Merge {
    /// The context of the PR in view for a message to the agents.
    pub(crate) fn context(&self) -> Option<String> {
        let pr = self.tab_ref()?.pr.as_ref()?;
        let (ready, _) = pr.ready().text();
        let mut text = format!(
            "[codehead: context from the merge tab]\nPull request {} ({}): “{}” · {} · {} · `{}` → `{}`",
            pr.url,
            pr.pr.label(),
            pr.title,
            pr.state.label(),
            ready,
            pr.head,
            pr.base
        );

        if let Some(dir) = self.dirs.get(&pr.pr.repo) {
            let _ = write!(text, "\nCheckout: {}", dir.display());
        }

        let failed: Vec<String> = pr
            .checks
            .iter()
            .filter(|check| check.verdict == Verdict::Fail)
            .map(|check| format!("- {} ({})", check_name(check), check.url))
            .collect();

        if !failed.is_empty() {
            let _ = write!(text, "\nFailed checks:\n{}", failed.join("\n"));
        }

        let open: Vec<String> = pr
            .threads
            .iter()
            .filter(|thread| thread.path.is_some() && !thread.resolved)
            .map(|thread| {
                format!(
                    "- {}:{} {}: {}",
                    thread.path.as_deref().unwrap_or_default(),
                    thread.line.unwrap_or(0),
                    thread.root.author,
                    thread.root.body.lines().next().unwrap_or_default()
                )
            })
            .collect();

        if !open.is_empty() {
            let _ = write!(text, "\nOpen review threads:\n{}", open.join("\n"));
        }

        Some(text)
    }

    /// Where the context menu opens: the PR diff: as in the changes view; the overview: above /
    /// below the stop in focus.
    pub(crate) fn ctx_place(&mut self, area: Rect) -> crate::ui::context_menu::Place {
        if self.tab_ref().is_some_and(|tab| tab.sub == Sub::Changes)
            && let Some(view) = self.changes_view()
        {
            return view.ctx_place();
        }

        let cur = self.tab_ref().and_then(|tab| tab.focus);
        let stop_area = self
            .layout
            .hits
            .iter()
            .find(|(_, stop)| Some(*stop) == cur)
            .map_or(Rect { height: 1, ..area }, |(rect, _)| *rect);

        crate::ui::context_menu::Place::At(Rect { height: 1, ..stop_area })
    }

    /// A context menu command of the PR diff (0 key of its own).
    pub(crate) fn ctx_cmd(&mut self, cmd: &str) -> Act {
        match self.changes_view().map(|view| view.ctx_cmd(cmd)) {
            Some(crate::features::changes::Act::Toast(toast)) => Act::Toast(toast),
            _ => Act::None,
        }
    }

    /// The context menu (ctrl+.): the actions of the stop in focus (the diff: of its row).
    pub(crate) fn ctx_items(&mut self) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
        use crate::ui::context_menu::{Act as ContextAction, Item, key};

        if self.prs.is_empty() {
            return ("merge".into(), vec![Item::new("read again", "r", key('r'))]);
        }

        let diff = self.tab_ref().is_some_and(|tab| tab.sub == Sub::Changes);

        let (title, mut items) = if diff {
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

        items.extend([
            Item::new("previous PR", "[", key('[')),
            Item::new("next PR", "]", key(']')),
            Item::new(
                if diff { "overview" } else { "diff" },
                if diff { "<" } else { ">" },
                key(if diff { '<' } else { '>' }),
            ),
            Item::new("read the PR again", "r", key('r')),
        ]);

        (title, items)
    }

    /// The changes view of the tab in view (PR mode), for the host's hints.
    pub(crate) fn hints(&self) -> String {
        overview_hints(self)
    }
}

fn overview_hints(merge: &Merge) -> String {
    let Some(tab) = merge.tab_ref() else {
        return "r: read again · tab: next ".into();
    };

    if tab.sub == Sub::Changes {
        if matches!(tab.focus, Some(Stop::Tabs | Stop::Subs)) {
            return "←→ PR / view · ↑↓ row · ↓ ⏎: the diff · tab: next ".into();
        }

        return tab
            .changes
            .as_ref()
            .map(|view| format!("{}· ↑ on top: views ", view.hints()))
            .unwrap_or_default();
    }

    if tab.input.edit.is_some() {
        return "ctrl+⏎ save · esc: close (edited: esc esc) ".into();
    }

    if tab.input.menu.is_some() {
        return "↑↓ pick · ⏎ take · esc close ".into();
    }

    "j k ↑↓ move · ←→ along · ⏎ act · o GitHub · [ ] PR · < > view · r read · tab next · ctrl+. actions ".into()
}
