use crate::engine::store::{Event, Kind};
use serde_json::json;
use std::fmt::Write as _;

pub(super) const RUST: &str = include_str!("fixtures/highlight.rs.txt");

pub(super) fn markdown(blocks: usize) -> String {
    let mut text = String::new();

    for i in 0..blocks {
        let _ = write!(
            text,
            "## step {i}\n\nRead `src/app.rs` and **fixed** the [link](https://codehead.sh) for @ada.\n\n\
             - item one with *emphasis*\n- item two with `code`\n\n\
             | col | val |\n|---|---|\n| a | {i} |\n\n```rust\nfn f{i}(x: u32) -> u32 {{ x + {i} }}\n```\n\n"
        );
    }

    text
}

/// N turns: prompt, thought, 3 tool calls + results, text.
pub(super) fn events(turns: usize) -> Vec<Event> {
    let base = 1_795_090_200_000u64;
    let mut events = Vec::new();
    let ev = |ts: u64, agent: Option<&str>, kind: Kind| Event {
        ts,
        agent: agent.map(Into::into),
        kind,
    };

    for i in 0..turns {
        let ts = base + i as u64 * 60_000;

        events.push(ev(
            ts,
            None,
            Kind::Prompt {
                from: "owner".into(),
                to: vec!["ada".into()],
                text: format!("turn {i}: fix the build"),
            },
        ));
        events.push(ev(ts + 1, Some("ada"), Kind::TurnStart));
        events.push(ev(
            ts + 2,
            Some("ada"),
            Kind::Thought {
                text: "check the failing test first".into(),
            },
        ));

        for call in 0..3 {
            let id = format!("t{i}-{call}");

            events.push(ev(
                ts + 3,
                Some("ada"),
                Kind::Tool {
                    id: id.clone(),
                    name: "Bash".into(),
                    input: json!({"command": "cargo test", "description": "run tests"}),
                },
            ));
            events.push(ev(
                ts + 4,
                Some("ada"),
                Kind::ToolResult {
                    id,
                    ok: true,
                    output: "test result: ok. 120 passed\n".repeat(20),
                },
            ));
        }

        events.push(ev(ts + 5, Some("ada"), Kind::Text { text: markdown(1) }));
    }

    events
}

/// N files, each 4 hunks of 40 changed lines.
pub(super) fn diff(files: usize) -> String {
    let mut text = String::new();

    for file in 0..files {
        let _ = write!(
            text,
            "diff --git a/src/f{file}.rs b/src/f{file}.rs\nindex 1111111..2222222 100644\n--- a/src/f{file}.rs\n+++ b/src/f{file}.rs\n"
        );

        diff_hunks(&mut text);
    }

    text
}

fn diff_hunks(text: &mut String) {
    for hunk in 0..4 {
        let _ = writeln!(text, "@@ -{0},40 +{0},40 @@ fn h{hunk}()", hunk * 100 + 1);

        for line in 0..20 {
            let _ = write!(
                text,
                "-    let old_{line} = compute({line}, \"old\");\n+    let new_{line} = compute({line}, \"new\");\n"
            );
        }
    }
}

/// A repo on main with an origin (bare, local: 0 network), 50 files, an ignored `.env`: ROOT
/// holds `repo/` and `origin.git/`.
pub(super) fn repo_with_origin(root: &std::path::Path) -> std::path::PathBuf {
    let _ = std::fs::remove_dir_all(root);
    let repo = root.join("repo");

    std::fs::create_dir_all(repo.join("src")).unwrap();

    for i in 0..50 {
        std::fs::write(repo.join(format!("src/f{i}.rs")), format!("fn f{i}() {{}}\n")).unwrap();
    }

    std::fs::write(repo.join(".gitignore"), ".env*\n").unwrap();
    std::fs::write(repo.join(".env"), "KEY=1\n").unwrap();

    let sh = |dir: &std::path::Path, script: &str| {
        let ok = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();

        assert!(ok, "{script}");
    };

    sh(root, "git init -q --bare origin.git");
    sh(
        &repo,
        "git init -q -b main && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 0 && git add -A && git -c user.email=t@t -c user.name=t commit -qm init && git remote add origin ../origin.git && git push -q -u origin main && git remote set-head origin main",
    );

    repo
}

pub(super) fn paths(count: usize) -> Vec<String> {
    let dirs = [
        "src",
        "src/engine",
        "src/changes",
        "site/src/content/docs",
        "vendor/hyperpolyglot/src",
    ];

    (0..count)
        .map(|i| format!("{}/module_{i}/file_{}.rs", dirs[i % dirs.len()], i * 7 % 101))
        .collect()
}
