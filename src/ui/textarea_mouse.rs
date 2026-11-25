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
            MouseEventKind::Up(MouseButton::Left) if self.drag || inside => self.mouse_thread_value(),
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

    fn mouse_thread_value(&mut self) -> bool {
        self.drag = false;

        if let Some(thread_value) = self.selected_text()
            && clip::copy(&thread_value)
        {
            self.copied = Some(thread_value.chars().count());
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

fn mouse_position(event: MouseEvent, text: &TextArea) -> Pos {
    let y = (event.row as isize - text.area.y as isize).clamp(0, text.area.height.saturating_sub(1) as isize) as usize;
    let x = event.column.saturating_sub(text.area.x) as usize;

    text.pos_at(text.scroll + y, x)
}
fn mouse_press(event: MouseEvent, area: &mut TextArea) -> bool {
    let path = mouse_position(event, area);

    if event.modifiers.contains(KeyModifiers::SHIFT) {
        area.move_to(path, true);
        return true;
    }

    let count_2 = match area.clicks {
        Some((thread, query, count_value)) if query == path && thread.elapsed() < Duration::from_millis(400) => {
            count_value % 3 + 1
        }
        _ => 1,
    };

    area.clicks = Some((Instant::now(), path, count_2));
    area.last_op = None;
    area.want_x = None;

    match count_2 {
        2 => area.select_word(path),
        3 => {
            area.anchor = Some(Pos { row: path.row, col: 0 });
            area.cur = Pos {
                row: path.row,
                col: area.lines[path.row].len(),
            };
        }
        _ => {
            area.move_to(path, false);

            if area.range().is_none() {
                area.anchor = Some(path);
            }
        }
    }

    area.drag = count_2 == 1;
    area.follow = true;

    true
}
