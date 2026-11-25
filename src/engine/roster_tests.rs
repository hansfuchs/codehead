use super::*;

#[test]
fn names_and_modes() {
    assert!(valid_name("ada"));
    assert!(valid_name("r2-d2_x"));
    assert!(!valid_name("2pac"));
    assert!(!valid_name("a/b"));
    assert!(!valid_name(""));
    assert_eq!(permission_mode("autopilot"), "auto");
    assert_eq!(permission_mode("supervised"), "default");
}

#[test]
fn toml_keeps_unset_keys_out() {
    let agent = Agent {
        name: "ada".into(),
        harness: "claude".into(),
        mode: "autopilot".into(),
        model: Some("opus".into()),
        ..Default::default()
    };
    let text = toml::to_string_pretty(&agent).unwrap();

    assert!(text.contains("model = \"opus\""));
    assert!(!text.contains("effort"));
    assert!(!text.contains("archived"));

    let back: Agent = toml::from_str(&text).unwrap();

    assert_eq!(back.model.as_deref(), Some("opus"));
}
