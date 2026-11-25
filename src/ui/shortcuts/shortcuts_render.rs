use super::{Frame, Pick, Rect, TextArea, UnicodeWidthStr, box_size, dropdown, text_box};

impl Pick {
    /// The dropdown for the `#` of TA (TA rendered this frame) inside BOUNDS, never over AVOID
    /// (the prompt box), and the box with the current (hovered) shortcut's text beside it.
    pub(crate) fn render(&mut self, frame: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
        self.hide();
        let Some(output) = self.open(ta) else { return };
        let Some(path) = ta.screen_pos(output.at) else { return };
        let above = avoid.y.saturating_sub(bounds.y) >= 5
            || avoid.y.saturating_sub(bounds.y) >= bounds.bottom().saturating_sub(avoid.bottom());
        let count = output.matches.len();
        let width_value = (output
            .matches
            .iter()
            .map(|&i| output.list[i].name.width())
            .max()
            .unwrap_or(0)
            + 6)
        .clamp(20, 40) as u16;

        let (anchor, room) = if above {
            (
                Rect {
                    x: path.x,
                    y: avoid.y,
                    width: 1,
                    height: 1,
                },
                Rect {
                    height: avoid.y.saturating_sub(bounds.y),
                    ..bounds
                },
            )
        } else {
            let y = avoid.bottom().clamp(bounds.y, bounds.bottom());

            (
                Rect {
                    x: path.x,
                    y: avoid.bottom().saturating_sub(1),
                    width: 1,
                    height: 1,
                },
                Rect {
                    y,
                    height: bounds.bottom() - y,
                    ..bounds
                },
            )
        };

        let Some(area) = dropdown::place(anchor, width_value, count, room, above) else {
            return;
        };

        let rows = shortcut_rows(&output);

        self.draw_preview(
            frame,
            &(output),
            &(rows),
            ShortcutLayout {
                bounds,
                count,
                room,
                area,
            },
        );
    }

    fn draw_preview(
        &mut self,
        frame: &mut Frame<'_>,
        output: &super::Open,
        rows: &Vec<dropdown::Row>,
        settings: ShortcutLayout,
    ) {
        let ShortcutLayout {
            bounds,
            count,
            room,
            area,
        } = settings;

        let (off, hits) = dropdown::list(
            frame,
            area,
            dropdown::List {
                label: "shortcuts",
                note: "tab / enter",
                rows,
                cursor: self.cursor,
                offset: self.offset,
                empty: "0 shortcuts",
            },
        );

        self.offset = off;
        self.rows = hits;
        self.area = area;

        // the 2nd box: the text of the hovered row, else of the current one
        let cur = self
            .hover
            .filter(|&height_value| height_value < count)
            .unwrap_or(self.cursor);
        let text_value = &output.list[output.matches[cur]];
        let (bw, br) = box_size(&text_value.text, bounds);
        let row_y = self
            .rows
            .iter()
            .find(|(_, i)| *i == cur)
            .map_or(area.y + 1, |(result, _)| result.y);

        if let Some(row) = dropdown::place_beside(area, row_y, bw, br, room, area) {
            text_box(frame, row, &format!("#{}", text_value.name), &text_value.text);
        }
    }
}

#[derive(Clone, Copy)]
struct ShortcutLayout {
    bounds: Rect,
    count: usize,
    room: Rect,
    area: Rect,
}

fn shortcut_rows(output: &super::Open) -> Vec<dropdown::Row> {
    output
        .matches
        .iter()
        .map(|&i| dropdown::Row {
            text: format!("#{}", output.list[i].name),
            detail: String::new(),
            now: false,
            marks: Vec::new(),
        })
        .collect()
}
