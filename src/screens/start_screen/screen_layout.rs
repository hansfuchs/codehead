use super::{App, Constraint, Layout, Rect, THIN, Tab};

impl App {
    pub(super) fn split(&self, area: Rect) -> (Rect, Rect, bool) {
        // agents: a card grid, more room than a list
        let cards = matches!(self.tab, Tab::Agents | Tab::New);

        if area.width < THIN {
            split_stacked(area, cards)
        } else {
            split_side_by_side(area, cards)
        }
    }
}

pub(super) fn split_stacked(area: Rect, cards: bool) -> (Rect, Rect, bool) {
    let list = if cards {
        Constraint::Ratio(1, 2)
    } else {
        Constraint::Ratio(1, 3)
    };

    let [list, _, preview] = Layout::vertical([list, Constraint::Length(1), Constraint::Fill(1)]).areas(area);

    (list, preview, true)
}

pub(super) fn split_side_by_side(area: Rect, cards: bool) -> (Rect, Rect, bool) {
    let list = if cards {
        Constraint::Percentage(60)
    } else {
        Constraint::Percentage(40)
    };

    let [list, _, preview] = Layout::horizontal([list, Constraint::Length(1), Constraint::Fill(1)]).areas(area);

    (list, preview, false)
}
