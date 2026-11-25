#[test]
fn git_push_finds_a_push_in_any_part_of_the_command() {
    assert!(super::git_push("git push"));
    assert!(super::git_push("cargo test && git -C ../x push -u origin HEAD"));
    assert!(super::git_push("GIT_TRACE=1 /usr/bin/git push"));
    assert!(super::git_push("git -c a=b push; echo ok"));
    assert!(!super::git_push("git status"));
    assert!(!super::git_push("echo git push"));
    assert!(!super::git_push("git log --grep push"));
}
