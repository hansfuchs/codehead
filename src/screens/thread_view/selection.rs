use super::{DOUBLE_ESC, Duration, Pane, View, chat, content_cols, json};

impl View {
    /// Plain text of the selection: the message's text only (0 borders, 0 time, 0 copy button);
    /// rows a wrap split joined again (`Row::wrap`), so the copy has the source's lines.
    pub(super) fn selected(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.selection.sel?;

        let ((r0, c0), (r1, c1)) = if (r0, c0) <= (r1, c1) {
            ((r0, c0), (r1, c1))
        } else {
            ((r1, c1), (r0, c0))
        };

        let mut out: Vec<String> = Vec::new();

        for row_index in r0..=r1.min(self.stream.drawn.rows.len().saturating_sub(1)) {
            selected_row(self, r0, c0, r1, c1, &mut out, row_index);
        }

        // the common indent of the lines off; blank lines at the ends off
        let indent = out
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let lines: Vec<String> = out
            .iter()
            .map(|line| line.get(indent..).unwrap_or("").to_string())
            .collect();
        let text = lines.join("\n").trim_matches('\n').to_string();

        (!text.trim().is_empty()).then_some(text)
    }

    /// The rows of the message (stream item) at ROW: first, last.
    pub(super) fn item_rows(&self, row: usize) -> (usize, usize) {
        let starts = &self.stream.drawn.starts;
        let last = self.stream.drawn.rows.len().saturating_sub(1);

        match starts.partition_point(|&start| start <= row).checked_sub(1) {
            Some(i) => (
                starts[i],
                starts
                    .get(i + 1)
                    .map_or(last, |next| next.saturating_sub(1))
                    .max(starts[i]),
            ),
            None => (0, starts.first().map_or(last, |first| first.saturating_sub(1))),
        }
    }

    pub(super) fn copy(&mut self, text: &str) {
        let ok = crate::infra::clip::copy(text);

        self.toast(if ok {
            format!("copied {} chars", text.chars().count())
        } else {
            "0 clipboard tools found (wl-copy, xclip, xsel)".into()
        });
    }

    /// Something on screen moves: an agent at work (wave), monitors, the threads row, a toast.
    pub(super) fn animating(&self) -> bool {
        // the avatars of the agents box move
        !self.agents.members.is_empty()
            || !self.thread.busy.is_empty()
            || self.monitors.active()
            || crate::features::activity_rows::subagents::Subagents::active(&self.thread.subs)
            || self.subthreads.active()
            || self.panes.changes.pushing()
            || self
                .toast
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(4))
            || (self.panes.pane == Pane::Merge && self.merge_state.merge.armed())
    }

    /// Fire an interrupt armed by a single Esc once `DOUBLE_ESC` ends. True: state changed.
    pub(super) fn tick_esc(&mut self) -> bool {
        if self.esc_interrupt.is_some_and(|at| at.elapsed() >= DOUBLE_ESC) {
            self.esc_interrupt = None;
            self.interrupt();
            return true;
        }

        false
    }

    pub(super) fn interrupt(&mut self) {
        if self.thread.busy.is_empty() && self.prompt.queued.is_none() {
            return;
        }

        if let Some((new, old, _)) = self.prompt.queued.take() {
            self.toast(format!("replace {old} → {new}: dropped"));
        }

        self.host("interrupt", json!({"op": "interrupt"}));
        self.toast("interrupt…");
    }

    /// Search match I (item index): its tool run open, the stream scrolls to it at the next draw.
    pub(super) fn find_show(&mut self, i: usize) {
        if let Some(chat::Item::Run { key, .. }) = self.thread.items.get(i) {
            self.stream.open.insert(*key);
        }

        self.find_to = Some(i);
        self.stream.dirty = true;
    }

    pub(super) fn scroll(&mut self, delta: isize) {
        let max = self
            .stream
            .drawn
            .rows
            .len()
            .saturating_sub(usize::from(self.layout.stream_rect.height));

        self.stream.back = (self.stream.back as isize + delta).clamp(0, max as isize) as usize;
    }

    /// ctrl+t: every thought open / folded. Hidden (setting): shown folded first.
    pub(super) fn toggle_thoughts(&mut self) {
        if self.stream.thoughts == chat::Thoughts::Hidden {
            self.stream.thoughts = chat::Thoughts::Folded;
            self.stream.all_thoughts = true;
            self.toast("thoughts shown · setting: thoughts");
        } else {
            self.stream.all_thoughts = !self.stream.all_thoughts;
        }

        self.stream.open_thoughts.clear();
        self.stream.dirty = true;
    }

    /// Index in drawn.rows of screen row Y of the stream.
    pub(super) fn row_at(&self, y: u16) -> Option<usize> {
        if y < self.layout.stream_rect.y || y >= self.layout.stream_rect.bottom() {
            return None;
        }

        let height = usize::from(self.layout.stream_rect.height);
        let total = self.stream.drawn.rows.len();
        let end = total.saturating_sub(self.stream.back);
        let start = end.saturating_sub(height);

        // bottom-aligned: a short stream starts lower
        let pad = height.saturating_sub(end - start);
        let off = usize::from(y - self.layout.stream_rect.y).checked_sub(pad)?;
        let i = start + off;

        (i < end).then_some(i)
    }

    pub(super) fn tick(&self) -> usize {
        (self.started.elapsed().as_millis() / 150) as usize
    }
}

fn selected_row(view: &View, r0: usize, c0: u16, r1: usize, c1: u16, out: &mut Vec<String>, row_index: usize) {
    let row = &view.stream.drawn.rows[row_index];
    let text: String = row.line.spans.iter().map(|span| span.content.as_ref()).collect();

    let Some((start, end)) = content_cols(&text) else {
        return;
    };

    let chars: Vec<char> = text.chars().collect();

    let start = if row_index == r0 {
        start.max(usize::from(c0))
    } else {
        start
    };

    let end = if row_index == r1 {
        end.min(usize::from(c1) + 1)
    } else {
        end
    };

    let piece: String = chars
        .get(start.min(end)..end)
        .map(|slice| slice.iter().collect())
        .unwrap_or_default();
    let piece = piece.trim_end();
    let piece = piece
        .strip_prefix("↪ ")
        .or_else(|| piece.trim_start().strip_prefix("↪ "))
        .unwrap_or(piece);

    match out.last_mut() {
        Some(last) if row.wrap == 1 && row_index != r0 => {
            last.push(' ');
            last.push_str(piece.trim_start());
        }
        Some(last) if row.wrap == 2 && row_index != r0 => last.push_str(piece.trim_start()),
        _ => out.push(piece.to_string()),
    }
}
