use super::*;

#[test]
fn detail_lines_show_thread_agents_threads_and_limits() {
    let directory = Detail {
        agents: vec![AgentUse {
            agent: "pk".into(),
            harness: "claude".into(),
            sessions: 2,
            live: true,
            input: 1000,
            output: 500,
            cache_read: 9000,
            turns: 3,
            usd: 1.5,
            ctx: Some((50, 200)),
            ..Default::default()
        }],
        threads: vec![("t1".into(), 1.5, 1500, 3), ("t2".into(), 0.5, 100, 1)],
        all_usd: 2.0,
        all_tokens: 1600,
        day_usd: 1.0,
        limits: vec![("claude".into(), "five_hour".into(), 42.0, None)],
    };
    let names = HashMap::from([("pk".to_string(), ("amon".to_string(), None))]);
    let titles = HashMap::from([("t1".to_string(), "codehead v2".to_string())]);
    let text: Vec<String> = detail_lines(&directory, Some("t1"), &names, &titles, 70)
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let all = text.join("\n");

    for want in [
        "codehead v2",
        "$1.50 · 1.5k tokens",
        "hit 90%",
        "3 turns",
        "amon",
        "25%",
        "2 session(s) · live",
        "all threads",
        "› ",
        "claude 5h",
        "42%",
    ] {
        assert!(all.contains(want), "{want}: {all}");
    }

    let none: String = detail_lines(&directory, None, &names, &titles, 70)
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(none.contains("0 thread in view") && !none.contains("agents"));
}
