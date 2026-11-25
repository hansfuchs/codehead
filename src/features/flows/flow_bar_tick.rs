use super::{Duration, FlowBar, Instant};

impl FlowBar {
    /// Read the agent's flow of thread ID again: every second, at once with FORCE (a flow event came).
    pub(crate) fn tick(&mut self, id: &str, force: bool) -> bool {
        if !force
            && self
                .read_at
                .is_some_and(|thread| thread.elapsed() < Duration::from_secs(1))
        {
            return false;
        }

        self.read_at = Some(Instant::now());

        let Some(thread_value) = crate::engine::store::load(id) else {
            return false;
        };

        self.sync(&thread_value, force)
    }
}
