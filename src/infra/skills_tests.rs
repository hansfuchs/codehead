use super::*;

#[test]
fn front_matter_values() {
    let (count, directory) = front_matter(
        "---\nname: tmux\ndescription: Use the tmux CLI: tabs, panes.\nmetadata:\n  x: \"2\"\n---\nbody\nname: no",
    );

    assert_eq!(
        (count.as_deref(), directory.as_deref()),
        (Some("tmux"), Some("Use the tmux CLI: tabs, panes."))
    );

    let (count_value, directory_value) =
        front_matter("---\nname: \"a-b\"\ndescription: >\n  line 1\n  line 2\nother: x\n---\n");

    assert_eq!(
        (count_value.as_deref(), directory_value.as_deref()),
        (Some("a-b"), Some("line 1 line 2"))
    );
    assert_eq!(front_matter("# no front matter"), (None, None));
}

#[test]
fn skills_of_the_harnesses() {
    let home = std::env::temp_dir().join(format!("codehead-skills-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);

    let put = |rel: &str, text: &str| {
        let path = home.join(rel);

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };

    put(
        ".claude/skills/tmux/SKILL.md",
        "---\nname: tmux\ndescription: tmux cli\n---\n",
    );
    put(".claude/skills/bare/SKILL.md", "no front matter");
    put(
        ".claude/plugins/synced/x/general/.claude-plugin/plugin.json",
        "{\"name\":\"general\"}",
    );
    put(
        ".claude/plugins/synced/x/general/skills/antislop/SKILL.md",
        "---\nname: antislop\ndescription: fix slop\n---\n",
    );
    put(
        ".claude/plugins/synced/x/general~g2/.claude-plugin/plugin.json",
        "{\"name\":\"general\"}",
    );
    put(
        ".claude/plugins/synced/x/general~g2/skills/antislop/SKILL.md",
        "---\nname: antislop\ndescription: fix slop\n---\n",
    );

    // a marketplace catalog: not installed, not listed
    check_harness_skills(&home, put);
}

fn check_harness_skills(home: &Path, put: impl Fn(&str, &str)) {
    put(
        ".claude/plugins/marketplaces/m/p/skills/nope/SKILL.md",
        "---\nname: nope\n---\n",
    );
    put(
        ".codex/skills/.system/imagegen/SKILL.md",
        "---\nname: imagegen\ndescription: images\n---\n",
    );
    put("proj/.git/HEAD", "");
    put(
        "proj/.claude/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: ship it\n---\n",
    );
    let cwd = home.join("proj/sub");

    std::fs::create_dir_all(&cwd).unwrap();

    let names = |height: &[&str]| {
        load_in(
            home,
            &height.iter().map(std::string::ToString::to_string).collect::<Vec<_>>(),
            &cwd,
        )
        .into_iter()
        .map(|text_value| format!("{}/{}", text_value.harness, text_value.name))
        .collect::<Vec<_>>()
    };

    assert_eq!(
        names(&["claude"]),
        ["claude/bare", "claude/deploy", "claude/general:antislop", "claude/tmux"]
    );
    assert_eq!(names(&["codex", "claude"]).len(), 5);
    assert_eq!(names(&["codex"]), ["codex/imagegen"]);
    assert_eq!(names(&["pi"]), [] as [std::string::String; 0]);

    let _ = std::fs::remove_dir_all(home);
}
