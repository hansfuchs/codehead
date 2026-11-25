use super::*;

#[test]
fn registry_and_commands() {
    assert_eq!(ALL.len(), 13);
    assert!(acp_command(get("claude").unwrap(), None).is_none());

    // codex: the app server, an ACP agent only on an own command
    assert!(acp_command(get("codex").unwrap(), None).is_none());
    assert_eq!(
        acp_command(get("codex").unwrap(), Some("codex-acp")).unwrap(),
        ["codex-acp"]
    );
    assert_eq!(
        acp_command(get("grok").unwrap(), None).unwrap(),
        ["grok", "agent", "stdio"]
    );
    assert_eq!(
        acp_command(get("pi").unwrap(), Some("npx 'pi-acp' --x")).unwrap(),
        ["npx", "pi-acp", "--x"]
    );
    assert_eq!(split(r#"a "b c" ''"#), ["a", "b c", ""]);
}
