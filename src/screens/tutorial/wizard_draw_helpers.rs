const CW: usize = 22;
use super::{HOME_CHOICES, HomeChoice, Line, Modifier, PRESETS, Span, Status, Style, Wizard, avatar, harnesses, theme};

impl Wizard {
    pub(super) fn draw_harness(&self, lines: &mut Vec<Line<'static>>) {
        draw_harness_options(self, lines);
    }

    pub(super) fn draw_home(&self, lines: &mut Vec<Line<'static>>) {
        draw_home_options(self, lines);
    }

    pub(super) fn draw_crew(&self, lines: &mut Vec<Line<'static>>) {
        draw_crew_options(self, lines);
    }

    pub(super) fn draw_try(&self, lines: &mut Vec<Line<'static>>, _w: u16) {
        let what = if self.harness_state.note.is_empty() {
            "the tutorial ended before its last step."
        } else {
            "the tutorial did not start."
        };

        lines.push(Line::from(Span::styled(format!("  {what}"), theme::muted())));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  enter  ", theme::bold()),
            Span::styled("again", theme::text()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  →      ", theme::bold()),
            Span::styled("skip it", theme::text()),
        ]));
    }

    pub(super) fn draw_next(&self, lines: &mut Vec<Line<'static>>) {
        let lead = self.lead();

        for (key, what, hint) in [
            ("n ", "start 1st real thread", format!("{lead} takes it")),
            ("F3", "your home", "notes + plans land here".to_string()),
            ("? ", "hints again", "1 line per concept".to_string()),
        ] {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key}  "), theme::bold()),
                Span::styled(format!("{what:<24}"), theme::text()),
                Span::styled(hint, theme::muted()),
            ]));
        }
    }
}

fn draw_harness_options(wizard: &Wizard, lines: &mut Vec<Line<'static>>) {
    for (i, harness) in wizard.harness_rows().iter().enumerate() {
        draw_harness_option(wizard, lines, i, harness);
    }
}

fn draw_harness_option(wizard: &Wizard, lines: &mut Vec<Line<'static>>, i: usize, harness: &harnesses::Harness) {
    let ok = wizard.harness_state.installed.contains(&harness.name);
    let cur = i == wizard.harness_state.hcursor;

    let mark = if ok {
        Span::styled("  ✓ ", Style::default().fg(theme::pal().success()))
    } else {
        Span::styled("  ✗ ", theme::muted())
    };

    let label = Span::styled(
        format!("{:<14}", harness.label),
        if cur { theme::accent_bold() } else { theme::text() },
    );

    let what = if ok {
        Span::styled("ready", theme::muted())
    } else {
        Span::styled(
            harness.install.to_string(),
            if cur { theme::text() } else { theme::muted() },
        )
    };

    lines.push(Line::from(vec![
        Span::styled(if cur { "›" } else { " " }, theme::accent()),
        mark,
        label,
        what,
    ]));
}

fn draw_home_options(wizard: &Wizard, lines: &mut Vec<Line<'static>>) {
    for (i, (choice, name, what)) in HOME_CHOICES.iter().enumerate() {
        draw_home_option(wizard, lines, i, *choice, name, what);
    }

    lines.push(Line::from(""));
    let choice = HOME_CHOICES[wizard.home_state.home].0;

    if choice == HomeChoice::Skip {
        lines.push(Line::from(Span::styled(
            "    agents + settings stay in ~/.config/codehead",
            theme::muted(),
        )));
        return;
    }

    lines.push(Line::from(Span::styled("    codehead writes:", theme::muted())));

    for (path, what) in [
        ("codehead/agents/", "your crew"),
        ("codehead/settings.toml", "every device"),
        ("codehead/devices/", "1 file per device"),
        ("notes/", "plans + reports of agents"),
    ] {
        lines.push(Line::from(vec![
            Span::styled(format!("      {path:<24}"), theme::text()),
            Span::styled(what, theme::muted()),
        ]));
    }

    lines.push(Line::from(Span::styled(
        "    commits + syncs by itself.",
        theme::muted(),
    )));
}

fn draw_home_option(
    wizard: &Wizard,
    lines: &mut Vec<Line<'static>>,
    i: usize,
    choice: HomeChoice,
    name: &&str,
    what: &&str,
) {
    let cur = i == wizard.home_state.home;

    lines.push(Line::from(vec![
        Span::styled(if cur { "  › " } else { "    " }, theme::accent()),
        Span::styled(
            format!("{name:<10}"),
            if cur { theme::accent_bold() } else { theme::text() },
        ),
        Span::styled(what.to_string(), theme::muted()),
    ]));

    if cur && choice != HomeChoice::Skip {
        let hint = match choice {
            HomeChoice::Clone => "git URL",
            _ => "path",
        };

        let text = wizard.home_state.paths[i].clone();
        let caret = Span::styled("▏", theme::accent());

        let row = if text.is_empty() {
            vec![caret, Span::styled(hint, theme::muted())]
        } else {
            vec![Span::styled(text, theme::text()), caret]
        };

        lines.push(Line::from([vec![Span::raw("              ")], row].concat()));
    }
}

fn draw_crew_options(wizard: &Wizard, lines: &mut Vec<Line<'static>>) {
    // presets row
    let mut row = vec![Span::raw("  ")];

    for (i, (name, _)) in PRESETS.iter().enumerate() {
        let st = if i == wizard.crew_state.preset {
            theme::pill()
        } else {
            theme::muted()
        };

        row.push(Span::styled(format!(" {name} "), st));
        row.push(Span::raw("  "));
    }

    lines.push(Line::from(row));
    lines.push(Line::from(""));

    // the agents side by side: 1 column of 20 per agent
    let canvases: Vec<[String; avatar::SPAWN_H]> = wizard
        .crew_state
        .crew
        .iter()
        .map(|member| Wizard::canvas(member, Status::Idle))
        .collect();

    for row in 0..avatar::SPAWN_H {
        draw_crew_sprite_row(wizard, lines, &canvases, row);
    }

    let cell = |text: String, st: Style| vec![Span::styled(format!("{text:<CW$}"), st)];
    let mut names = vec![Span::raw("  ")];
    let mut roles = vec![Span::raw("  ")];
    let mut hs = vec![Span::raw("  ")];

    for (i, member) in wizard.crew_state.crew.iter().enumerate() {
        let focused = i == wizard.crew_state.focus;
        let style = Style::default()
            .fg(crate::domain::agents::color(Some(member.color)))
            .add_modifier(Modifier::BOLD);

        let name = if focused {
            format!("{}▏", member.name)
        } else {
            member.name.clone()
        };

        let mark = if focused { "› " } else { "  " };

        names.extend(cell(
            format!("  {mark}{name}"),
            if focused {
                style
            } else {
                style.remove_modifier(Modifier::BOLD)
            },
        ));
        roles.extend(cell(format!("    {}", member.role.name()), theme::text()));
        let model = crate::infra::config::harness_opt(member.harness, "model").unwrap_or_else(|| "default".into());

        hs.extend(cell(format!("    {} · {model}", member.harness), theme::muted()));
    }

    draw_crew_description(wizard, lines, names, roles, hs);
}

fn draw_crew_description(
    wizard: &Wizard,
    lines: &mut Vec<Line<'static>>,
    names: Vec<Span<'static>>,
    roles: Vec<Span<'static>>,
    hs: Vec<Span<'static>>,
) {
    lines.push(Line::from(names));
    lines.push(Line::from(roles));
    lines.push(Line::from(hs));
    lines.push(Line::from(""));

    let teach = match PRESETS[wizard.crew_state.preset].0 {
        "solo" => "1 agent does it all.".to_string(),
        "pair" => format!(
            "{} writes, {} checks. they talk on their own.",
            wizard.crew_state.crew[0].name, wizard.crew_state.crew[1].name
        ),
        _ => format!(
            "{} plans, {} writes, {} checks.",
            wizard.crew_state.crew[0].name, wizard.crew_state.crew[1].name, wizard.crew_state.crew[2].name
        ),
    };

    lines.push(Line::from(Span::styled(format!("  {teach}"), theme::muted())));
}

fn draw_crew_sprite_row(
    wizard: &Wizard,
    lines: &mut Vec<Line<'static>>,
    canvases: &[[String; avatar::SPAWN_H]],
    row: usize,
) {
    let mut spans = vec![Span::raw("  ")];

    for (i, member) in wizard.crew_state.crew.iter().enumerate() {
        let style = Style::default().fg(crate::domain::agents::color(Some(member.color)));

        spans.push(Span::styled(format!("   {}", canvases[i][row]), style));
        spans.push(Span::raw(" ".repeat(CW - 3 - avatar::SPAWN_W)));
    }

    lines.push(Line::from(spans));
}
