use super::{
    AgentState, CheckoutState, CostBox, Exit, IdeState, Instant, LayoutState, MergeState, MiniChatState, Msg,
    PaneState, Path, PromptState, SelectionState, ShellState, StreamState, UrlState, View, chat, store,
};
impl View {
    pub(super) fn new(id: &str, cwd: &Path, title: &str, tx: crate::infra::bus::Tx<Msg>) -> Self {
        let roster = crate::domain::agents::roster_only();
        let mut view = Self::initial_state(id, cwd, title, tx, &roster);

        view.set_roster(roster);
        view.read();
        view.load_prompt();
        view.load_shell_cwd();

        view
    }
    fn initial_state(
        id: &str,
        cwd: &Path,
        title: &str,
        tx: crate::infra::bus::Tx<Msg>,
        roster: &[crate::domain::agents::Agent],
    ) -> Self {
        let (tail, head_end) = initial_tail(id, &tx);

        Self {
            id: id.to_string(),
            title: title.lines().next().unwrap_or_default().to_string(),
            cwd: cwd.to_path_buf(),
            tail,
            early: (head_end > 0).then(Vec::new),
            thread: chat::Thread::default(),
            ask: crate::features::chat::ask::Picker::default(),
            #[cfg(test)]
            ops: std::cell::RefCell::default(),
            ctx: None,
            notes_panel: None,
            watch: crate::features::notifications::Watch::default(),
            handing: None,
            forking: None,
            monitors: crate::features::activity_rows::monitors::Monitors::new(id),
            subagents: crate::features::activity_rows::subagents::Subagents::default(),
            subthreads: crate::features::activity_rows::subthreads::SubThreads::default(),
            flowbars: Vec::new(),
            find: crate::features::chat::chat_search::ChatSearch::default(),
            find_to: None,
            menu: None,
            cost_box: CostBox::new(),
            toast: None,
            started: Instant::now(),
            last_esc: None,
            esc_interrupt: None,
            last_ctrl_c: None,
            last_key: None,
            exit: Exit::Back,
            mouse_exit: false,
            tx,
            agents: AgentState::new(),
            stream: StreamState::new(id),
            selection: SelectionState::new(),
            prompt: PromptState::new(id, cwd, roster),
            shell_state: ShellState::new(cwd),
            checkout_state: CheckoutState::new(),
            panes: PaneState::new(id, cwd),
            layout: LayoutState::new(),
            ide: IdeState::new(),
            merge_state: MergeState::new(id),
            urls_state: UrlState::new(),
            mini_chat_state: MiniChatState::new(),
        }
    }
}
fn initial_tail(id: &str, tx: &crate::infra::bus::Tx<Msg>) -> (store::Tail, u64) {
    let (tail, head_end) = if cfg!(test) {
        (store::Tail::new(id), 0)
    } else {
        store::Tail::from_end(id, 256 << 10)
    };

    if head_end > 0 {
        let (id, tx) = (id.to_string(), tx.clone());

        std::thread::spawn(move || {
            let _ = tx.send(Msg::Head(store::head(&id, head_end)));
        });
    }

    (tail, head_end)
}
