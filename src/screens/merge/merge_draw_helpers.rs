use super::{
    Block, Borders, Edit, Frame, Line, Menu, Merge, Modifier, Paragraph, PrRef, Rect, SLEEP, Span, Stop, Style, Sub,
    UnicodeWidthStr, shift, theme,
};

impl Merge {
    pub(super) fn draw_now(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        draw_merge(self, frame, area, focused);
    }

    /// The open dropdown (status, own comment, labels) under its stop.
    pub(super) fn draw_menu(&mut self, frame: &mut Frame, focused: bool) {
        draw_dropdown(self, frame, focused);
    }
}

fn draw_merge(merge: &mut Merge, frame: &mut Frame, area: Rect, focused: bool) {
    merge.layout.area = area;
    merge.layout.hits.clear();
    merge.layout.tab_hits.clear();
    merge.layout.sub_hits.clear();
    merge.layout.menu_hits.clear();
    merge.layout.edit_rect = Rect::default();

    if area.height < 4 || area.width < 20 {
        return;
    }

    if merge.prs.is_empty() {
        let msg = if merge.assoc_busy || (merge.sources.is_some() && merge.assoc_at.is_none()) {
            "  reading the pull requests of this thread…".to_string()
        } else {
            association_message(merge)
        };

        frame.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
        return;
    }

    let focus = merge.tab_ref().and_then(|thread| thread.focus);
    let tabs_focused = focused && focus == Some(Stop::Tabs);
    let subs_focused = focused && focus == Some(Stop::Subs);

    // row 0: the PR tabs; row 1: the views
    draw_pull_request_tabs(merge, frame, area, tabs_focused);
    let sub = merge.tab_ref().map(|thread_value| thread_value.sub).unwrap_or_default();

    draw_sub_tabs(merge, frame, area, subs_focused, sub);
    let body = Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3),
        ..area
    };

    merge.layout.body_area = body;

    if sub == Sub::Changes {
        draw_changes(merge, frame, body);

        return;
    }

    draw_overview(merge, frame, focused, focus, body);
}

fn draw_changes(merge: &mut Merge, frame: &mut Frame<'_>, body: Rect) {
    if let Some(character) = merge.changes_view() {
        character.draw(frame, body);

        if let Some(count) = character.take_note() {
            merge.note = Some(count);
        }
    }
}

fn association_message(merge: &Merge) -> String {
    if let Some(error_value) = &merge.assoc_error {
        format!("  pull requests: {error_value}")
    } else {
        "  0 pull requests: 0 PR of the thread's branches, 0 PR link in the thread. r: read again".to_string()
    }
}

#[path = "merge_tabs_draw.rs"]
mod tabs;
use tabs::{draw_pull_request_tabs, draw_sub_tabs};
#[path = "merge_dropdown.rs"]
mod dropdown;
use dropdown::draw_dropdown;
#[path = "merge_overview_draw.rs"]
mod overview;
use overview::draw_overview;
