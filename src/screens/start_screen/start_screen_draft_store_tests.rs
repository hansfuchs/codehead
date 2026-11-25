use super::*;

#[test]
fn save_load_remove() {
    let dir = std::env::temp_dir().join(format!("codehead-drafts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let worktree = Draft {
        id: "a".into(),
        title: "fix the build".into(),
        prompt: "- step 1\n  - nested".into(),
        notes: vec!["x/y.md".into()],
        picks: vec!["amon".into()],
        place_choice: "NewWorktree".into(),
        checkout: true,
        files: vec![("[Image #1]".into(), "/tmp/i.png".into())],
        carry: vec![(Some("ab12".into()), "/t.jsonl".into(), "old thread".into())],
        ..Default::default()
    };

    save(&dir, &worktree).unwrap();
    save(
        &dir,
        &Draft {
            id: "b".into(),
            prompt: "2nd".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // an empty draft: never listed
    save(
        &dir,
        &Draft {
            id: "c".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let all = load(&dir, "b");

    assert_eq!(all.len(), 1);
    assert!(all[0].saved > 0);
    assert_eq!(
        Draft {
            saved: 0,
            ..all[0].clone()
        },
        worktree
    );
    assert_eq!(all[0].name(), "fix the build");
    assert_eq!(
        load(&dir, "")[..]
            .iter()
            .map(super::Draft::name)
            .collect::<Vec<_>>()
            .len(),
        2
    );

    remove(&dir, "a");

    assert_eq!(load(&dir, "b"), [] as [crate::screens::start_screen::drafts::Draft; 0]);

    let _ = std::fs::remove_dir_all(&dir);
}
