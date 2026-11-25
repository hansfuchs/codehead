use super::{App, CARD_H, Constraint, Layout, Rect};

#[derive(Clone, Copy)]
pub(super) struct TaskLayout {
    pub(super) n_notes: usize,
    pub(super) opts_h: u16,
    pub(super) proj_h: u16,
    pub(super) r_drafts: Rect,
    pub(super) l_prompt: Rect,
    pub(super) i_prompt: Rect,
    pub(super) l_title: Rect,
    pub(super) i_title: Rect,
    pub(super) l_where: Rect,
    pub(super) repo_row: Rect,
    pub(super) proj_row: Rect,
    pub(super) info: Rect,
    pub(super) opts: Rect,
    pub(super) l_agents: Rect,
    pub(super) r_agents: Rect,
    pub(super) l_notes: Rect,
    pub(super) notes: Rect,
}

impl App {
    pub(super) fn task_layout(&self, area: Rect) -> TaskLayout {
        let title_rows = (self.compose.title.rows_for(area.width.saturating_sub(2)) as u16).min(3);
        let n_notes = self.note_list.selected.len();
        let note_rows = n_notes.min(5) as u16 + u16::from(n_notes > 5) + self.compose.carry.len().min(5) as u16;

        // the opts boxes below the where row: 1 row each
        let (opts_h, branch_h, proj_h, drafts_h, available) = task_location_heights(self, area, title_rows, note_rows);

        let agents_h = task_agent_height(self, area, available);

        // prompt first and largest (the rest of the height), then title, where (repo, branch,
        // opts), agents, notes, a
        // blank row below the notes (above the footer)
        let [
            r_drafts,
            l_prompt,
            i_prompt,
            _,
            l_title,
            i_title,
            _,
            l_where,
            repo_row,
            proj_row,
            info,
            opts,
            _,
            l_agents,
            r_agents,
            _,
            l_notes,
            notes,
            _,
        ] = task_rectangles(
            area,
            TaskHeights {
                title_rows,
                note_rows,
                opts_h,
                branch_h,
                proj_h,
                agents_h,
                drafts_h,
            },
        );

        TaskLayout {
            n_notes,
            opts_h,
            proj_h,
            r_drafts,
            l_prompt,
            i_prompt,
            l_title,
            i_title,
            l_where,
            repo_row,
            proj_row,
            info,
            opts,
            l_agents,
            r_agents,
            l_notes,
            notes,
        }
    }
}

#[derive(Clone, Copy)]
struct TaskHeights {
    title_rows: u16,
    note_rows: u16,
    opts_h: u16,
    branch_h: u16,
    proj_h: u16,
    agents_h: u16,
    drafts_h: u16,
}

fn task_rectangles(area: Rect, heights: TaskHeights) -> [Rect; 19] {
    let TaskHeights {
        title_rows,
        note_rows,
        opts_h,
        branch_h,
        proj_h,
        agents_h,
        drafts_h,
    } = heights;

    Layout::vertical([
        Constraint::Length(drafts_h),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(title_rows),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(proj_h),
        Constraint::Length(branch_h),
        Constraint::Length(opts_h),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(agents_h),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(note_rows),
        Constraint::Length(1),
    ])
    .areas(area)
}

fn task_location_heights(app: &App, area: Rect, title_rows: u16, note_rows: u16) -> (u16, u16, u16, u16, u16) {
    let opts_h = if app.repository.wheres.is_empty() { 0 } else { 2 };

    // "where": a label, the repo row, the branch row (a git repo only)
    let branch_h = u16::from(!app.repository.wheres.is_empty());

    // the projects row: a git repo only
    let proj_h = u16::from(app.cfg.repo.is_some());

    // the drafts row: "view drafts (n)" (1+ drafts), "save as draft"; a blank row below
    let drafts_h = 2;
    let available = area
        .height
        .saturating_sub(drafts_h + title_rows + opts_h + note_rows + branch_h + proj_h + 15);

    (opts_h, branch_h, proj_h, drafts_h, available)
}

fn task_agent_height(app: &App, area: Rect, available: u16) -> u16 {
    if app.agent_list.agents.is_empty() {
        1
    } else {
        let grid = app.agent_grid(area.width);
        let view = grid
            .height
            .min(2 * (CARD_H as usize + 1))
            .min(available.max(CARD_H + 1) as usize);

        // only the rows the cards fill: the rest goes to the prompt
        match grid
            .places
            .iter()
            .find(|&&(i, _, _)| i == app.compose.pick_cursor)
            .or(grid.places.first())
        {
            Some(&(_, _, top)) => {
                let off = crate::ui::cards::scroll(&grid, top, CARD_H, app.compose.pick_offset, view);

                crate::ui::cards::used(&grid, CARD_H, off, view).max(1) as u16
            }
            None => view as u16,
        }
    }
}
