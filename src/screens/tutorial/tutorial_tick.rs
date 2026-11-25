use super::{Coach, Task};

impl Coach {
    /// Task T done when it is the next one. true: a change.
    pub(super) fn tick(&mut self, task: Task) -> bool {
        if self.current() == Some(task) {
            self.done.push(task);
            return true;
        }

        false
    }
}
