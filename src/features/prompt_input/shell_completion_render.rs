use super::{Frame, Path, Rect, ShellComp, TextArea, dropdown, runner_of};

impl ShellComp {
    /// The dropdown above AVOID (the prompt box), at the word's column, inside BOUNDS.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame,
        ta: &TextArea,
        dir: &Path,
        thread_commands: &[String],
        bounds: Rect,
        avoid: Rect,
    ) {
        render_shell_comp(self, frame, ta, dir, thread_commands, bounds, avoid);
    }
}

fn marks(ctx: &super::Ctx, typed_chars: usize, text: &str) -> Vec<usize> {
    if typed_chars > 0 && text.to_lowercase().starts_with(&ctx.word.to_lowercase()) {
        (0..typed_chars).collect()
    } else {
        Vec::new()
    }
}

fn render_shell_comp(
    shell: &mut ShellComp,
    frame: &mut Frame,
    ta: &TextArea,
    dir: &Path,
    thread_commands: &[String],
    bounds: Rect,
    avoid: Rect,
) {
    shell.hide();

    let Some((ctx, entries)) = shell.open(ta, dir, thread_commands) else {
        return;
    };

    let Some(screen) = ta.screen_pos(ctx.at) else { return };

    let label = match runner_of(&ctx.before) {
        Some("package") => "scripts",
        Some(runner) => runner,
        None if ctx.before.is_empty() => "commands",
        None => ctx.before.first().map_or("complete", String::as_str),
    };

    let width = entry_width(&entries);

    let Some(area) = dropdown::place(
        Rect {
            x: screen.x,
            y: avoid.y,
            width: 1,
            height: 1,
        },
        width,
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
    let typed_chars = ctx.word.chars().count();

    entries
        .iter()
        .map(|entry| {
            let text = entry.text.trim_end().to_string();

            let marks = marks(ctx, typed_chars, &text);

            dropdown::Row {
                text,
                detail: entry.detail.clone(),
                marks,
                ..Default::default()
            }
        })
        .collect()
}

fn entry_width(entries: &[super::Entry]) -> u16 {
    entries
        .iter()
        .map(|entry| entry.text.chars().count() + entry.detail.chars().count().min(40) + 8)
        .max()
        .unwrap_or(0)
        .clamp(28, 90) as u16
}
