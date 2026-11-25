use super::{Clear, Frame, Pane, Rect, Region, View, ctx_of};

impl View {
    pub(super) fn draw_overlays(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        cwd_rect: Rect,
        comp_cmds: Option<&[String]>,
    ) {
        // dropdowns over everything, then the menu
        if self.menu.is_none() {
            self.draw_prompt_dropdowns(frame, area, cwd_rect, comp_cmds);
        }

        self.draw_ide_pick(frame, area);

        self.refresh_context_menu();

        let place = self.ctx_place();

        if let Some(ctx) = self.ctx.as_mut() {
            ctx.render(frame, place, area);
        }

        if self.menu.is_some() {
            // live: an agent came or left, a flow moved on
            refresh_menu_items(self);
        }

        if let Some(menu) = self.menu.as_mut() {
            frame.render_widget(Clear, Rect::default());

            // a command palette: below the view bar, centered on the screen's width
            menu.render(frame, frame.area());
        }

        if let Some(panel) = self.notes_panel.as_mut() {
            panel.draw(frame, frame.area());
        }
    }

    pub(super) fn draw_prompt_dropdowns(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        cwd_rect: Rect,
        comp_cmds: Option<&[String]>,
    ) {
        let shell = self.shell_mode();

        self.draw_agents_menu(frame, area);
        self.prompt
            .mention
            .render(frame, &self.prompt.input, &self.agents.roster, area);
        self.prompt.hist.render(frame, area, self.layout.input_rect);
        self.prompt
            .command
            .render(frame, &self.prompt.input, area, self.layout.input_rect);
        self.prompt
            .shortcuts
            .render(frame, &self.prompt.input, area, self.layout.input_rect);
        self.prompt
            .paths
            .render(frame, &self.prompt.input, area, self.layout.input_rect);

        match comp_cmds {
            Some(cmds) => self.prompt.comp.render(
                frame,
                &self.prompt.input,
                &self.shell_state.shell_cwd,
                cmds,
                area,
                self.layout.input_rect,
            ),
            None => self.prompt.comp.hide(),
        }

        let open = self.prompt.shortcuts.is_open(&self.prompt.input);
        let keys = self.panes.pane == Pane::Chat || self.panes.prompt_focus;

        crate::ui::shortcuts::render_popup(frame, &self.prompt.input, self.selection.hover, keys, area, open);

        match self.shell_state.cwd_edit.as_ref() {
            Some(input) if shell => self.shell_state.cwd_paths.render(frame, input, area, cwd_rect),
            _ => self.shell_state.cwd_paths.hide(),
        }
    }
    pub(super) fn refresh_context_menu(&mut self) {
        // the panel's menu: live (a push starts, a commit comes: its ship steps change)
        if self.ctx.is_some() && self.region() == Region::Panel {
            refresh_context_menu_items(self);
        }
    }
}

fn refresh_menu_items(view: &mut View) {
    let items = view.menu_items();

    if let Some(menu) = view.menu.as_mut() {
        menu.set_items(items);
    }
}

fn refresh_context_menu_items(view: &mut View) {
    let (_, items) = view.panes.changes.ctx_items();
    let items = items
        .into_iter()
        .map(|item| item.map(|action| ctx_of(&action)))
        .collect();
    let head = view.panes.changes.ctx_head();

    if let Some(ctx) = view.ctx.as_mut() {
        ctx.set_items(items);
        ctx.head = head;
    }
}
