use super::{Frame, Path, Rect, ShellComp, TextArea, dropdown, runner_of};

impl ShellComp {
    /// The dropdown above AVOID (the prompt box), at the word's column, inside BOUNDS.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame,
        ta: &TextArea,
        dir: &Path,
        thread: &[String],
        bounds: Rect,
        avoid: Rect,
    ) {
        render_shell_comp(self, frame, ta, dir, thread, bounds, avoid);
    }
}

fn marks(ctx: &super::Ctx, count_value: usize, text: &str) -> Vec<usize> {
    if count_value > 0 && text.to_lowercase().starts_with(&ctx.word.to_lowercase()) {
        (0..count_value).collect()
    } else {
        Vec::new()
    }
}

fn render_shell_comp(
    shell: &mut ShellComp,
    frame: &mut Frame,
    ta: &TextArea,
    dir: &Path,
    thread: &[String],
    bounds: Rect,
    avoid: Rect,
) {
    shell.hide();

    let Some((ctx, entries)) = shell.open(ta, dir, thread) else {
        return;
    };

    let Some(path) = ta.screen_pos(ctx.at) else { return };

    let label = match runner_of(&ctx.before) {
        Some("package") => "scripts",
        Some(result) => result,
        None if ctx.before.is_empty() => "commands",
        None => ctx.before.first().map_or("complete", String::as_str),
    };

    let width_value = entry_width(&entries);

    let Some(area) = dropdown::place(
        Rect {
            x: path.x,
            y: avoid.y,
            width: 1,
            height: 1,
        },
        width_value,
        entries.len(),
        bounds,
        true,
    ) else {
        return;
    };

    let rows = entry_rows(&ctx, &entries);

    let note = if shell.active {
        "↑↓ tab · enter take · esc"
    } else if entries.len() == 1 {
        "tab take"
    } else {
        "tab pick"
    };

    let (off, rows) = dropdown::list_in(
        frame,
        area,
        dropdown::List {
            label,
            note,
            rows: &rows,
            cursor: shell.cursor,
            offset: shell.offset,
            empty: "",
        },
        shell.active,
    );

    shell.offset = off;
    shell.rows = rows;
    shell.area = area;
}

fn entry_rows(ctx: &super::Ctx, entries: &[super::Entry]) -> Vec<dropdown::Row> {
    let count_value = ctx.word.chars().count();

    entries
        .iter()
        .map(|error_value| {
            let text = error_value.text.trim_end().to_string();

            let marks = marks(ctx, count_value, &text);

            dropdown::Row {
                text,
                detail: error_value.detail.clone(),
                marks,
                ..Default::default()
            }
        })
        .collect()
}

fn entry_width(entries: &[super::Entry]) -> u16 {
    entries
        .iter()
        .map(|error| error.text.chars().count() + error.detail.chars().count().min(40) + 8)
        .max()
        .unwrap_or(0)
        .clamp(28, 90) as u16
}
