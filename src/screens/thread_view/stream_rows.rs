use super::stream::StreamFrame;
use super::{Line, Rect, Span, View, item_ts, theme};
impl View {
    pub(super) fn mark_stream_seen(&mut self) {
        if self.menu.is_some() || self.notes_panel.is_some() {
            return;
        }

        let ts = (self.stream.back > 0).then(|| {
            let bottom = self
                .stream
                .drawn
                .rows
                .len()
                .saturating_sub(self.stream.back)
                .min(self.stream.drawn.rows.len());
            let last = self
                .stream
                .drawn
                .starts
                .iter()
                .take(self.thread.items.len())
                .rposition(|&text| text < bottom);

            last.and_then(|i| self.thread.items[..=i].iter().rev().find_map(item_ts))
                .unwrap_or(0)
        });

        if ts != Some(0) {
            self.watch.on_screen(&self.id, ts);
        }
    }
    pub(super) fn stream_rows(&mut self, st: Rect) -> (Vec<Line<'static>>, StreamFrame) {
        let extra = self.pending_rows();
        let height = usize::from(st.height);
        let total = self.stream.drawn.rows.len();

        self.stream.back = self.stream.back.min(total.saturating_sub(height));
        let end = total - self.stream.back;
        let start = end.saturating_sub(height);

        // the search bar open: its shown match has the marker
        if self.find.open {
            self.stream.flash = self.find.current();
        }

        let flash = self.stream.flash.and_then(|i| {
            Some((
                *self.stream.drawn.starts.get(i)?,
                self.stream.drawn.starts.get(i + 1).copied().unwrap_or(total),
            ))
        });

        let mut lines = marked_stream_rows(self, start, end, flash);

        mark_search_words(self, &mut lines, start);
        let mut cut = 0;

        if self.stream.back == 0 && !extra.is_empty() {
            lines.extend(extra);
            cut = lines.len().saturating_sub(height);
            lines.drain(..cut);
        }

        // bottom-aligned: a short thread sits on the title row
        let pad = height.saturating_sub(lines.len()) as u16;
        let shown = Rect {
            y: st.y + pad,
            height: st.height - pad,
            ..st
        };

        (
            lines,
            StreamFrame {
                area: st,
                shown,
                start,
                end,
                cut,
                total,
            },
        )
    }
}
fn mark_stream_row(source: &Line<'static>, marked: bool) -> Line<'static> {
    let mut line = source.clone();

    if !marked {
        return line;
    }

    line.spans.insert(0, Span::styled("▌", theme::accent()));

    if let Some(text) = line.spans.get_mut(1)
        && let Some(rest) = text.content.strip_prefix([' ', '│', '┌', '└']).map(String::from)
    {
        text.content = rest.into();
    }

    line
}
fn mark_search_row(line: &mut Line<'static>, words: &[String], marked: bool) {
    if marked {
        *line = crate::features::chat::chat_search::mark(line, words);
    }
}

fn marked_stream_rows(view: &View, start: usize, end: usize, flash: Option<(usize, usize)>) -> Vec<Line<'static>> {
    view.stream.drawn.rows[start..end]
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let marked = flash.is_some_and(|(first, last)| (first..last).contains(&(start + offset)));

            mark_stream_row(&row.line, marked)
        })
        .collect()
}

fn mark_search_words(view: &View, lines: &mut [Line<'static>], start: usize) {
    let words = view.find.words();

    if !words.is_empty() {
        for (count, line) in lines.iter_mut().enumerate() {
            let item = view
                .stream
                .drawn
                .starts
                .partition_point(|&text| text <= start + count)
                .checked_sub(1);

            mark_search_row(line, &words, item.is_some_and(|i| view.find.is_hit(i)));
        }
    }
}
