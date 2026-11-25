use super::{HOME_CHOICES, HomeChoice, Line, Member, SCREENS, Screen, Span, Status, Wizard, avatar, theme};

impl Wizard {
    pub(super) fn header(&self, width: u16) -> Line<'static> {
        header_line(self, width)
    }
    pub(super) fn title(&self) -> (&'static str, String) {
        match SCREENS[self.screen] {
            Screen::Harness => ("harnesses", "agents run on a harness. 1 needed.".into()),
            Screen::Home => (
                "home repo",
                "1 git repo: your crew, notes, settings. synced to every device.".into(),
            ),
            Screen::Crew => (
                "your crew",
                "agents = named teammates. @name calls 1 into a thread.".into(),
            ),
            Screen::Try => ("try it", "a real thread with your crew. it goes away after.".into()),
            Screen::Next => ("ready", "your crew waits on the start screen.".into()),
        }
    }
    pub(super) fn keys(&self) -> Vec<(&'static str, &'static str)> {
        let esc = if self.history.is_empty() {
            ("esc", "quit")
        } else {
            ("esc", "back")
        };

        let mut value = match SCREENS[self.screen] {
            Screen::Harness => vec![
                ("↑↓", "pick"),
                ("c", "copy install"),
                ("r", "check again"),
                ("enter", "next"),
            ],
            Screen::Home if HOME_CHOICES[self.home_state.home].0 == HomeChoice::Skip => {
                vec![("↑↓", "pick"), ("enter", "next")]
            }
            Screen::Home => vec![("↑↓", "pick"), ("type", "path"), ("enter", "next")],
            Screen::Crew => vec![
                ("←→", "preset"),
                ("↑↓", "agent"),
                ("type", "name"),
                ("space", "avatar"),
                ("enter", "next"),
            ],
            Screen::Try => vec![("enter", "again"), ("→", "skip")],
            Screen::Next => vec![("enter", "start screen")],
        };

        value.insert(0, esc);
        value
    }
    /// The 13x6 canvas of member M: the spawn while it runs, then the live sprite at `SPAWN_AT`.
    pub(super) fn canvas(message: &Member, status: Status) -> [String; avatar::SPAWN_H] {
        avatar_canvas(message, status)
    }
    /// What a real run does, 1 line each.
    pub(crate) fn plan(&self) -> Vec<String> {
        let mut out = Vec::new();
        let path = self.home_state.paths[self.home_state.home].clone();

        out.push(match HOME_CHOICES[self.home_state.home].0 {
            HomeChoice::New => format!(
                "home: new {path} (git init, codehead home init, gh repo create --private when gh is logged in)"
            ),
            HomeChoice::Clone => format!("home: git clone {path}, codehead home init, this device's file"),
            HomeChoice::Existing => format!("home: {path} (codehead home init: agents, flows, settings move in)"),
            HomeChoice::Skip => "home: 0 (agents + settings in ~/.config/codehead)".into(),
        });

        let crew: Vec<String> = self
            .crew_state
            .crew
            .iter()
            .map(|message| {
                format!(
                    "{} ({}, {}, {})",
                    message.name,
                    message.role.name(),
                    message.harness,
                    message.avatar.id()
                )
            })
            .collect();

        out.push(format!("agents: {} → <home>/codehead/agents/", crew.join(", ")));
        out.push("tutorial: sandbox repo <data>/tutorial, 1 ephemeral thread, deleted at the end".into());

        out
    }
}

fn header_line(widget_state: &Wizard, width: u16) -> Line<'static> {
    let mut spans = vec![Span::styled("  codehead", theme::bold())];

    if widget_state.dry {
        spans.push(Span::styled("  dry run", theme::muted()));
    }

    let used: usize = spans.iter().map(|text_value| text_value.content.chars().count()).sum();
    let pad = (width as usize).saturating_sub(used + SCREENS.len() * 2 + 1);

    spans.push(Span::raw(" ".repeat(pad)));

    for i in 0..SCREENS.len() {
        let st = match i.cmp(&widget_state.screen) {
            std::cmp::Ordering::Equal => theme::accent(),
            std::cmp::Ordering::Less => theme::text(),
            std::cmp::Ordering::Greater => theme::muted(),
        };

        spans.push(Span::styled(if i <= widget_state.screen { "● " } else { "○ " }, st));
    }

    Line::from(spans)
}

fn avatar_canvas(message: &Member, status: Status) -> [String; avatar::SPAWN_H] {
    if let Some(rows) = message.spawn.rows(&message.avatar, status) {
        return rows;
    }

    let live = message.avatar.rows(status, avatar::now());

    std::array::from_fn(|result| {
        let blank = " ".repeat(avatar::SPAWN_W);

        if result >= avatar::SPAWN_AT.1 && result < avatar::SPAWN_AT.1 + avatar::HEIGHT {
            format!(
                "{}{}{}",
                " ".repeat(avatar::SPAWN_AT.0),
                live[result - avatar::SPAWN_AT.1],
                " ".repeat(avatar::SPAWN_W - avatar::SPAWN_AT.0 - avatar::WIDTH)
            )
        } else {
            blank
        }
    })
}
