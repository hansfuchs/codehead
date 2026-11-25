use super::{Act, Focus, Mode, MouseButton, MouseEvent, MouseEventKind, Out, Position, ProjTab, StatusTab, ThreadList};

impl ThreadList {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out {
        mouse_thread_list(self, event)
    }
}

fn mouse_thread_list(state: &mut ThreadList, event: MouseEvent) -> Out {
    state.poll(true);
    let at = Position {
        x: event.column,
        y: event.row,
    };

    if state.form.is_some() {
        return Out::None;
    }

    if state.mode == Mode::Switch && state.filter.mouse(event) {
        state.filter.copied = None;
        return Out::None;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => click_thread_list(state, at),
        MouseEventKind::ScrollDown if state.view.cards() && state.list.contains(at) => {
            state.cursor = (state.cursor + 1).min(state.visible().len().saturating_sub(1));
            Out::None
        }
        MouseEventKind::ScrollUp if state.view.cards() && state.list.contains(at) => {
            state.cursor = state.cursor.saturating_sub(1);
            Out::None
        }
        _ => Out::Pass,
    }
}

fn click_thread_list(state: &mut ThreadList, at: Position) -> Out {
    if let Some(&(_, act)) = state.hit_regions.act_rows.iter().find(|(rect, _)| rect.contains(at)) {
        return state.run(act);
    }

    state.job_state.actions = None;
    state.job_state.pending = None;

    if state.repos.click(at) {
        state.focus = Focus::Repos;
        state.repo_pick = state.repo_tabs().get(state.repos.i).cloned();
        state.cursor = 0;
        state.offset = 0;
        return Out::None;
    }

    if state.projs.click(at) {
        state.focus = Focus::Projects;
        state.proj_pick = state.proj_tabs().get(state.projs.i).cloned().unwrap_or(ProjTab::All);
        state.cursor = 0;
        state.offset = 0;
        return Out::None;
    }

    if state.views.click(at) {
        state.focus = Focus::Views;
        let all = state.views_all();

        state.set_view(all[state.views.i.min(all.len() - 1)]);
        return Out::None;
    }

    if state.has_stats() && state.stats.click(at) {
        state.focus = Focus::Status;
        state.stat_pick = StatusTab::ALL.get(state.stats.i).copied();
        state.cursor = 0;
        state.offset = 0;
        return Out::None;
    }

    let Some(&(_, row)) = state.hit_regions.rows.iter().find(|(rect, _)| rect.contains(at)) else {
        return Out::Pass;
    };

    state.focus = Focus::List;

    // the menu: a click opens; the start screen: a click picks, a 2nd opens
    if state.mode == Mode::Switch || row == state.cursor {
        state.cursor = row;
        return state.run(Act::Open);
    }

    state.cursor = row;

    Out::None
}
