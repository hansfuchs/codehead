use super::*;

#[test]
fn written_by_us_marks() {
    let raw = "<!-- codehead:style mecha -->\n<!-- codehead:end -->";

    assert!(written_by_us(raw));
    assert_eq!(parse_profile(Some(raw), None).style.as_deref(), Some("mecha"));
    assert!(!written_by_us("plain prompt"));
}

#[test]
fn full_model_names() {
    assert_eq!(model_name("claude-opus-5-5"), "Opus 5.5");
    assert_eq!(model_name("opus"), "Opus 5.5");
    assert_eq!(model_name("opus[1m]"), "Opus 5.5 (1M)");
    assert_eq!(model_name("claude-opus-4-1-20250805"), "Opus 4.1");
    assert_eq!(model_name("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
    assert_eq!(model_name("gpt-5.5"), "gpt-5.5");
}

use crate::infra::styles::Style;

fn styles() -> Vec<Style> {
    vec![Style {
        id: "mecha".into(),
        name: "MECHA".into(),
        body: "BODY".into(),
    }]
}

#[test]
fn compose_parse_round_trip() {
    let sprite = crate::domain::sprites::from_id("regi:7 eyes=3 speed=0.5");
    let profile = Profile {
        style: Some("mecha".into()),
        voice: "dry.\ncalls the owner boss.".into(),
        prompt: "you review rust.".into(),
        sprite,
    };

    for (harness, global, embedded) in [
        ("claude", Some("MECHA"), false),
        ("claude", None, true),
        ("codex", Some("MECHA"), true),
    ] {
        let raw = compose(&profile, &styles(), harness, global);

        assert_eq!(raw.contains("BODY"), embedded, "{harness} {global:?}");
        assert!(raw.contains("<!-- codehead:handoff -->") && raw.contains("needs harness: <harness> (<tool>)"));
        assert_eq!(parse_profile(Some(&raw), None), profile);
    }
}

#[test]
fn foreign_prompt_is_system_prompt() {
    let profile = parse_profile(Some("be brief"), Some("mecha"));

    assert_eq!(
        profile,
        Profile {
            style: Some("mecha".into()),
            prompt: "be brief".into(),
            ..Profile::default()
        }
    );
    assert_eq!(compose(&Profile::default(), &styles(), "claude", None), "");
}

#[test]
fn effort_keeps_other_keys() {
    let path = std::env::temp_dir().join(format!("codehead-settings-{}.json", std::process::id()));

    std::fs::write(&path, "{\n  \"zeta\": 1,\n  \"alpha\": \"x\"\n}\n").unwrap();
    set_effort_in(&path, Some("high")).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();

    assert!(
        text.find("zeta").unwrap() < text.find("alpha").unwrap()
            && text.find("alpha").unwrap() < text.find("effortLevel").unwrap(),
        "{text}"
    );

    set_effort_in(&path, None).unwrap();

    assert!(!std::fs::read_to_string(&path).unwrap().contains("effortLevel"));

    std::fs::write(&path, "not json").unwrap();

    assert!(set_effort_in(&path, Some("low")).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");

    let _ = std::fs::remove_file(&path);
}
