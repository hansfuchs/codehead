use super::*;

/// Temp files of atomic writes and editor scratch files: transient; real files: not.
#[test]
fn transient_names() {
    let temps = [
        "src/view.rs.tmp.48213.0123456789ab",
        "src/view.rs.tmp.0a1b2c3d",
        "4913",
        "src/4913",
        "notes.md~",
        "src/.view.rs.swp",
        "src/.view.rs.swx",
        ".#view.rs",
        "src/#view.rs#",
    ];
    let real = [
        "src/view.rs",
        "src/view.tmp.rs",
        "src/view.rs.tmp.48213.xyz",
        "src/view.rs.tmp.0a1b2c3",
        "src/49130",
        "src/swap.swp",
        "#",
        "src/.env",
    ];

    let wrong: Vec<&str> = temps
        .iter()
        .filter(|path| !transient(path))
        .chain(real.iter().filter(|path| transient(path)))
        .copied()
        .collect();

    assert!(wrong.is_empty(), "misjudged: {wrong:?}");
}

/// A dir git listed with a trailing `/`, a dir with a `.git` entry: nested; a file: not.
#[test]
fn nested_repository_entries() {
    let top = std::env::temp_dir().join(format!("ch-stray-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);

    std::fs::create_dir_all(top.join("vendor/lib")).unwrap();
    std::fs::write(top.join("vendor/lib/.git"), "gitdir: /elsewhere\n").unwrap();
    std::fs::write(top.join("a.txt"), "a\n").unwrap();

    let verdicts = (
        nested_repository(&top, ".claude/worktrees/agent-1/"),
        nested_repository(&top, "vendor/lib"),
        nested_repository(&top, "a.txt"),
    );

    assert_eq!(verdicts, (true, true, false));

    std::fs::remove_dir_all(&top).unwrap();
}
