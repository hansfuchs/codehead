use super::{Op, Snapshot, TextArea};

impl TextArea {
    /// Push an undo step (a run of Type / Delete = 1 step). True: step pushed.
    pub(super) fn snapshot(&mut self, op: Op) -> bool {
        if op != Op::Other && self.last_op == Some(op) {
            return false;
        }

        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cur: self.cur,
        });

        if self.undo.len() > 200 {
            self.undo.remove(0);
        }

        self.redo.clear();
        self.last_op = Some(op);

        true
    }

    pub(super) fn restore(&mut self, from_undo: bool) {
        let (src, dst) = if from_undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };

        if let Some(text) = src.pop() {
            dst.push(Snapshot {
                lines: self.lines.clone(),
                cur: self.cur,
            });
            self.lines = text.lines;
            self.cur = text.cur;
            self.anchor = None;
            self.last_op = None;
            self.follow = true;
            self.want_x = None;
        }
    }

    /// After an edit: 0 change → drop the step SNAPSHOT pushed (if it pushed 1).
    pub(super) fn drop_noop(&mut self, pushed: bool) {
        if pushed && self.undo.last().is_some_and(|text| text.lines == self.lines) {
            self.undo.pop();
            self.last_op = None;
        }
    }
}
