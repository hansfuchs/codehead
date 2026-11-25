use super::*;

#[test]
fn args_per_editor_kind() {
    let wt = Path::new("/w");

    let strings = |args: Vec<OsString>| {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        strings(args_at(Kind::VsCode, wt, "src/a.rs", 12)),
        ["/w", "-g", "/w/src/a.rs:12"]
    );
    assert_eq!(
        strings(args_at(Kind::Zed, wt, "src/a.rs", 12)),
        ["/w", "/w/src/a.rs:12"]
    );
    assert_eq!(
        strings(args_at(Kind::JetBrains, wt, "src/a.rs", 12)),
        ["--line", "12", "/w/src/a.rs"]
    );
    assert_eq!(strings(args_dir(Kind::VsCode, wt)), ["/w"]);
    assert_eq!(
        strings(args_at(Kind::Terminal, wt, "src/a.rs", 12)),
        ["+12", "/w/src/a.rs"]
    );
    assert_eq!(kind_of(Path::new("/usr/local/bin/cursor")), Kind::VsCode);
    assert_eq!(kind_of(Path::new("rustrover")), Kind::JetBrains);
    assert_eq!(kind_of(Path::new("hx")), Kind::Other);
    assert_eq!(kind_of(Path::new("/usr/local/bin/flow")), Kind::Flow);
    assert_eq!(
        strings(args_at(Kind::Flow, wt, "src/a.rs", 12)),
        ["--gui", "--project", "/w", "/w/src/a.rs:12"]
    );
    assert!(!Kind::Flow.in_terminal() && Kind::Terminal.in_terminal());
}

#[test]
fn a_flow_instance_with_the_worktree_gets_the_file() {
    let list = r#"[{"name":"a","pid":1,"version":"v","socket":"/s/flow-1.sock","projects":["/other"]},{"name":"b","pid":2,"version":"v","socket":"/s/flow-2.sock","projects":["/w","/x"]}]"#;

    assert_eq!(flow_pick(list, Path::new("/w")).as_deref(), Some("/s/flow-2.sock"));

    let lines = "{\"name\":\"b\",\"socket\":\"/s/flow-2.sock\",\"projects\":[\"/w\"]}\n";

    assert_eq!(flow_pick(lines, Path::new("/w")).as_deref(), Some("/s/flow-2.sock"));
    assert_eq!(flow_pick(list, Path::new("/nope")), None);
    assert_eq!(flow_pick("", Path::new("/w")), None);
}
