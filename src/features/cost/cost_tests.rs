use super::*;

#[test]
fn plan_windows_get_short_names() {
    assert_eq!(window("five_hour"), "5h");
    assert_eq!(window("seven_day"), "7d");
    assert_eq!(window("seven_day_opus"), "7d opus");
    assert_eq!(window("other_window"), "other window");
}

#[test]
fn the_rate_comes_from_the_agents_last_paid_turn() {
    use crate::engine::store::{Event, Kind};

    let end = |cost: f64, input: u64| {
        Event::new(
            Some("pk"),
            Kind::TurnEnd {
                cost,
                input,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                ctx: 0,
                window: 0,
                ms: 0,
                error: None,
            },
        )
    };

    let evs = vec![
        end(1.0, 100),
        end(3.0, 100),
        end(0.0, 50),
        Event::new(
            Some("ada"),
            Kind::TurnEnd {
                cost: 9.0,
                input: 1,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                ctx: 0,
                window: 0,
                ms: 0,
                error: None,
            },
        ),
    ];

    assert_eq!(last_rate(&evs, "pk"), Some(0.03));
    assert_eq!(last_rate(&evs, "zed"), None);
}

#[test]
fn a_running_turn_counts_at_once() {
    let mut agent = AgentUse {
        input: 100,
        output: 50,
        usd: 1.0,
        usd_per_token: 0.01,
        ctx: Some((1000, 200_000)),
        turns: 1,
        ..Default::default()
    };
    let turn = crate::engine::harness::TurnUse {
        input: 10,
        output: 20,
        cache_read: 70,
        cache_write: 0,
        ctx: 5000,
    };

    add_running(&mut agent, &turn, "", 42);

    assert_eq!((agent.input, agent.output, agent.cache_read), (110, 70, 70));
    assert!((agent.usd - 2.0).abs() < 1e-9);
    assert_eq!(agent.ctx, Some((5000, 200_000)));
    assert_eq!(agent.samples, vec![(42, 30)]);
}

#[test]
fn humanizes() {
    assert_eq!(human(0), "·");
    assert_eq!(human(250), "250");
    assert_eq!(human(98_049), "98.0k");
    assert_eq!(human(12_812_803), "12.8M");
}

/// Live check: `CODEHEAD_THREAD`=<id> cargo test `live_fetch` -- --ignored --nocapture
#[test]
#[ignore = "live agent usage; explicit run required"]
fn live_fetch() {
    let id = std::env::var("CODEHEAD_THREAD").unwrap();
    let snap = fetch(&id, &names());
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 14)).unwrap();

    term.draw(|frame| draw(frame, &snap, Duration::from_secs(0), now()))
        .unwrap();
    let buffer = term.backend().buffer().clone();

    let text: String = (0..14)
        .map(|y| {
            (0..60)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
                + "\n"
        })
        .collect();

    println!("{text}");

    assert!(snap.found);
}

#[test]
fn renders() {
    let now = 1_000_000;
    let snap = render_snapshot(now);
    let (width, height) = (56, 16);
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    term.draw(|frame| draw(frame, &snap, Duration::from_secs(1), now))
        .unwrap();
    let buffer = term.backend().buffer().clone();

    let text: String = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
                + "\n"
        })
        .collect();

    println!("{text}");

    assert!(text.contains("$28.47 thread"));
    assert!(
        text.chars()
            .any(|character| ('\u{2801}'..='\u{28ff}').contains(&character)),
        "sprite row"
    );
    assert!(text.contains("$26.87"));
    assert!(text.contains("19%"));
    assert!(text.contains(" 13%  2h") && text.contains(" 95%  3d"), "{text}");

    // compact: the thread view's header, a 400-char message to amon
    assert_compact_cost(now, &snap);
}

fn assert_compact_cost(now: u64, snap: &Snap) {
    let (width, height) = (56, 7);
    let next = Next {
        to: vec!["amon".into()],
        chars: 400,
    };
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    term.draw(|frame| {
        let area = frame.area();

        draw_at(frame.buffer_mut(), area, snap, Duration::from_secs(1), now, &next);
    })
    .unwrap();

    let buffer = term.backend().buffer().clone();

    let text: String = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
                + "\n"
        })
        .collect();

    println!("{text}");
    let first = text.lines().next().unwrap();

    assert!(first.contains("tok") && first.ends_with("$28.47 thread"), "{text}");
    assert!(text.contains("$26.87") && !text.contains("9 turns"), "{text}");
    assert!(!text.contains("q close") && !text.contains("◆"), "{text}");

    // amon's context line: ━ in amon's color up to 19 %, ─ rest, the % after it
    assert_compact_context(width, &buffer, &text);

    // the window bars: used ━ in text (7d: error from 90 %), the next message's share ━ in info, ─ rest
    assert_compact_limits(width, &buffer, &text);

    // the next message: ≥ context + text tokens, $, share per window (0 rate: ?)
    assert!(text.contains("≥38.1k tok ~$0.38 · 5h +0.2% · 7d ?"), "{text}");
}

fn assert_compact_context(width: u16, buffer: &Buffer, text: &String) {
    let row = text
        .lines()
        .position(|line| line.starts_with(" amon"))
        .expect("amon row");
    let cells: Vec<_> = (0..width).map(|x| buffer[(x, row as u16)].clone()).collect();
    let amon = agents::color(Some("orange"));
    let fill = cells
        .iter()
        .filter(|cell| cell.symbol() == "━" && cell.fg == amon)
        .count();
    let rest = cells.iter().filter(|cell| cell.symbol() == "─").count();

    assert!(fill > 0 && rest > fill * 3, "{text}");
    assert!(text.lines().nth(row).unwrap().ends_with(" 19%"), "{text}");

    // lilith: 0 context yet, all ─
    assert!(
        text.lines().any(|line| line.starts_with(" lilith")
            && line.contains('─')
            && !line.contains('━')
            && line.ends_with('·')),
        "{text}"
    );
}

fn assert_compact_limits(width: u16, buffer: &ratatui::buffer::Buffer, text: &String) {
    let row = text.lines().position(|line| line.starts_with(" 5h")).expect("5h row");
    let cells: Vec<_> = (0..width).map(|x| buffer[(x, row as u16)].clone()).collect();

    assert!(
        cells
            .iter()
            .any(|cell| cell.symbol() == "━" && cell.fg == theme::pal().text),
        "used part: {text}"
    );
    assert!(
        cells
            .iter()
            .any(|cell| cell.symbol() == "━" && cell.fg == theme::pal().info()),
        "share: {text}"
    );
    assert!(
        cells
            .iter()
            .any(|cell| cell.symbol() == "━" && cell.fg == theme::pal().error()),
        "7d near the limit: {text}"
    );
    assert!(cells.iter().any(|cell| cell.symbol() == "─"), "rest: {text}");
    assert!(!text.contains('░') && !text.contains('▒'), "{text}");

    // the windows side by side: 5h and 7d on 1 row
    assert!(text.lines().nth(row).unwrap().contains(" 7d "), "{text}");
}

fn render_snapshot(now: u64) -> Snap {
    Snap {
        found: true,
        status: "working · turn 3".into(),
        cost: 28.47,
        input: 250,
        output: 98_049,
        cache_read: 12_812_803,
        cache_write: 534_032,
        turns: 9,
        agents: vec![
            AgentRow {
                name: "amon".into(),
                color: Some("orange".into()),
                tokens: 98_000,
                usd: 26.87,
                ctx: Some(19),
                live: true,
                sprite: Some(crate::domain::sprites::default_for("amon")),
                ctx_tokens: 38_000,
                usd_per_token: 0.000_01,
            },
            AgentRow {
                name: "lilith".into(),
                tokens: 92,
                usd: 1.6,
                ..Default::default()
            },
        ],
        limits: vec![
            Limit {
                harness: "claude".into(),
                label: "five_hour".into(),
                pct: 13.0,
                resets: Some(now + 7200),
                usd_per_pct: Some(2.0),
            },
            Limit {
                harness: "claude".into(),
                label: "seven_day".into(),
                pct: 95.0,
                resets: Some(now + 3 * 86_400),
                usd_per_pct: None,
            },
        ],
    }
}

#[test]
fn a_morph_moves_only_the_changed_cells() {
    let area = Rect::new(0, 0, 8, 1);
    let mut from = Buffer::empty(area);

    from.set_string(0, 0, "ab 12%", Style::default());
    let mut to = Buffer::empty(area);

    to.set_string(0, 0, "ab 47%", Style::default());
    let row = |buffer: &Buffer| (0..8).map(|x| buffer[(x, 0)].symbol().to_string()).collect::<String>();
    let at = |secs: f32| Morph {
        from: from.clone(),
        to: to.clone(),
        start: Instant::now().checked_sub(Duration::from_secs_f32(secs)).unwrap(),
    };

    // start: the old box; the end: the new one
    assert_eq!(row(&at(0.0).shown()), row(&from));
    assert_eq!(row(&at(1.0).shown()), row(&to));

    // midway: equal cells untouched, changed ones old, noise or new
    let mid = row(&at(MORPH_TIME.as_secs_f32() * 0.5).shown());

    assert!(mid.starts_with("ab ") && mid.ends_with("%  "), "{mid}");

    // bar cells dither: each one old or new, 0 noise
    let mut fb = Buffer::empty(area);

    fb.set_string(0, 0, "░░░░░░░░", Style::default());
    let mut tb = Buffer::empty(area);

    tb.set_string(0, 0, "▒▒▒▒▒▒▒▒", Style::default());
    let bar_morph = Morph {
        from: fb,
        to: tb,
        start: Instant::now().checked_sub(MORPH_TIME / 2).unwrap(),
    };

    assert!(
        row(&bar_morph.shown())
            .chars()
            .all(|character| character == '░' || character == '▒')
    );
}
