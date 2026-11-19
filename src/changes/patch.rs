//! A part of a file's diff as a patch of its own: the picked lines alone, for `git apply`.
//!
//! - forward (stage): the old side stays the old file; picked changes apply, the rest of the
//!   added lines go, the rest of the removed lines stay as context.
//! - reverse (stash): the new side stays the worktree file; `git apply -R` takes the picked
//!   changes out, the rest stays. The patch itself puts them back (restore).

use std::collections::BTreeSet;

/// The picked lines of 1 file: added lines by new-side number, removed lines by (the new-side
/// line number they sit before, their text); ALL: every change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    pub adds: BTreeSet<u32>,
    pub dels: BTreeSet<(u32, String)>,
    pub all: bool,
}

impl Pick {
    pub fn is_empty(&self) -> bool {
        !self.all && self.adds.is_empty() && self.dels.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Forward,
    Reverse,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Ctx,
    Old,
    New,
    /// `\ No newline at end of file`.
    Mark,
}

/// `-a,b` / `+c,d` of a hunk head: (start, count).
fn range(w: &str) -> Option<(u32, u32)> {
    let mut it = w.get(1..)?.split(',');
    let s = it.next()?.parse().ok()?;
    let c = it.next().map_or(Some(1), |x| x.parse().ok())?;
    Some((s, c))
}

/// The patch of the changes of DIFF (1 file, unified, any -U) that P picks, for DIR. None: 0
/// picked change in DIFF.
pub fn pick(diff: &str, p: &Pick, dir: Dir) -> Option<String> {
    let (mut a, mut b) = (String::new(), String::new());
    let (mut old_null, mut new_null) = (false, false);
    let mut mode = String::new();
    // hunks: orig (old start, old count, new start, new count), lines
    let mut hunks: Vec<((u32, u32, u32, u32), Vec<(Role, String)>)> = Vec::new();
    let mut n = 0u32;
    // the last diff line made it into the patch (a `\ No newline` mark follows it)
    let mut kept = false;
    for l in diff.lines() {
        if hunks.is_empty() || l.starts_with("@@ ") {
            if let Some(rest) = l.strip_prefix("diff --git ") {
                if let Some((x, y)) = rest.split_once(" b/") {
                    a = x.trim_start_matches("a/").to_string();
                    b = y.to_string();
                }
            } else if let Some(m) = l.strip_prefix("new file mode ").or_else(|| l.strip_prefix("deleted file mode ")) {
                mode = m.trim().to_string();
            } else if let Some(x) = l.strip_prefix("--- ") {
                old_null = x == "/dev/null";
                if !old_null {
                    a = x.strip_prefix("a/").unwrap_or(x).to_string();
                }
            } else if let Some(x) = l.strip_prefix("+++ ") {
                new_null = x == "/dev/null";
                if !new_null {
                    b = x.strip_prefix("b/").unwrap_or(x).to_string();
                }
            } else if let Some(h) = l.strip_prefix("@@ ") {
                let mut ws = h.split(' ');
                let (os, oc) = ws.next().and_then(range)?;
                let (ns, nc) = ws.next().and_then(range)?;
                hunks.push(((os, oc, ns, nc), Vec::new()));
                n = ns;
            }
            continue;
        }
        let Some((_, lines)) = hunks.last_mut() else { continue };
        let (c, text) = match l.as_bytes().first() {
            Some(b'\\') => {
                if kept {
                    lines.push((Role::Mark, l.to_string()));
                }
                continue;
            }
            Some(&c) => (c, &l[1..]),
            None => (b' ', ""),
        };
        let role = match c {
            b' ' => {
                n += 1;
                Some(Role::Ctx)
            }
            b'+' => {
                let on = p.all || p.adds.contains(&n);
                n += 1;
                match (on, dir) {
                    (true, _) => Some(Role::New),
                    (false, Dir::Forward) => None,
                    (false, Dir::Reverse) => Some(Role::Ctx),
                }
            }
            b'-' => {
                let on = p.all || p.dels.contains(&(n, text.replace('\t', "    ")));
                match (on, dir) {
                    (true, _) => Some(Role::Old),
                    (false, Dir::Forward) => Some(Role::Ctx),
                    (false, Dir::Reverse) => None,
                }
            }
            _ => continue,
        };
        kept = role.is_some();
        if let Some(r) = role {
            lines.push((r, text.to_string()));
        }
    }
    if a.is_empty() {
        a = b.clone();
    }
    if b.is_empty() {
        b = a.clone();
    }
    let mut body = String::new();
    let (mut delta, mut old_lines, mut new_lines) = (0i64, 0u32, 0u32);
    for ((os, oc, ns, nc), lines) in &hunks {
        if !lines.iter().any(|(r, _)| matches!(r, Role::Old | Role::New)) {
            continue;
        }
        let old = lines.iter().filter(|(r, _)| matches!(r, Role::Ctx | Role::Old)).count() as u32;
        let new = lines.iter().filter(|(r, _)| matches!(r, Role::Ctx | Role::New)).count() as u32;
        // the 1st line of each side (a 0-line side's start: the line before it)
        let first = |s: u32, c: u32| i64::from(if c == 0 { s + 1 } else { s });
        let (of, nf) = match dir {
            Dir::Forward => {
                let of = first(*os, *oc);
                (of, of + delta)
            }
            Dir::Reverse => {
                let nf = first(*ns, *nc);
                (nf - delta, nf)
            }
        };
        let start = |f: i64, c: u32| (if c == 0 { f - 1 } else { f }).max(0);
        body.push_str(&format!("@@ -{},{old} +{},{new} @@\n", start(of, old), start(nf, new)));
        for (r, t) in lines {
            match r {
                Role::Ctx => body.push(' '),
                Role::Old => body.push('-'),
                Role::New => body.push('+'),
                Role::Mark => {}
            }
            body.push_str(t);
            body.push('\n');
        }
        delta += i64::from(new) - i64::from(old);
        old_lines += old;
        new_lines += new;
    }
    if body.is_empty() {
        return None;
    }
    let mode = if mode.is_empty() { "100644".to_string() } else { mode };
    let mut head = format!("diff --git a/{a} b/{b}\n");
    let (old_gone, new_gone) = (old_null && old_lines == 0, new_null && new_lines == 0);
    if old_gone {
        head.push_str(&format!("new file mode {mode}\n--- /dev/null\n"));
    } else {
        head.push_str(&format!("--- a/{a}\n"));
    }
    if new_gone {
        head = head.replacen(&format!("--- a/{a}\n"), &format!("deleted file mode {mode}\n--- a/{a}\n"), 1);
        head.push_str("+++ /dev/null\n");
    } else {
        head.push_str(&format!("+++ b/{b}\n"));
    }
    Some(head + &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    const D: &str = "diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,6 +1,6 @@
 a
-b
+B
 c
-d
+D
 e
 f
";

    fn adds(v: &[u32]) -> Pick {
        Pick { adds: v.iter().copied().collect(), ..Default::default() }
    }

    #[test]
    fn forward_keeps_the_picked_add_drops_the_rest() {
        // B alone: `-b` stays (context), `-d`, `+D`: d stays
        let p = Pick { adds: [2].into(), dels: [(2, "b".into())].into(), all: false };
        let out = pick(D, &p, Dir::Forward).unwrap();
        assert_eq!(out, "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,6 +1,6 @@\n a\n-b\n+B\n c\n d\n e\n f\n");
        // only the add of D: an insertion (d stays)
        let out = pick(D, &adds(&[4]), Dir::Forward).unwrap();
        assert!(out.contains("@@ -1,6 +1,7 @@\n a\n b\n c\n d\n+D\n e\n f\n"), "{out}");
        assert!(pick(D, &adds(&[9]), Dir::Forward).is_none());
    }

    #[test]
    fn reverse_keeps_the_worktree_side() {
        // take B out (back to b): the rest (D) stays as context
        let p = Pick { adds: [2].into(), dels: [(2, "b".into())].into(), all: false };
        let out = pick(D, &p, Dir::Reverse).unwrap();
        assert!(out.ends_with("@@ -1,6 +1,6 @@\n a\n-b\n+B\n c\n D\n e\n f\n"), "{out}");
    }

    #[test]
    fn a_new_file_part_and_whole() {
        let d = "diff --git a/n b/n\nnew file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@\n+1\n+2\n+3\n";
        let f = pick(d, &adds(&[2]), Dir::Forward).unwrap();
        assert_eq!(f, "diff --git a/n b/n\nnew file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,1 @@\n+2\n");
        // reverse, line 2: the file stays, 1 and 3 are its context
        let r = pick(d, &adds(&[2]), Dir::Reverse).unwrap();
        assert!(r.ends_with("--- a/n\n+++ b/n\n@@ -1,2 +1,3 @@\n 1\n+2\n 3\n"), "{r}");
        let all = pick(d, &Pick { all: true, ..Default::default() }, Dir::Reverse).unwrap();
        assert!(all.contains("new file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@"), "{all}");
    }

    fn sh(d: &std::path::Path, c: &str) -> String {
        let o = Command::new("sh").arg("-c").arg(c).current_dir(d).stderr(Stdio::piped()).output().unwrap();
        assert!(o.status.success(), "{c}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn apply(d: &std::path::Path, patch: &str, args: &str) {
        let f = d.join("p.diff");
        std::fs::write(&f, patch).unwrap();
        sh(d, &format!("git apply {args} p.diff && rm p.diff"));
    }

    #[test]
    fn real_git_takes_the_patches() {
        let d = std::env::temp_dir().join(format!("codehead-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, "git init -q && git config user.email t@t && git config user.name t && seq 1 30 > x && git add x && git commit -qm i");
        // 2 hunks: 3 → three, 4 → four; 25 removed, 26 → twenty-six
        sh(&d, "seq 1 30 | sed 's/^3$/three/;s/^4$/four/;/^25$/d;s/^26$/twenty-six/' > x");
        let diff = sh(&d, "git diff -- x");
        // stage `four` (its removed `4` too) and the removal of 25 (it sits before new line 25)
        let p = Pick { adds: [4].into(), dels: [(3, "4".into()), (25, "25".into())].into(), all: false };
        apply(&d, &pick(&diff, &p, Dir::Forward).unwrap(), "--cached");
        let cached = sh(&d, "git diff --cached -U0 -- x");
        assert!(cached.contains("-4\n+four") && cached.contains("-25") && !cached.contains("three") && !cached.contains("twenty"), "{cached}");
        // stash `three` (back to 3) in the worktree; the patch puts it back
        let diff = sh(&d, "git diff HEAD -- x");
        let p = Pick { adds: [3].into(), dels: [(3, "3".into())].into(), all: false };
        let stash = pick(&diff, &p, Dir::Reverse).unwrap();
        apply(&d, &stash, "-R");
        let now = std::fs::read_to_string(d.join("x")).unwrap();
        assert!(now.contains("\n3\nfour\n") && now.contains("twenty-six") && !now.contains("\n25\n"), "{now}");
        apply(&d, &stash, "");
        assert!(std::fs::read_to_string(d.join("x")).unwrap().contains("\nthree\nfour\n"));
        // an untracked file: a part of it staged
        sh(&d, "printf 'a\\nb\\nc\\n' > n");
        let o = Command::new("git").args(["diff", "--no-index", "--", "/dev/null", "n"]).current_dir(&d).output().unwrap();
        let diff = String::from_utf8_lossy(&o.stdout);
        apply(&d, &pick(&diff, &adds(&[2]), Dir::Forward).unwrap(), "--cached");
        assert_eq!(sh(&d, "git show :n"), "b\n");
        let _ = std::fs::remove_dir_all(d);
    }
}
