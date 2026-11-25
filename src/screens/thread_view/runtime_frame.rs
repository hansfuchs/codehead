use super::{Backend, Duration, Event, Instant, KeyEventKind, Pane, Path, Runtime, Terminal, run_outside, suspended};

impl Runtime {
    pub(super) fn frame_interval(&self) -> Duration {
        if self.view.spawning()
            || crate::ui::dissolve::active()
            || self
                .view
                .menu
                .as_ref()
                .is_some_and(crate::screens::quick_menu::Menu::waiting)
        {
            Duration::from_millis(33)
        } else if self.view.animating() {
            Duration::from_millis(150)
        } else {
            Duration::from_secs(1)
        }
    }

    pub(super) fn notifications_changed(&mut self) {
        let version = crate::domain::notifications::version() + crate::infra::bus::version();

        if version != self.notes_version {
            self.notes_version = version;
            self.dirty = true;
        }
    }

    pub(super) fn draw_frame<B: Backend>(&mut self, term: &mut Terminal<B>, every: Duration) -> Result<(), String> {
        if !self.dirty && self.drawn.elapsed() < every {
            return Ok(());
        }

        let (pane, menu) = (self.view.panes.pane, self.view.menu.is_some());
        let _watch = crate::engine::watch(move || format!("view: draw {pane:?}{}", if menu { " + menu" } else { "" }));

        term.draw(|frame| self.view.draw(frame))
            .map_err(|error| error.to_string())?;
        self.drawn = Instant::now();
        self.dirty = false;
        crate::engine::first_frame();

        if let Some(at) = self.opening.take() {
            record_open(at);
        }

        Ok(())
    }

    pub(super) fn read_keys(&mut self, every: Duration) -> Result<bool, String> {
        let wait = every.saturating_sub(self.drawn.elapsed()).max(Duration::from_millis(1));

        Ok(crate::infra::terminal::read_ready(wait)?
            .into_iter()
            .any(|(event, at)| self.event(event, at)))
    }

    fn event(&mut self, event: Event, at: Instant) -> bool {
        let tag = if crate::engine::trace_on() {
            format!(
                "view {:?}{}: {event:?}",
                self.view.panes.pane,
                if self.view.menu.is_some() { " + menu" } else { "" }
            )
        } else {
            String::new()
        };

        let _watch = crate::engine::watch(move || tag);

        crate::ui::textarea::stamp(Some(at));
        self.dirty = true;
        let done = self.route_event(event);

        crate::ui::textarea::stamp(None);
        self.view.save_prompt();

        done
    }

    fn route_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.view.key(key),
            Event::Mouse(event) => {
                self.view.mouse(event);
                self.view.mouse_exit
            }
            Event::Paste(text) => {
                self.view.paste(&text);
                false
            }
            _ => false,
        }
    }

    pub(super) fn outside<B: Backend>(&mut self, term: &mut Terminal<B>) {
        if let Some(arguments) = self.view.panes.outside.take() {
            self.editor(term, &arguments);
        }

        if let Some(target) = self.view.panes.yazi.take() {
            self.yazi(term, &target);
        }
    }

    fn editor<B: Backend>(&mut self, term: &mut Terminal<B>, arguments: &[std::ffi::OsString]) {
        match suspended(term, arguments, &[]) {
            Ok(status) if status.success() => {}
            Ok(status) => self.view.toast(format!("editor exited with {status}")),
            Err(error) => self.view.toast(format!("editor: {error}")),
        }

        self.view.panes.files.forget_images();
        self.dirty = true;
    }

    fn yazi<B: Backend>(&mut self, term: &mut Terminal<B>, target: &Path) {
        match run_outside(term, target) {
            Ok(paths) => self.view.yazi_picked(&paths),
            Err(error) => self.view.toast(error),
        }

        self.view.panes.files.forget_images();
        self.dirty = true;
    }

    pub(super) fn poll(&mut self) {
        self.dirty |= self.view.prompt.hist.poll();
        self.dirty |= self.view.prompt.comp.poll();
        self.dirty |= self.view.merge_state.merge.poll(self.view.panes.pane == Pane::Merge);
        self.dirty |= self.view.panes.files.poll();

        if let Some(toast) = self.view.panes.files.take_toast() {
            self.view.toast(toast);
            self.dirty = true;
        }

        self.dirty |= self.view.poll_merge_sources();
    }
}

fn record_open(at: Instant) {
    let took = at.elapsed();

    if took > crate::engine::BUDGET {
        crate::engine::trace(&format!("SLOW {} view open → 1st frame", took.as_millis()));
    }

    crate::engine::transition_end();
}
