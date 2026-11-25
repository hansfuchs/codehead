use super::fixture::*;
use super::*;

fn base(path: &Path) -> String {
    oid(path, "origin/main")
}

#[test]
fn list_puts_unpushed_commits_first_then_pushed_ones() {
    let width = repo("list");

    let cs = list(&width, &base(&width));

    let subjects: Vec<(&str, bool)> = cs
        .list
        .iter()
        .map(|character| (character.subject.as_str(), character.pushed))
        .collect();

    assert_eq!(
        subjects,
        vec![
            ("c4 subject", false),
            ("c3 subject", false),
            ("c2 subject", true),
            ("c1 subject", true)
        ]
    );
    assert_eq!(cs.upstream.as_deref(), Some("origin/feat"));
    assert_eq!(cs.head, oid(&width, "HEAD"));
    assert!(cs.list[0].body.contains("c4 body") && !cs.more && cs.remote_only == 0);

    // 0 upstream: every commit unpushed; HEAD as base: 0 commit
    sh(
        &width,
        "git branch --unset-upstream && git push -q origin --delete feat",
    );
    let cs = list(&width, &base(&width));

    assert!(
        cs.upstream.is_none() && cs.list.iter().all(|character_value| !character_value.pushed) && cs.list.len() == 4
    );
    assert!(list(&width, "HEAD").list.len() == 5, "HEAD: the newest commits, 0 base");

    drop(&width);
}

#[test]
fn uncommit_of_the_top_commit_keeps_the_files_and_the_staged_changes() {
    let width = repo("top");

    sh(&width, "echo staged > y.txt && git add y.txt && echo dirty >> x.txt");
    let (head, c4) = (oid(&width, "HEAD"), oid(&width, "HEAD"));
    let before = out(&width, "cat x.txt y.txt c3.txt c4.txt");

    let user = uncommit(&width, std::slice::from_ref(&c4), &head, "t1").unwrap();

    assert_eq!(user.count, 1);
    assert_eq!(oid(&width, "HEAD"), oid(&width, &format!("{c4}^")), "HEAD: c3");
    assert_eq!(
        out(&width, "cat x.txt y.txt c3.txt c4.txt"),
        before,
        "the worktree as before"
    );
    assert_eq!(
        out(&width, "git diff --cached --name-only"),
        "y.txt",
        "the staged change stays staged; c4 unstaged"
    );
    assert_eq!(out(&width, "git status --porcelain c4.txt"), "?? c4.txt");
    assert_eq!(oid(&width, &backup_ref("t1")), head, "the old HEAD in the backup ref");

    drop(&width);
}

#[test]
fn uncommit_of_a_middle_commit_cascades_a_conflicting_later_change() {
    let width = repo("mid");

    // c5 changes x.txt line 1, c6 changes the same line again and adds z.txt
    sh(
        &width,
        "sed -i.bak '1s/.*/A/' x.txt && rm x.txt.bak && git commit -qam c5",
    );
    sh(
        &width,
        "sed -i.bak '1s/.*/B/' x.txt && rm x.txt.bak && echo z > z.txt && git add -A && git commit -qm c6",
    );
    let (head, c5) = (oid(&width, "HEAD"), oid(&width, "HEAD^"));
    let before = out(&width, "cat x.txt z.txt");

    let user = uncommit(&width, std::slice::from_ref(&c5), &head, "t2").unwrap();

    assert_eq!(out(&width, "cat x.txt z.txt"), before, "the worktree as before");
    assert_eq!(user.cascaded, vec![(head[..7].to_string(), "x.txt".to_string())]);
    assert_eq!(out(&width, "git log -1 --format=%s"), "c6", "c6 stays, with z.txt only");
    assert_eq!(out(&width, "git show --format= --name-only HEAD"), "z.txt");
    assert_eq!(out(&width, "git diff --name-only"), "x.txt", "x.txt: uncommitted");
    assert_eq!(out(&width, "git log --format=%s HEAD~1 -1"), "c4 subject");

    drop(&width);
}

#[test]
fn uncommit_refuses_pushed_commits_a_moved_head_and_merges() {
    let width = repo("refuse");
    let head = oid(&width, "HEAD");
    let c2 = oid(&width, "origin/feat");

    assert!(uncommit(&width, &[c2], &head, "t3").unwrap_err().contains("pushed"));
    assert!(
        uncommit(&width, std::slice::from_ref(&head), "0000000", "t3")
            .unwrap_err()
            .contains("HEAD moved")
    );

    sh(
        &width,
        "git switch -q -c side HEAD~1 && echo s > s.txt && git add -A && git commit -qm side && git switch -q feat && git merge -q --no-edit side",
    );
    let head = oid(&width, "HEAD");
    let c3 = oid(&width, "HEAD~1~1");

    assert!(
        uncommit(&width, &[c3], &head, "t3")
            .unwrap_err()
            .contains("merge commit")
    );
    assert_eq!(oid(&width, "HEAD"), head, "0 change");

    drop(&width);
}

#[test]
fn push_to_pushes_the_commit_and_the_older_unpushed_ones_only() {
    let width = repo("push");
    let c3 = oid(&width, "HEAD^");

    push_to(&width, &c3).unwrap();

    assert_eq!(oid(&width, "origin/feat"), c3);

    let cs = list(&width, &base(&width));

    assert_eq!(
        cs.list.iter().filter(|character| !character.pushed).count(),
        1,
        "c4 stays local"
    );

    drop(&width);
}

#[test]
fn a_branch_that_tracks_main_pushes_to_its_own_name_never_to_main() {
    let width = repo("tracks-main");

    sh(&width, "git branch -q -u origin/main");
    let (main, c3) = (oid(&width, "origin/main"), oid(&width, "HEAD^"));

    assert_eq!(
        upstream(&width).as_deref(),
        Some("origin/feat"),
        "the same name, not @{{u}}"
    );

    let cs = list(&width, &base(&width));

    assert_eq!(
        cs.list.iter().filter(|character| character.pushed).count(),
        2,
        "c1, c2 on origin/feat: pushed"
    );

    push_to(&width, &c3).unwrap();

    assert_eq!(oid(&width, "origin/main"), main, "main untouched");
    assert_eq!(oid(&width, "origin/feat"), c3);
    assert_eq!(
        out(&width, "git rev-parse --abbrev-ref @{u}"),
        "origin/feat",
        "tracks its own name now"
    );

    drop(&width);
}

#[test]
fn push_to_without_an_upstream_makes_the_pushed_branch_the_upstream() {
    let width = repo("no-upstream");

    sh(
        &width,
        "git branch -q --unset-upstream && git push -q origin --delete feat && git update-ref -d refs/remotes/origin/feat",
    );
    let c3 = oid(&width, "HEAD^");

    assert!(upstream(&width).is_none());

    push_to(&width, &c3).unwrap();

    assert_eq!(oid(&width, "origin/feat"), c3);
    assert_eq!(out(&width, "git rev-parse --abbrev-ref @{u}"), "origin/feat");

    drop(&width);
}

#[test]
fn a_staged_change_on_top_of_a_picked_commit_keeps_the_index() {
    let width = repo("index-kept");

    // c4.txt: added by c4, a staged change on top of it
    sh(&width, "echo more >> c4.txt && git add c4.txt");
    let head = oid(&width, "HEAD");
    let before = out(&width, "cat c4.txt");

    let user = uncommit(&width, &[head.clone(), head.clone()], &head, "t4").unwrap();

    assert_eq!(user.count, 1, "1 commit, picked twice");
    assert!(user.index_kept && user.note().contains("index kept"), "{}", user.note());
    assert_eq!(oid(&width, "HEAD"), oid(&width, &format!("{head}^")));
    assert_eq!(out(&width, "cat c4.txt"), before, "the worktree as before");
    assert_eq!(
        out(&width, "git diff --cached --name-only"),
        "c4.txt",
        "the old index: c4.txt staged as a whole"
    );

    drop(&width);
}

#[test]
fn uncommit_in_a_sparse_checkout_keeps_the_files_outside_the_cone() {
    let width = repo("sparse");

    sh(
        &width,
        "mkdir -p a b && echo a > a/x && echo b > b/y && git add -A && git commit -qm ab && echo c5 > a/c5 && git add -A && git commit -qm c5",
    );
    sh(&width, "git sparse-checkout set --no-cone a");
    let head = oid(&width, "HEAD");

    uncommit(&width, std::slice::from_ref(&head), &head, "t5").unwrap();

    let status = out(&width, "git status --porcelain");

    assert!(!status.contains(" D "), "0 deletions outside the cone: {status}");
    assert_eq!(status, "?? a/c5");

    drop(&width);
}

#[test]
fn show_signature_in_the_config_breaks_neither_the_list_nor_a_rewrite() {
    let width = repo("signature");

    // signed commits: `No signature` on stdout before each record (unsigned ones: 0 line)
    sh(
        &width,
        "ssh-keygen -q -t ed25519 -N '' -f ../k && git config gpg.format ssh && git config user.signingkey \"$PWD/../k\"",
    );
    sh(
        &width,
        "git commit -q --amend -S --no-edit && git config log.showSignature true && echo c5 > c5.txt && git add -A && git commit -q -S -m c5",
    );

    assert!(
        out(&width, "git log -1 --format=%H").starts_with("No signature"),
        "the config bites"
    );

    let (head, c4) = (oid(&width, "HEAD"), oid(&width, "HEAD^"));

    let cs = list(&width, &base(&width));

    assert_eq!(cs.list[0].oid, head);

    uncommit(&width, std::slice::from_ref(&c4), &head, "t6").unwrap();

    assert_eq!(
        out(&width, "git -c log.showSignature=false log -1 --format=%an%n%B"),
        "t\nc5"
    );
    assert_eq!(
        out(
            &width,
            "git cat-file commit HEAD | tail -c 4 | od -c | head -1 | tr -s ' '"
        ),
        out(
            &width,
            &format!("git cat-file commit {head} | tail -c 4 | od -c | head -1 | tr -s ' '")
        ),
        "1 trailing line break, as before"
    );

    drop(&width);
}

#[test]
fn push_to_refuses_a_commit_that_left_the_branch_and_another_name_upstream() {
    let width = repo("stale");
    let c4 = oid(&width, "HEAD");

    sh(&width, "git commit -q --amend -m 'c4 amended'");
    let feat = oid(&width, "origin/feat");

    assert!(push_to(&width, &c4).unwrap_err().contains("0 more on the branch"));
    assert_eq!(oid(&width, "origin/feat"), feat, "0 push");

    // fix tracks origin/hans/fix: 0 upstream of the same name
    sh(
        &width,
        "git push -q origin HEAD:refs/heads/hans/fix && git fetch -q && git switch -q -c fix && git branch -q -u origin/hans/fix",
    );

    assert!(upstream(&width).is_none(), "{:?}", upstream(&width));

    drop(&width);
}

#[test]
fn uncommit_refuses_the_root_commit() {
    let width = repo("root");

    sh(&width, "git branch -q --unset-upstream");
    let (head, root) = (oid(&width, "HEAD"), out(&width, "git rev-list --max-parents=0 HEAD"));

    assert!(
        uncommit(&width, &[root], &head, "t7").unwrap_err().contains("pushed"),
        "on origin/feat: pushed first"
    );

    sh(
        &width,
        "git push -q origin --delete feat && git update-ref -d refs/remotes/origin/feat",
    );
    let root = out(&width, "git rev-list --max-parents=0 HEAD");

    assert!(
        uncommit(&width, &[root], &head, "t7")
            .unwrap_err()
            .contains("root commit")
    );

    drop(&width);
}

#[test]
fn a_remote_branch_deleted_after_the_merge_leaves_the_commits_unpushed() {
    let width = repo("gone");

    sh(&width, "git push -q origin --delete feat && git fetch -q --prune");

    assert!(upstream(&width).is_none(), "[gone]: 0 upstream");

    let cs = list(&width, &base(&width));

    assert!(cs.list.iter().all(|character| !character.pushed) && cs.list.len() == 4);

    drop(&width);
}
