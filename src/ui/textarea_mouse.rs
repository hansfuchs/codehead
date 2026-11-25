use super::{Duration, Instant, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Pos, Position, TextArea, clip};

impl TextArea {
    /// Mouse event; false = outside the input (and no drag running).
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> bool {
        let inside = self.area.contains(Position {
            x: event.column,
            y: event.row,
        });

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => mouse_press(event, self),
            MouseEventKind::Drag(MouseButton::Left) if self.drag => self.mouse_branch(event),
            MouseEventKind::Up(MouseButton::Left) if self.drag || inside => self.finish_drag(),
            MouseEventKind::ScrollUp if inside => {
                self.scroll = self.scroll.saturating_sub(3);
                self.follow = false;
                true
            }
            MouseEventKind::ScrollDown if inside => {
                let max = self.vis.len().saturating_sub(self.area.height as usize);

                self.scroll = (self.scroll + 3).min(max);
                self.follow = false;
                true
            }
            _ => false,
        }
    }

    fn finish_drag(&mut self) -> bool {
        self.drag = false;

        if let Some(text) = self.selected_text()
            && clip::copy(&text)
        {
            self.copied = Some(text.chars().count());
        }

        true
    }

    fn mouse_branch(&mut self, event: MouseEvent) -> bool {
        if event.row < self.area.y {
            self.scroll = self.scroll.saturating_sub(1);
        } else if event.row >= self.area.y + self.area.height {
            self.scroll = (self.scroll + 1).min(self.vis.len().saturating_sub(1));
        }

        self.cur = mouse_position(event, self);
        self.follow = true;
        true
    }
}

fn mouse_position(event: MouseEvent, ta: &TextArea) -> Pos {
    let y = (event.row as isize - ta.area.y as isize).clamp(0, ta.area.height.saturating_sub(1) as isize) as usize;
    let x = event.column.saturating_sub(ta.area.x) as usize;

    ta.pos_at(ta.scroll + y, x)
}
fn mouse_press(event: MouseEvent, ta: &mut TextArea) -> bool {
    let pos = mouse_position(event, ta);

    if event.modifiers.contains(KeyModifiers::SHIFT) {
        ta.move_to(pos, true);
        return true;
    }

    let clicks = match ta.clicks {
        Some((last, last_pos, last_clicks)) if last_pos == pos && last.elapsed() < Duration::from_millis(400) => {
            last_clicks % 3 + 1
        }
        _ => 1,
    };

    ta.clicks = Some((Instant::now(), pos, clicks));
    ta.last_op = None;
    ta.want_x = None;

    match clicks {
        2 => ta.select_word(pos),
        3 => {
            ta.anchor = Some(Pos { row: pos.row, col: 0 });
            ta.cur = Pos {
                row: pos.row,
                col: ta.lines[pos.row].len(),
            };
        }
        _ => {
            ta.move_to(pos, false);

            if ta.range().is_none() {
                ta.anchor = Some(pos);
            }
        }
    }

    ta.drag = clicks == 1;
    ta.follow = true;

    true
}
