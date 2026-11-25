use super::{Frame, Rect, View};

impl View {
    /// PR mode: the commit picker's label (0: every commit).
    pub(super) fn pick_label(&self, index: usize) -> (String, String) {
        pick_label_view(self, index)
    }

    /// PR mode: the picked commit as the picker's index.
    pub(super) fn pick_index(&self) -> usize {
        let Some(src) = &self.pr_src else { return 0 };

        src.commit
            .as_ref()
            .and_then(|oid| src.commits.iter().rev().position(|commit| &commit.oid == oid))
            .map_or(0, |position| position + 1)
    }

    /// PR mode: pick INDEX of the picker (0: every commit).
    pub(super) fn pick(&mut self, index: usize) {
        let oid = if index == 0 {
            None
        } else {
            self.pr_src
                .as_ref()
                .and_then(|source| source.commits.iter().rev().nth(index - 1))
                .map(|commit| commit.oid.clone())
        };

        self.scm.pick = None;
        self.pick_commit(oid);
    }

    /// PR mode: the open commit dropdown under the picker row.
    pub(super) fn draw_pick(&mut self, frame: &mut Frame, area: Rect) {
        draw_pick_view(self, frame, area);
    }
}

fn pick_label_view(view: &View, index: usize) -> (String, String) {
    let Some(src) = &view.pr_src else {
        return (String::new(), String::new());
    };

    if index == 0 {
        return (
            "all commits".into(),
            format!(
                "{} · vs {}",
                src.commits.len(),
                if src.base.is_empty() { "base" } else { &src.base }
            ),
        );
    }

    match src.commits.iter().rev().nth(index - 1) {
        Some(commit) => (
            format!("{} {}", commit.short, commit.headline),
            crate::infra::gh::stamp(commit.date),
        ),
        None => (String::new(), String::new()),
    }
}

fn draw_pick_view(view: &mut View, frame: &mut Frame, area: Rect) {
    view.scm.pick_rows.clear();
    let Some(cur) = view.scm.pick else { return };
    let count = view.pr_src.as_ref().map_or(0, |source| source.commits.len()) + 1;

    let rows: Vec<crate::ui::dropdown::Row> = (0..count)
        .map(|index| {
            let (text, detail) = view.pick_label(index);

            crate::ui::dropdown::Row {
                text,
                detail,
                now: index == view.pick_index(),
                marks: Vec::new(),
            }
        })
        .collect();

    let anchor = Rect {
        y: area.y,
        height: 1,
        ..area
    };
    let width = area.width.max(50);
    let bounds = Rect { width, ..frame.area() };

    let Some(rect) = crate::ui::dropdown::place(anchor, width, count, bounds, false) else {
        return;
    };

    let (_, hits) = crate::ui::dropdown::list(
        frame,
        rect,
        crate::ui::dropdown::List {
            label: "commits",
            note: "enter: show · esc",
            rows: &rows,
            cursor: cur,
            offset: 0,
            empty: "0 commits",
        },
    );

    view.scm.pick_rows = hits;
}
