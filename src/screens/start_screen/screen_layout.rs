use super::{App, Constraint, Layout, Rect, THIN, Tab};

impl App {
    pub(super) fn split(&self, area: Rect) -> (Rect, Rect, bool) {
        // agents: a card grid, more room than a list
        let cards = matches!(self.tab, Tab::Agents | Tab::New);

        if area.width < THIN {
            split_list_state(area, cards)
        } else {
            split_list(area, cards)
        }
    }
}

pub(super) fn split_list_state(area: Rect, cards: bool) -> (Rect, Rect, bool) {
    let list = if cards {
        Constraint::Ratio(1, 2)
    } else {
        Constraint::Ratio(1, 3)
    };

    let [agent, _, bytes] = Layout::vertical([list, Constraint::Length(1), Constraint::Fill(1)]).areas(area);

    (agent, bytes, true)
}

pub(super) fn split_list(area: Rect, cards: bool) -> (Rect, Rect, bool) {
    let list = if cards {
        Constraint::Percentage(60)
    } else {
        Constraint::Percentage(40)
    };

    let [agent_value, _, bytes_value] =
        Layout::horizontal([list, Constraint::Length(1), Constraint::Fill(1)]).areas(area);

    (agent_value, bytes_value, false)
}
