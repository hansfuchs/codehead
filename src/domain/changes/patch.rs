//! A part of a file's diff as a patch of its own: the picked lines alone, for `git apply`.
//!
//! - forward (stage): the old side stays the old file; picked changes apply, the rest of the
//!   added lines go, the rest of the removed lines stay as context.
//! - reverse (stash): the new side stays the worktree file; `git apply -R` takes the picked
//!   changes out, the rest stays. The patch itself puts them back (restore).

use std::collections::BTreeSet;
use std::fmt::Write as _;

/// The picked lines of 1 file: added lines by new-side number, removed lines by (the new-side
/// line number they sit before, their text); ALL: every change.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Pick {
    pub adds: BTreeSet<u32>,
    pub dels: BTreeSet<(u32, String)>,
    pub all: bool,
}

impl Pick {
    pub(crate) fn is_empty(&self) -> bool {
        !self.all && self.adds.is_empty() && self.dels.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dir {
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
fn range(width: &str) -> Option<(u32, u32)> {
    let mut it = width.get(1..)?.split(',');
    let text = it.next()?.parse().ok()?;
    let character = it.next().map_or(Some(1), |x| x.parse().ok())?;

    Some((text, character))
}

/// The patch of the changes of DIFF (1 file, unified, any -U) that P picks, for DIR. None: 0
/// picked change in DIFF.
pub(super) fn pick(diff: &str, pick: &Pick, direction: Dir) -> Option<String> {
    let mut selection = PatchSelection {
        pick,
        direction,
        header: PatchHeader::default(),
        hunks: Vec::new(),
        new_line: 0,
        kept: false,
    };

    for line in diff.lines() {
        selection.line(line)?;
    }

    selection.finish()
}

#[derive(Default)]
struct PatchHeader {
    old_path: String,
    new_path: String,
    old_null: bool,
    new_null: bool,
    mode: String,
}

struct HunkRange {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
}

struct PatchHunk {
    range: HunkRange,
    lines: Vec<(Role, String)>,
}

struct PatchSelection<'pick> {
    pick: &'pick Pick,
    direction: Dir,
    header: PatchHeader,
    hunks: Vec<PatchHunk>,
    new_line: u32,
    kept: bool,
}

#[derive(Default)]
struct PatchBody {
    text: String,
    delta: i64,
    old_lines: u32,
    new_lines: u32,
}

impl PatchSelection<'_> {
    fn line(&mut self, line: &str) -> Option<()> {
        if self.hunks.is_empty() || line.starts_with("@@ ") {
            header_line(line, self)?;
            return Some(());
        }

        if line.starts_with('\\') {
            marker(line, self);
            return Some(());
        }

        let (prefix, text) = line
            .as_bytes()
            .first()
            .map_or((b' ', ""), |&prefix| (prefix, &line[1..]));

        if !matches!(prefix, b' ' | b'+' | b'-') {
            return Some(());
        }

        let role = role(prefix, text, self);

        self.kept = role.is_some();

        if let (Some(role), Some(hunk)) = (role, self.hunks.last_mut()) {
            hunk.lines.push((role, text.to_string()));
        }

        Some(())
    }

    fn finish(mut self) -> Option<String> {
        let mut body = PatchBody::default();

        for hunk in &self.hunks {
            body.hunk(hunk, self.direction);
        }

        if body.text.is_empty() {
            return None;
        }

        Some(self.header.render(&body) + &body.text)
    }
}

fn selected_role(selected: bool, direction: Dir, role: Role) -> Option<Role> {
    if selected {
        return Some(role);
    }

    match (role, direction) {
        (Role::Old, Dir::Forward) | (Role::New, Dir::Reverse) => Some(Role::Ctx),
        _ => None,
    }
}

impl PatchHeader {
    fn line(&mut self, line: &str) {
        let paths = line.strip_prefix("diff --git ").and_then(|rest| rest.split_once(" b/"));

        if let Some((old, new)) = paths {
            self.old_path = old.trim_start_matches("a/").to_string();
            new.clone_into(&mut self.new_path);
        } else if let Some(mode) = line
            .strip_prefix("new file mode ")
            .or_else(|| line.strip_prefix("deleted file mode "))
        {
            mode.trim().clone_into(&mut self.mode);
        } else if let Some(path) = line.strip_prefix("--- ") {
            old_path(path, self);
        } else if let Some(path) = line.strip_prefix("+++ ") {
            new_path(path, self);
        }
    }

    fn render(&mut self, body: &PatchBody) -> String {
        if self.old_path.is_empty() {
            self.old_path.clone_from(&self.new_path);
        }

        if self.new_path.is_empty() {
            self.new_path.clone_from(&self.old_path);
        }

        let mode = if self.mode.is_empty() { "100644" } else { &self.mode };
        let mut header = format!("diff --git a/{} b/{}\n", self.old_path, self.new_path);

        if self.old_null && body.old_lines == 0 {
            let _ = write!(header, "new file mode {mode}\n--- /dev/null\n");
        } else {
            let _ = writeln!(header, "--- a/{}", self.old_path);
        }

        if self.new_null && body.new_lines == 0 {
            header = header.replacen(
                &format!("--- a/{}\n", self.old_path),
                &format!("deleted file mode {mode}\n--- a/{}\n", self.old_path),
                1,
            );
            header.push_str("+++ /dev/null\n");
        } else {
            let _ = writeln!(header, "+++ b/{}", self.new_path);
        }

        header
    }
}

impl PatchHunk {
    fn changed(&self) -> bool {
        self.lines.iter().any(|(role, _)| matches!(role, Role::Old | Role::New))
    }

    fn counts(&self) -> (u32, u32) {
        let old = self
            .lines
            .iter()
            .filter(|(role, _)| matches!(role, Role::Ctx | Role::Old))
            .count() as u32;
        let new = self
            .lines
            .iter()
            .filter(|(role, _)| matches!(role, Role::Ctx | Role::New))
            .count() as u32;

        (old, new)
    }
}

impl HunkRange {
    fn starts(&self, direction: Dir, delta: i64) -> (i64, i64) {
        match direction {
            Dir::Forward => {
                let first = first_line(self.old_start, self.old_count);

                (first, first + delta)
            }
            Dir::Reverse => {
                let first = first_line(self.new_start, self.new_count);

                (first - delta, first)
            }
        }
    }
}

fn first_line(start: u32, count: u32) -> i64 {
    i64::from(if count == 0 { start + 1 } else { start })
}

fn patch_start(first: i64, count: u32) -> i64 {
    (if count == 0 { first - 1 } else { first }).max(0)
}

impl PatchBody {
    fn hunk(&mut self, hunk: &PatchHunk, direction: Dir) {
        if !hunk.changed() {
            return;
        }

        let (old, new) = hunk.counts();
        let (old_first, new_first) = hunk.range.starts(direction, self.delta);
        let _ = writeln!(
            self.text,
            "@@ -{},{old} +{},{new} @@",
            patch_start(old_first, old),
            patch_start(new_first, new)
        );

        for (role, text) in &hunk.lines {
            append_line(&mut self.text, *role, text);
        }

        self.delta += i64::from(new) - i64::from(old);
        self.old_lines += old;
        self.new_lines += new;
    }
}

fn append_line(output: &mut String, role: Role, text: &str) {
    match role {
        Role::Ctx => output.push(' '),
        Role::Old => output.push('-'),
        Role::New => output.push('+'),
        Role::Mark => {}
    }

    output.push_str(text);
    output.push('\n');
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;

fn header_line(line: &str, model_state: &mut PatchSelection<'_>) -> Option<()> {
    let Some(header) = line.strip_prefix("@@ ") else {
        model_state.header.line(line);
        return Some(());
    };

    let mut ranges = header.split(' ');
    let (old_start, old_count) = ranges.next().and_then(range)?;
    let (new_start, new_count) = ranges.next().and_then(range)?;

    model_state.new_line = new_start;
    model_state.hunks.push(PatchHunk {
        range: HunkRange {
            old_start,
            old_count,
            new_start,
            new_count,
        },
        lines: Vec::new(),
    });

    Some(())
}

fn marker(line: &str, model_state: &mut PatchSelection<'_>) {
    if !model_state.kept {
        return;
    }

    if let Some(hunk) = model_state.hunks.last_mut() {
        hunk.lines.push((Role::Mark, line.to_string()));
    }
}

fn role(prefix: u8, text: &str, model_state: &mut PatchSelection<'_>) -> Option<Role> {
    match prefix {
        b' ' => {
            model_state.new_line += 1;
            Some(Role::Ctx)
        }
        b'+' => {
            let selected = model_state.pick.all || model_state.pick.adds.contains(&model_state.new_line);

            model_state.new_line += 1;
            selected_role(selected, model_state.direction, Role::New)
        }
        b'-' => {
            let selected = model_state.pick.all
                || model_state
                    .pick
                    .dels
                    .contains(&(model_state.new_line, text.replace('\t', "    ")));

            selected_role(selected, model_state.direction, Role::Old)
        }
        _ => None,
    }
}

fn old_path(path: &str, model_state: &mut PatchHeader) {
    model_state.old_null = path == "/dev/null";

    if !model_state.old_null {
        path.strip_prefix("a/")
            .unwrap_or(path)
            .clone_into(&mut model_state.old_path);
    }
}

fn new_path(path: &str, model_state: &mut PatchHeader) {
    model_state.new_null = path == "/dev/null";

    if !model_state.new_null {
        path.strip_prefix("b/")
            .unwrap_or(path)
            .clone_into(&mut model_state.new_path);
    }
}
