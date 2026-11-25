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
    let agent_value = [
        agent("nova", "codex"),
        agent("Amon", "claude"),
        agent("lilith", "claude"),
    ];
    let group = groups(&agent_value, |_| true);

    assert_eq!(group, vec![("claude".into(), vec![1, 2]), ("codex".into(), vec![0])]);

    let group_value = groups(&agent_value, |x| matches(x, "cod"));

    assert_eq!(group_value, vec![("codex".into(), vec![0])]);

    // 2 columns of 20 in 41: claude in 1 row, codex below
    let line = grid(&groups(&agent_value, |_| true), 20, CARD_H, 41);

    assert_eq!(line.heads, vec![(0, "claude · 2".into()), (8, "codex · 1".into())]);
    assert_eq!(line.places, vec![(1, 0, 1), (2, 21, 1), (0, 0, 9)]);
    assert_eq!(line.height, 15);

    // tight: the next heading right below the cards
    let line_value = tight_grid(&groups(&agent_value, |_| true), 20, CARD_H, 41);

    assert_eq!(
        line_value.heads,
        vec![(0, "claude · 2".into()), (7, "codex · 1".into())]
    );
    assert_eq!(line_value.places, vec![(1, 0, 1), (2, 21, 1), (0, 0, 8)]);
    assert_eq!(line_value.height, 14);
}
