use super::{ActivityLayout, Frame, PromptLayout, View};
impl View {
    pub(super) fn draw(&mut self, frame: &mut Frame) {
        self.prepare_draw();
        let (top, area, changes_left) = self.draw_bounds(frame);
        let prompt = PromptLayout::new(self, area);
        let activity = ActivityLayout::new(self, &prompt);

        self.layout.stream_rect = activity.stream;
        self.layout.view_rect = activity.stream;
        self.draw_pane(frame, activity.stream, changes_left);
        self.draw_activity(frame, &activity);
        self.draw_bar(frame, top);
        self.draw_prompt_status(frame, prompt.title);
        self.draw_header_cost(frame, prompt.header);
        let commands = self.draw_prompt(frame, prompt.input, prompt.bubble, prompt.search);
        let cwd = self.draw_hints(frame, prompt.hint);

        self.draw_footer(frame, prompt.footer);
        self.draw_spawns(frame);
        self.draw_overlays(frame, area, cwd, commands.as_deref());
        self.selection.last_frame.clone_from(frame.buffer_mut());
    }
}
