use super::*;

fn agent(name: &str, harness: &str) -> Agent {
    Agent {
        name: name.into(),
        harness: harness.into(),
        model: None,
        color: None,
        mode: String::new(),
        prompt: None,
        last_seen: None,
    }
}

#[test]
fn grouped_by_harness_a_to_z() {
    let agents = [
        agent("nova", "codex"),
        agent("Amon", "claude"),
        agent("lilith", "claude"),
    ];
    let all = groups(&agents, |_| true);

    assert_eq!(all, vec![("claude".into(), vec![1, 2]), ("codex".into(), vec![0])]);

    let filtered = groups(&agents, |agent| matches(agent, "cod"));

    assert_eq!(filtered, vec![("codex".into(), vec![0])]);

    // 2 columns of 20 in 41: claude in 1 row, codex below
    let wide = grid(&groups(&agents, |_| true), 20, CARD_H, 41);

    assert_eq!(wide.heads, vec![(0, "claude · 2".into()), (8, "codex · 1".into())]);
    assert_eq!(wide.places, vec![(1, 0, 1), (2, 21, 1), (0, 0, 9)]);
    assert_eq!(wide.height, 15);

    // tight: the next heading right below the cards
    let tight = tight_grid(&groups(&agents, |_| true), 20, CARD_H, 41);

    assert_eq!(tight.heads, vec![(0, "claude · 2".into()), (7, "codex · 1".into())]);
    assert_eq!(tight.places, vec![(1, 0, 1), (2, 21, 1), (0, 0, 8)]);
    assert_eq!(tight.height, 14);
}
