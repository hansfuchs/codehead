use super::{
    Ask, Color, Drawn, Fold, Hit, Line, Modifier, Picker, Row, Span, Style, Sub, Thoughts, UnicodeWidthStr, markdown,
    memo, theme,
};

/// Rows of the report of subagent SUB (a message of its master), BAR before each: `↳ subagent
/// “what” · done · 2m10s`, then the report (folded to 6 lines, KEY opens it) or why it ended (lost:
/// a `pick up` row: the master goes on from the subagent's context).
pub(super) fn sub_rows(
    directory: &mut Drawn,
    text_value: &Sub,
    sub: usize,
    key: usize,
    fold: &Fold,
    width: usize,
    bar: &[Span<'static>],
) {
    let pal = theme::pal();
    let state = text_value.state.as_deref().unwrap_or("running");

    let color = match state {
        "done" => pal.success(),
        "stopped" => pal.subtext0,
        _ => pal.error(),
    };

    let secs = text_value.end.saturating_sub(text_value.start) / 1000;

    let took = if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };

    let head = [
        bar.to_vec(),
        vec![
            Span::styled("↳ subagent ", theme::accent()),
            Span::styled(format!("“{}”", text_value.description), theme::bold()),
            Span::styled(if text_value.resumed { " · resumed · " } else { " · " }, theme::muted()),
            Span::styled(
                state.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" · {took}"), theme::muted()),
        ],
    ]
    .concat();

    directory.rows.push(Row::plain(Line::from(head)));
    directory.rows.push(Row::plain(Line::from(bar.to_vec())));

    match state {
        "lost" | "stopped" => {
            sub_rows_lost_stopped(directory, text_value, sub, width, bar, state, color);
        }
        _ => {
            sub_rows_match(directory, text_value, key, fold, width, bar);
        }
    }
}

fn sub_rows_match(
    directory: &mut Drawn,
    text_value: &Sub,
    key: usize,
    fold: &Fold<'_>,
    width: usize,
    bar: &[Span<'static>],
) {
    let lines: Vec<&str> = text_value.result.trim_end().lines().collect();
    let open = fold.run(key);
    let long = lines.len() > 6;

    let text = if long && !open {
        lines[..6].join("\n")
    } else {
        text_value.result.trim_end().to_string()
    };

    directory.rows.extend(memo::markdown(&text, width, bar));

    if long {
        let at = directory.rows.len();

        let label = if open {
            "▾ fold the report".to_string()
        } else {
            format!("▸ {} more lines (click)", lines.len() - 6)
        };

        directory.rows.push(Row::plain(Line::from(
            [bar.to_vec(), vec![Span::styled(label, theme::muted())]].concat(),
        )));
        directory.hits.insert(at, Hit::Run(key));
    }
}

fn sub_rows_lost_stopped(
    directory: &mut Drawn,
    text_value: &Sub,
    sub: usize,
    width: usize,
    bar: &[Span<'static>],
    state: &str,
    color: Color,
) {
    let why = if state == "stopped" {
        "stopped by the owner".to_string()
    } else {
        format!("ended without a report: {}", text_value.result)
    };

    directory
        .rows
        .extend(markdown::plain(&why, width, Style::default().fg(color), bar));

    if state == "lost" {
        let at = directory.rows.len();
        let pre = [bar.to_vec(), vec![Span::styled("↻ ", theme::accent())]].concat();

        directory.rows.extend(markdown::plain(
            &format!(
                "pick up: {} goes on from this subagent's context (click)",
                text_value.master
            ),
            width,
            theme::accent(),
            &pre,
        ));

        for i in at..directory.rows.len() {
            directory.hits.insert(i, Hit::PickUp(sub));
        }
    }
}
/// Rows of the thoughts above a message (fold KEY), BAR before each: `▸ ∴ 1st line…` folded,
/// `▾ ∴ text` open (each thought wrapped). Clickable.
pub(super) fn thought_rows(
    directory: &mut Drawn,
    thoughts: &[String],
    key: usize,
    fold: &Fold,
    width_value: usize,
    bar: &[Span<'static>],
) {
    if thoughts.is_empty() || fold.thoughts == Thoughts::Hidden {
        return;
    }

    let st = theme::muted().add_modifier(Modifier::ITALIC);
    let open = fold.thought(key);
    let at = directory.rows.len();

    if open {
        thought_rows_mark(directory, thoughts, fold, width_value, bar, st);
    } else {
        let first = thoughts[0]
            .lines()
            .find(|line_value| !line_value.trim().is_empty())
            .unwrap_or_default();

        let more = if thoughts.len() > 1 {
            format!(" · {} thoughts", thoughts.len())
        } else {
            String::new()
        };

        let room =
            width_value.saturating_sub(bar.iter().map(|text| text.content.width()).sum::<usize>() + 4 + more.width());

        let cut: String = if first.width() > room {
            first.chars().take(room.saturating_sub(1)).collect::<String>() + "…"
        } else {
            first.to_string()
        };

        directory.rows.push(Row::plain(Line::from(
            [
                bar.to_vec(),
                vec![
                    Span::styled("▸ ∴ ", theme::muted()),
                    Span::styled(cut, st),
                    Span::styled(more, theme::muted()),
                ],
            ]
            .concat(),
        )));
    }

    if fold.thoughts != Thoughts::Open {
        for i in at..directory.rows.len() {
            directory.hits.insert(i, Hit::Thought(key));
        }
    }
}

fn thought_rows_mark(
    directory: &mut Drawn,
    thoughts: &[String],
    fold: &Fold<'_>,
    width_value: usize,
    bar: &[Span<'static>],
    st: Style,
) {
    for (i, thread) in thoughts.iter().enumerate() {
        let mark = if i == 0 && fold.thoughts != Thoughts::Open {
            "▾ ∴ "
        } else {
            "  ∴ "
        };

        let first = [bar.to_vec(), vec![Span::styled(mark, theme::muted())]].concat();
        let mut rows = markdown::plain(thread, width_value, st, &first);

        for row in rows.iter_mut().skip(1) {
            row.line.spans[bar.len()] = Span::raw("    ");
        }

        directory.rows.extend(rows);
    }
}
/// The thoughts of a run (open: interleaved with its calls; Open setting: also under a folded run).
pub(super) fn run_thought_rows(
    directory: &mut Drawn,
    thoughts: &[(usize, String)],
    before: usize,
    width: usize,
    bar: &[Span<'static>],
) {
    let st = theme::muted().add_modifier(Modifier::ITALIC);

    for (_, thread) in thoughts.iter().filter(|(count, _)| *count == before) {
        let first = [bar.to_vec(), vec![Span::styled("∴ ", theme::muted())]].concat();
        let rest = [bar.to_vec(), vec![Span::raw("  ")]].concat();
        let mut rows = markdown::plain(thread, width, st, &first);

        for row in rows.iter_mut().skip(1) {
            row.line.spans.splice(..=bar.len(), rest.clone());
        }

        directory.rows.extend(rows);
    }
}
/// The rows of open ask A (answered questions, the shown one, its rows: clickable, the hint),
/// BAR before each. ASK: the view's picker of A.
pub(super) fn ask_rows(directory: &mut Drawn, agent_value: &Ask, width: usize, bar: &[Span<'static>], ask: &Picker) {
    // an ask of a subagent: whose it is
    if let Some(sub) = agent_value.input["_subagent"].as_str() {
        directory.rows.extend(markdown::plain(
            &format!("from subagent “{sub}” of {}", agent_value.agent),
            width,
            theme::accent(),
            bar,
        ));
    }

    let qs = agent_value.questions();
    let multi = qs.get(ask.question).is_some_and(|query| query.3);

    if let Some((query_value, header, _, _)) = qs.get(ask.question) {
        // the questions before / after: answered ones with their answer
        ask_rows_answered(
            directory,
            agent_value,
            width,
            bar,
            ask,
            QuestionTabs {
                qs: &qs,
                query_value,
                header,
            },
        );
    } else {
        directory
            .rows
            .extend(markdown::plain(&agent_value.summary(), width, theme::bold(), bar));

        if let Some(cmd) = agent_value.input["command"].as_str() {
            directory.rows.extend(markdown::plain(
                &format!("$ {cmd}"),
                width,
                theme::fg(theme::pal().info()),
                bar,
            ));
        }

        if let Some(plan) = agent_value.input["plan"].as_str() {
            directory.rows.extend(markdown::markdown(plan, width, bar));
        }
    }

    for (i, row) in ask.rows(agent_value).into_iter().enumerate() {
        ask_rows_cur(directory, width, bar, ask, multi, i, &row);
    }

    directory
        .rows
        .extend(markdown::plain(&ask.hint(agent_value), width, theme::muted(), bar));
}

fn ask_rows_answered(
    directory: &mut Drawn,
    agent_value: &Ask,
    width: usize,
    bar: &[Span<'static>],
    ask: &Picker,
    context: QuestionTabs<'_>,
) {
    let QuestionTabs {
        qs,
        query_value,
        header,
    } = context;

    if qs.len() > 1 {
        ask_rows_answered_answered(directory, agent_value, width, bar, ask, qs);
    } else if !header.is_empty() {
        directory
            .rows
            .extend(markdown::plain(header, width, theme::muted(), bar));
    }

    directory
        .rows
        .extend(markdown::plain(query_value, width, theme::bold(), bar));
}

fn ask_rows_answered_answered(
    directory: &mut Drawn,
    agent_value: &Ask,
    width: usize,
    bar: &[Span<'static>],
    ask: &Picker,
    qs: &[super::ask::Question],
) {
    let answered = ask.answered(agent_value);

    let tabs: Vec<String> = qs
        .iter()
        .enumerate()
        .map(|(i, (qq, height, ..))| {
            let name = if height.is_empty() {
                format!("{}", i + 1)
            } else {
                height.clone()
            };

            let done = answered.iter().any(|(x, _)| x == qq);

            match (i == ask.question, done) {
                (true, _) => format!("[{name}]"),
                (false, true) => format!("{name} ✓"),
                (false, false) => name,
            }
        })
        .collect();

    directory.rows.extend(markdown::plain(
        &format!("{} · {}/{}", tabs.join(" · "), ask.question + 1, qs.len()),
        width,
        theme::muted(),
        bar,
    ));
}

fn ask_rows_cur(
    directory: &mut Drawn,
    width: usize,
    bar: &[Span<'static>],
    ask: &Picker,
    multi: bool,
    i: usize,
    row: &super::ask::Row,
) {
    let cur = row.cursor && ask.focus;

    // multi: a box per row; single: a dot on the answer given before (←)
    let mark = match (multi, row.picked) {
        (true, true) => "[x] ",
        (true, false) => "[ ] ",
        (false, true) => "● ",
        (false, false) => "",
    };

    let st = if cur { theme::accent_bold() } else { theme::text() };

    let line = if row.other {
        let text = if row.label.is_empty() && !cur {
            "type an answer".to_string()
        } else {
            row.label.clone()
        };

        format!("{mark}{}. Other: {text}{}", i + 1, if cur { "▏" } else { "" })
    } else {
        format!("{mark}{}. {}", i + 1, row.label)
    };

    let at = directory.rows.len();
    let pre = [
        bar.to_vec(),
        vec![Span::styled(if cur { "› " } else { "  " }, theme::accent())],
    ]
    .concat();

    let style = if row.other && row.label.is_empty() && !cur {
        theme::muted()
    } else {
        st
    };

    let mut rows = markdown::plain(&line, width, style, &pre);

    if !row.desc.is_empty() {
        rows.extend(markdown::plain(
            &format!("     {}", row.desc),
            width,
            theme::muted(),
            bar,
        ));
    }

    for key in 0..rows.len() {
        directory.hits.insert(at + key, Hit::Choice(i));
    }

    directory.rows.extend(rows);
}

#[derive(Clone, Copy)]
pub(super) struct QuestionTabs<'render> {
    pub(super) qs: &'render [super::ask::Question],
    pub(super) query_value: &'render str,
    pub(super) header: &'render str,
}
