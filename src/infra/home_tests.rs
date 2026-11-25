use super::*;

#[test]
fn parse_reads_name_path_and_url_per_line() {
    let text = "# x\n\n## Other repos\n\n### doinstruct\n\n- platform: core apps. `~/projects/doinstruct/platform`. `git@github.com:doinstruct/platform.git`. Notes: `notes/platform/`.\n- security: GHAS replacement. `/abs/security`. `https://github.com/doinstruct/security.git`.\n- 0 path: `notes/x`\n- line with no colon `~/x`\n- dup: again. `~/projects/doinstruct/platform`.\n";
    let result = parse(text);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "platform");
    assert!(result[0].path.ends_with("projects/doinstruct/platform") && result[0].path.is_absolute());
    assert_eq!(result[0].url.as_deref(), Some("git@github.com:doinstruct/platform.git"));
    assert_eq!(
        (result[1].path.to_str(), result[1].url.as_deref()),
        (
            Some("/abs/security"),
            Some("https://github.com/doinstruct/security.git")
        )
    );
}
