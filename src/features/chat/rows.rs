use super::{
    Ask, Color, Drawn, Fold, Hit, Line, Modifier, Picker, Row, Span, Style, Sub, Thoughts, UnicodeWidthStr, markdown,
    memo, theme,
};

/// Rows of the report of subagent SUB (a message of its master), BAR before each: `↳ subagent
/// “what” · done · 2m10s`, then the report (folded to 6 lines, KEY opens it) or why it ended (lost:
/// a `pick up` row: the master goes on from the subagent's context).
pub(super) fn sub_rows(
    drawn: &mut Drawn,
    subagent: &Sub,
    sub: usize,
    key: usize,
    fold: &Fold,
    width: usize,
    bar: &[Span<'static>],
) {
    let pal = theme::pal();
    let state = subagent.state.as_deref().unwrap_or("running");

    let color = match state {
        "done" => pal.success(),
        "stopped" => pal.subtext0,
        _ => pal.error(),
    };

    let secs = subagent.end.saturating_sub(subagent.start) / 1000;

    let took = if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };

    let head = [
        bar.to_vec(),
        vec![
            Span::styled("↳ subagent ", theme::accent()),
            Span::styled(format!("“{}”", subagent.description), theme::bold()),
            Span::styled(if subagent.resumed { " · resumed · " } else { " · " }, theme::muted()),
            Span::styled(
                state.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" · {took}"), theme::muted()),
        ],
    ]
    .concat();

    drawn.rows.push(Row::plain(Line::from(head)));
    drawn.rows.push(Row::plain(Line::from(bar.to_vec())));

    match state {
        "lost" | "stopped" => {
            lost_rows(drawn, subagent, sub, width, bar, state, color);
        }
        _ => {
            report_rows(drawn, subagent, key, fold, width, bar);
        }
    }
}

fn report_rows(drawn: &mut Drawn, subagent: &Sub, key: usize, fold: &Fold<'_>, width: usize, bar: &[Span<'static>]) {
    let lines: Vec<&str> = subagent.result.trim_end().lines().collect();
    let open = fold.run(key);
    let long = lines.len() > 6;

    let text = if long && !open {
        lines[..6].join("\n")
    } else {
        subagent.result.trim_end().to_string()
    };

    drawn.rows.extend(memo::markdown(&text, width, bar));

    if long {
        let at = drawn.rows.len();

        let label = if open {
            "▾ fold the report".to_string()
        } else {
            format!("▸ {} more lines (click)", lines.len() - 6)
        };

        drawn.rows.push(Row::plain(Line::from(
            [bar.to_vec(), vec![Span::styled(label, theme::muted())]].concat(),
        )));
        drawn.hits.insert(at, Hit::Run(key));
    }
}

fn lost_rows(
    drawn: &mut Drawn,
    subagent: &Sub,
    sub: usize,
    width: usize,
    bar: &[Span<'static>],
    state: &str,
    color: Color,
) {
    let why = if state == "stopped" {
        "stopped by the owner".to_string()
    } else {
        format!("ended without a report: {}", subagent.result)
    };

    drawn
        .rows
        .extend(markdown::plain(&why, width, Style::default().fg(color), bar));

    if state == "lost" {
        let at = drawn.rows.len();
        let pre = [bar.to_vec(), vec![Span::styled("↻ ", theme::accent())]].concat();

        drawn.rows.extend(markdown::plain(
            &format!(
                "pick up: {} goes on from this subagent's context (click)",
                subagent.master
            ),
            width,
            theme::accent(),
            &pre,
        ));

        for i in at..drawn.rows.len() {
            drawn.hits.insert(i, Hit::PickUp(sub));
        }
    }
}
/// Rows of the thoughts above a message (fold KEY), BAR before each: `▸ ∴ 1st line…` folded,
/// `▾ ∴ text` open (each thought wrapped). Clickable.
pub(super) fn thought_rows(
    drawn: &mut Drawn,
    thoughts: &[String],
    key: usize,
    fold: &Fold,
    width: usize,
    bar: &[Span<'static>],
) {
    if thoughts.is_empty() || fold.thoughts == Thoughts::Hidden {
        return;
    }

    let italic = theme::muted().add_modifier(Modifier::ITALIC);
    let open = fold.thought(key);
    let at = drawn.rows.len();

    if open {
        open_thought_rows(drawn, thoughts, fold, width, bar, italic);
    } else {
        let first = thoughts[0]
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default();

        let more = if thoughts.len() > 1 {
            format!(" · {} thoughts", thoughts.len())
        } else {
            String::new()
        };

        let room = width.saturating_sub(bar.iter().map(|span| span.content.width()).sum::<usize>() + 4 + more.width());

        let cut: String = if first.width() > room {
            first.chars().take(room.saturating_sub(1)).collect::<String>() + "…"
        } else {
            first.to_string()
        };

        drawn.rows.push(Row::plain(Line::from(
            [
                bar.to_vec(),
                vec![
                    Span::styled("▸ ∴ ", theme::muted()),
                    Span::styled(cut, italic),
                    Span::styled(more, theme::muted()),
                ],
            ]
            .concat(),
        )));
    }

    if fold.thoughts != Thoughts::Open {
        for i in at..drawn.rows.len() {
            drawn.hits.insert(i, Hit::Thought(key));
        }
    }
}

fn open_thought_rows(
    drawn: &mut Drawn,
    thoughts: &[String],
    fold: &Fold<'_>,
    width: usize,
    bar: &[Span<'static>],
    italic: Style,
) {
    for (i, thought) in thoughts.iter().enumerate() {
        let mark = if i == 0 && fold.thoughts != Thoughts::Open {
            "▾ ∴ "
        } else {
            "  ∴ "
        };

        let first = [bar.to_vec(), vec![Span::styled(mark, theme::muted())]].concat();
        let mut rows = markdown::plain(thought, width, italic, &first);

        for row in rows.iter_mut().skip(1) {
            row.line.spans[bar.len()] = Span::raw("    ");
        }

        drawn.rows.extend(rows);
    }
}
/// The thoughts of a run (open: interleaved with its calls; Open setting: also under a folded run).
pub(super) fn run_thought_rows(
    drawn: &mut Drawn,
    thoughts: &[(usize, String)],
    before: usize,
    width: usize,
    bar: &[Span<'static>],
) {
    let italic = theme::muted().add_modifier(Modifier::ITALIC);

    for (_, thought) in thoughts.iter().filter(|(calls_before, _)| *calls_before == before) {
        let first = [bar.to_vec(), vec![Span::styled("∴ ", theme::muted())]].concat();
        let rest = [bar.to_vec(), vec![Span::raw("  ")]].concat();
        let mut rows = markdown::plain(thought, width, italic, &first);

        for row in rows.iter_mut().skip(1) {
            row.line.spans.splice(..=bar.len(), rest.clone());
        }

        drawn.rows.extend(rows);
    }
}
/// The rows of open ask A (answered questions, the shown one, its rows: clickable, the hint),
/// BAR before each. ASK: the view's picker of A.
pub(super) fn ask_rows(drawn: &mut Drawn, open_ask: &Ask, width: usize, bar: &[Span<'static>], ask: &Picker) {
    // an ask of a subagent: whose it is
    if let Some(sub) = open_ask.input["_subagent"].as_str() {
        drawn.rows.extend(markdown::plain(
            &format!("from subagent “{sub}” of {}", open_ask.agent),
            width,
            theme::accent(),
            bar,
        ));
    }

    let questions = open_ask.questions();
    let multi = questions.get(ask.question).is_some_and(|question| question.3);

    if let Some((question, header, _, _)) = questions.get(ask.question) {
        // the questions before / after: answered ones with their answer
        question_rows(
            drawn,
            open_ask,
            width,
            bar,
            ask,
            QuestionTabs {
                questions: &questions,
                question,
                header,
            },
        );
    } else {
        drawn
            .rows
            .extend(markdown::plain(&open_ask.summary(), width, theme::bold(), bar));

        if let Some(cmd) = open_ask.input["command"].as_str() {
            drawn.rows.extend(markdown::plain(
                &format!("$ {cmd}"),
                width,
                theme::fg(theme::pal().info()),
                bar,
            ));
        }

        if let Some(plan) = open_ask.input["plan"].as_str() {
            drawn.rows.extend(markdown::markdown(plan, width, bar));
        }
    }

    for (i, row) in ask.rows(open_ask).into_iter().enumerate() {
        choice_rows(drawn, width, bar, ask, multi, i, &row);
    }

    drawn
        .rows
        .extend(markdown::plain(&ask.hint(open_ask), width, theme::muted(), bar));
}

fn question_rows(
    drawn: &mut Drawn,
    open_ask: &Ask,
    width: usize,
    bar: &[Span<'static>],
    ask: &Picker,
    context: QuestionTabs<'_>,
) {
    let QuestionTabs {
        questions,
        question,
        header,
    } = context;

    if questions.len() > 1 {
        question_tabs_row(drawn, open_ask, width, bar, ask, questions);
    } else if !header.is_empty() {
        drawn.rows.extend(markdown::plain(header, width, theme::muted(), bar));
    }

    drawn.rows.extend(markdown::plain(question, width, theme::bold(), bar));
}

fn question_tabs_row(
    drawn: &mut Drawn,
    open_ask: &Ask,
    width: usize,
    bar: &[Span<'static>],
    ask: &Picker,
    questions: &[super::ask::Question],
) {
    let answered = ask.answered(open_ask);

    let tabs: Vec<String> = questions
        .iter()
        .enumerate()
        .map(|(i, (question, header, ..))| {
            let name = if header.is_empty() {
                format!("{}", i + 1)
            } else {
                header.clone()
            };

            let done = answered.iter().any(|(asked, _)| asked == question);

            match (i == ask.question, done) {
                (true, _) => format!("[{name}]"),
                (false, true) => format!("{name} ✓"),
                (false, false) => name,
            }
        })
        .collect();

    drawn.rows.extend(markdown::plain(
        &format!("{} · {}/{}", tabs.join(" · "), ask.question + 1, questions.len()),
        width,
        theme::muted(),
        bar,
    ));
}

fn choice_rows(
    drawn: &mut Drawn,
    width: usize,
    bar: &[Span<'static>],
    ask: &Picker,
    multi: bool,
    i: usize,
    row: &super::ask::Row,
) {
    let focused = row.cursor && ask.focus;

    // multi: a box per row; single: a dot on the answer given before (←)
    let mark = match (multi, row.picked) {
        (true, true) => "[x] ",
        (true, false) => "[ ] ",
        (false, true) => "● ",
        (false, false) => "",
    };

    let label_style = if focused { theme::accent_bold() } else { theme::text() };

    let line = if row.other {
        let text = if row.label.is_empty() && !focused {
            "type an answer".to_string()
        } else {
            row.label.clone()
        };

        format!("{mark}{}. Other: {text}{}", i + 1, if focused { "▏" } else { "" })
    } else {
        format!("{mark}{}. {}", i + 1, row.label)
    };

    let at = drawn.rows.len();
    let pre = [
        bar.to_vec(),
        vec![Span::styled(if focused { "› " } else { "  " }, theme::accent())],
    ]
    .concat();

    let style = if row.other && row.label.is_empty() && !focused {
        theme::muted()
    } else {
        label_style
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

    for offset in 0..rows.len() {
        drawn.hits.insert(at + offset, Hit::Choice(i));
    }

    drawn.rows.extend(rows);
}

#[derive(Clone, Copy)]
pub(super) struct QuestionTabs<'render> {
    pub(super) questions: &'render [super::ask::Question],
    pub(super) question: &'render str,
    pub(super) header: &'render str,
}
