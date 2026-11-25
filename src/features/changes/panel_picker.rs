use super::{Frame, Rect, View};

impl View {
    /// PR mode: the commit picker's label (0: every commit).
    pub(super) fn pick_label(&self, key: usize) -> (String, String) {
        pick_label_view(self, key)
    }

    /// PR mode: the picked commit as the picker's index.
    pub(super) fn pick_index(&self) -> usize {
        let Some(src) = &self.pr_src else { return 0 };

        src.commit
            .as_ref()
            .and_then(|character| src.commits.iter().rev().position(|x| &x.oid == character))
            .map_or(0, |path| path + 1)
    }

    /// PR mode: pick K of the picker (0: every commit).
    pub(super) fn pick(&mut self, key: usize) {
        let oid = if key == 0 {
            None
        } else {
            self.pr_src
                .as_ref()
                .and_then(|text| text.commits.iter().rev().nth(key - 1))
                .map(|character| character.oid.clone())
        };

        self.scm.pick = None;
        self.pick_commit(oid);
    }

    /// PR mode: the open commit dropdown under the picker row.
    pub(super) fn draw_pick(&mut self, frame: &mut Frame, area: Rect) {
        draw_pick_view(self, frame, area);
    }
}

fn pick_label_view(view: &View, key: usize) -> (String, String) {
    let Some(src) = &view.pr_src else {
        return (String::new(), String::new());
    };

    if key == 0 {
        return (
            "all commits".into(),
            format!(
                "{} · vs {}",
                src.commits.len(),
                if src.base.is_empty() { "base" } else { &src.base }
            ),
        );
    }

    match src.commits.iter().rev().nth(key - 1) {
        Some(character) => (
            format!("{} {}", character.short, character.headline),
            crate::infra::gh::stamp(character.date),
        ),
        None => (String::new(), String::new()),
    }
}

fn draw_pick_view(view: &mut View, frame: &mut Frame, area: Rect) {
    view.scm.pick_rows.clear();
    let Some(cur) = view.scm.pick else { return };
    let count = view.pr_src.as_ref().map_or(0, |text_value| text_value.commits.len()) + 1;

    let rows: Vec<crate::ui::dropdown::Row> = (0..count)
        .map(|key| {
            let (text, detail) = view.pick_label(key);

            crate::ui::dropdown::Row {
                text,
                detail,
                now: key == view.pick_index(),
                marks: Vec::new(),
            }
        })
        .collect();

    let anchor = Rect {
        y: area.y,
        height: 1,
        ..area
    };
    let width_value = area.width.max(50);
    let bounds = Rect {
        width: width_value,
        ..frame.area()
    };

    let Some(result) = crate::ui::dropdown::place(anchor, width_value, count, bounds, false) else {
        return;
    };

    let (_, hits) = crate::ui::dropdown::list(
        frame,
        result,
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
