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
fn a_1st_turn_takes_the_window_of_the_agents_last_turn_elsewhere() {
    use crate::engine::store::{Event, Kind};

    let end = |agent: &str, window: u64| {
        Event::new(
            Some(agent),
            Kind::TurnEnd {
                cost: 0.0,
                input: 1,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                ctx: 10,
                window,
                ms: 0,
                error: None,
            },
        )
    };

    let events = vec![end("regi", 1_000_000), end("regi", 0), end("gigas", 475_000)];
    let mut agent = AgentUse::default();
    let turn = crate::engine::harness::TurnUse {
        ctx: 172_200,
        ..Default::default()
    };

    add_running(&mut agent, &turn, last_window(&events, "regi").unwrap(), 1);

    // 172.2k of 1M: 17 %, never 86 % of a 200k guess
    assert_eq!(agent.ctx, Some((172_200, 1_000_000)));
    assert_eq!(last_window(&events, "nova"), None);
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

    add_running(&mut agent, &turn, 1_000_000, 42);

    // the thread's own window wins over the fallback
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

#[test]
fn a_codex_thread_shows_0_claude_window() {
    let codex_only = render_snapshot(100).agents.split_off(1);
    let limits = render_snapshot(100).limits;

    let kept = of_harnesses(limits, &codex_only);

    assert!(kept.iter().all(|limit| limit.harness == "codex"), "{kept:?}");
    assert_eq!(kept.len(), 1);
}

/// Live check: `CODEHEAD_THREAD`=<id> cargo test `live_fetch` -- --ignored --nocapture
#[test]
#[ignore = "live agent usage; explicit run required"]
fn live_fetch() {
    let id = std::env::var("CODEHEAD_THREAD").unwrap();
    let snap = fetch(&id, &names());
    let next = Next::default();
    let show = Show {
        next: &next,
        expanded: Some(Expanded { all: None, scroll: 0 }),
    };
    let (buffer, text) = render(&snap, show, 76, 40, now());

    println!("{text}");

    assert!(snap.found && buffer.area.width == 76);
}

/// SNAP drawn WIDTH x HEIGHT at NOW: the buffer, its rows as text (trailing spaces cut).
fn render(snap: &Snap, show: Show, width: u16, height: u16, now: u64) -> (Buffer, String) {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);

    draw_at(&mut buffer, area, snap, now, show);

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

    (buffer, text)
}

/// The column of the 1st cell of ROW with a background (a bar's used part) or `░`.
fn bar_start(buffer: &Buffer, row: usize) -> Option<u16> {
    (0..buffer.area.width).find(|x| {
        let cell = &buffer[(*x, row as u16)];

        cell.symbol() == "░" || cell.symbol() == "▒" || cell.bg != ratatui::style::Color::Reset
    })
}

#[test]
fn the_compact_box_groups_agents_by_harness_with_aligned_dithered_bars() {
    let now = 1_000_000;
    let snap = render_snapshot(now);
    let next = Next {
        to: vec!["amon".into()],
        chars: 400,
    };
    let show = Show {
        next: &next,
        expanded: None,
    };

    let (buffer, text) = render(&snap, show, 60, 4, now);

    println!("{text}");
    let rows: Vec<&str> = text.lines().collect();

    // claude: 5h + 7d side by side, amon below; codex: its window, gigas below
    assert!(
        rows[0].starts_with("claude") && rows[0].contains("5h") && rows[0].contains("7d"),
        "{text}"
    );
    assert!(rows[1].starts_with("amon") && rows[1].ends_with(" 19%"), "{text}");
    assert!(rows[2].starts_with("codex") && rows[2].contains(" 94%"), "{text}");
    assert!(rows[3].starts_with("gigas") && rows[3].ends_with('·'), "{text}");

    // every 1st bar starts in 1 column
    let starts: Vec<Option<u16>> = (0..3).map(|row| bar_start(&buffer, row)).collect();

    assert!(
        starts.iter().all(|start| *start == starts[0] && start.is_some()),
        "{starts:?}\n{text}"
    );

    // dithered: used on a background, rest ░, the next message's share ▒ (5h has a rate)
    assert!(rows[0].contains('▒') && rows[0].contains('░'), "{text}");

    let amon = agents::color(Some("orange"));

    assert!(
        (0..60).any(|x| buffer[(x, 1)].bg == amon),
        "amon's context on amon's color: {text}"
    );

    // 7d ≥ 90 %: error
    assert!((0..60).any(|x| buffer[(x, 0)].bg == theme::pal().error()), "{text}");

    // 0 "all", 0 "next", 0 "working"
    assert!(
        !text.contains("all") && !text.contains("next") && !text.contains("working"),
        "{text}"
    );
}

#[test]
fn the_expanded_box_shows_the_details_over_the_compact_rows() {
    let now = 1_000_000;
    let snap = render_snapshot(now);
    let next = Next {
        to: vec!["amon".into()],
        chars: 400,
    };
    let all = AllThreads {
        threads: vec![ThreadCost {
            id: "t1".into(),
            title: "codehead v2".into(),
            usd: 28.47,
            tokens: 98_299,
            turns: 9,
        }],
        usd: 40.0,
        tokens: 200_000,
        day_usd: 12.5,
    };
    let show = Show {
        next: &next,
        expanded: Some(Expanded {
            all: Some(&all),
            scroll: 0,
        }),
    };

    let (_, text) = render(&snap, show, 62, 40, now);

    println!("{text}");

    for want in [
        "codehead v2",
        "$28.47 · 98.2k tok",
        "cache 12.8M read · 534.0k write · hit 99%",
        "last hour",
        "cache r",
        "1 session · context 38.0k of 200.0k (19%)",
        "resets in 2h · next +0.2% · $2.00 per 1%",
        "@amon  ≥38.1k tok ~$0.38",
        "all threads",
        "$40.00 · 200.0k tok · last 24h $12.50",
        "› ",
    ] {
        assert!(text.contains(want), "{want}:\n{text}");
    }

    // the compact rows stay at the bottom
    let rows: Vec<&str> = text.lines().collect();

    assert!(
        rows[rows.len() - 1].starts_with("gigas") && rows[rows.len() - 4].starts_with("claude"),
        "{text}"
    );
}

fn render_snapshot(now: u64) -> Snap {
    Snap {
        found: true,
        id: "t1".into(),
        title: "codehead v2".into(),
        cost: 28.47,
        agents: vec![
            AgentRow {
                name: "amon".into(),
                color: Some("orange".into()),
                ctx: Some(19),
                live: true,
                ctx_tokens: 38_000,
                usage: AgentUse {
                    agent: "amon".into(),
                    harness: "claude".into(),
                    sessions: 1,
                    live: true,
                    input: 250,
                    output: 98_000,
                    cache_read: 12_812_803,
                    cache_write: 534_032,
                    turns: 9,
                    usd: 28.47,
                    ctx: Some((38_000, 200_000)),
                    samples: vec![(now - 60, 5000)],
                    usd_per_token: 0.000_01,
                },
            },
            AgentRow {
                name: "gigas".into(),
                usage: AgentUse {
                    agent: "gigas".into(),
                    harness: "codex".into(),
                    output: 49,
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
        limits: render_limits(now),
    }
}

/// claude 5h (a rate) + 7d near the limit, codex 5h.
fn render_limits(now: u64) -> Vec<Limit> {
    vec![
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
        Limit {
            harness: "codex".into(),
            label: "five_hour".into(),
            pct: 94.0,
            resets: Some(now + 7200),
            usd_per_pct: None,
        },
    ]
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
