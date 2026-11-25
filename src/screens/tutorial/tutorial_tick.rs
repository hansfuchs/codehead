use super::{Coach, Task};

impl Coach {
    /// Task T done when it is the next one. true: a change.
    pub(super) fn tick(&mut self, thread: Task) -> bool {
        if self.current() == Some(thread) {
            self.done.push(thread);
            return true;
        }

        false
    }
}
