//! Skills of the harnesses (`> skills`): every SKILL.md the harness of an agent can load, with its
//! name + description (front matter). Claude Code: ~/.claude/skills, installed plugins
//! (~/.claude/plugins/synced + cache), the project's .claude/skills.
//! Codex: ~/.codex/skills, its plugin cache, the project's .codex/skills + .agents/skills.
//! Skills built into a harness binary have 0 file: not listed.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Skill {
    /// `skill`, or `plugin:skill` for a plugin's skill.
    pub name: String,
    pub description: String,
    pub harness: String,
}

/// `name` + `description` of a SKILL.md's front matter (a folded / multi-line value: joined).
pub(crate) fn front_matter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();

    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }

    let (mut name, mut desc) = (None, None);
    let mut open: Option<&mut Option<String>> = None;

    for line in lines {
        if line.trim() == "---" {
            break;
        }

        // an indented line: more of the value above
        if line.starts_with([' ', '\t']) {
            continue_field(open.as_deref_mut().and_then(Option::as_mut), line);

            continue;
        }

        open = None;

        let Some((key, raw)) = line.split_once(':') else {
            continue;
        };

        let trimmed = raw.trim();

        let plain = if matches!(trimmed, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
            ""
        } else {
            trimmed
        };

        let unquoted = plain
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .or_else(|| plain.strip_prefix('\'').and_then(|inner| inner.strip_suffix('\'')))
            .unwrap_or(plain);

        match key.trim() {
            "name" => {
                name = Some(unquoted.to_string());
                open = Some(&mut name);
            }
            "description" => {
                desc = Some(unquoted.to_string());
                open = Some(&mut desc);
            }
            _ => {}
        }
    }

    (
        name.filter(|name| !name.is_empty()),
        desc.filter(|desc| !desc.is_empty()),
    )
}

/// Name of the plugin that holds DIR: `name` of the nearest `MARK/plugin.json` above it (below ROOT).
fn plugin_of(dir: &Path, root: &Path, mark: &str) -> Option<String> {
    let mut current = Some(dir);

    while let Some(path) = current {
        if let Ok(manifest) = std::fs::read_to_string(path.join(mark).join("plugin.json")) {
            return serde_json::from_str::<serde_json::Value>(&manifest).ok()?["name"]
                .as_str()
                .map(String::from);
        }

        if path == root {
            break;
        }

        current = path.parent();
    }

    None
}

/// Every SKILL.md below ROOT (max. DEPTH levels) → OUT. MARK: the plugin manifest dir
/// (`.claude-plugin`): a skill of a plugin is `plugin:skill`.
fn scan(root: &Path, depth: usize, harness: &str, mark: Option<&str>, out: &mut Vec<Skill>) {
    let mut dirs: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, level)) = dirs.pop() {
        scan_path(
            out,
            &mut dirs,
            &(dir),
            level,
            ScanSettings {
                root,
                depth,
                harness,
                mark,
            },
        );
    }
}

fn scan_path(
    out: &mut Vec<Skill>,
    dirs: &mut Vec<(PathBuf, usize)>,
    dir: &PathBuf,
    level: usize,
    settings: ScanSettings<'_>,
) {
    let ScanSettings {
        root,
        depth,
        harness,
        mark,
    } = settings;

    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if path.is_dir() {
            queue_directory(path, &name, level, depth, dirs);

            continue;
        }

        if name != "SKILL.md" {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        let (declared, description) = front_matter(&text);

        let skill = declared.unwrap_or_else(|| {
            dir.file_name()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

        if skill.is_empty() {
            continue;
        }

        let name = match mark.and_then(|mark| plugin_of(dir, root, mark)) {
            Some(plugin) => format!("{plugin}:{skill}"),
            None => skill,
        };

        out.push(Skill {
            name,
            description: description.unwrap_or_default(),
            harness: harness.to_string(),
        });
    }
}

/// Project skill dirs: REL (`.claude/skills`) in CWD and each dir above it, up to the git top / home.
fn project_dirs(cwd: &Path, rel: &str) -> Vec<PathBuf> {
    let home = crate::infra::config::home();
    let mut dirs = Vec::new();
    let mut current = Some(cwd);

    while let Some(path) = current {
        if path == home {
            break;
        }

        if path.join(rel).is_dir() {
            dirs.push(path.join(rel));
        }

        if path.join(".git").exists() {
            break;
        }

        current = path.parent();
    }

    dirs
}

/// Skills of HARNESSES (`claude`, `codex`; others: 0 known skill dirs) for a thread in CWD, below
/// HOME. By harness, then name; 1 row per name (plugin versions side by side: 1 of them).
pub(crate) fn load_in(home: &Path, harnesses: &[String], cwd: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    let has = |harness: &str| harnesses.iter().any(|name| name == harness);

    if has("claude") {
        for dir in project_dirs(cwd, ".claude/skills") {
            scan(&dir, 3, "claude", None, &mut out);
        }

        scan(&home.join(".claude/skills"), 4, "claude", None, &mut out);
        scan(
            &home.join(".claude/plugins"),
            8,
            "claude",
            Some(".claude-plugin"),
            &mut out,
        );
    }

    if has("codex") {
        for dir in [".codex/skills", ".agents/skills"]
            .into_iter()
            .flat_map(|rel| project_dirs(cwd, rel))
        {
            scan(&dir, 3, "codex", None, &mut out);
        }

        scan(&home.join(".codex/skills"), 4, "codex", None, &mut out);
        scan(&home.join(".agents/skills"), 4, "codex", None, &mut out);
        scan(
            &home.join(".codex/plugins"),
            8,
            "codex",
            Some(".codex-plugin"),
            &mut out,
        );
    }

    out.sort_by(|left, right| {
        (&left.harness, &left.name, right.description.len()).cmp(&(&right.harness, &right.name, left.description.len()))
    });

    out.dedup_by(|left, right| left.harness == right.harness && left.name == right.name);

    out
}

pub(crate) fn load(harnesses: &[String], cwd: &Path) -> Vec<Skill> {
    load_in(&crate::infra::config::home(), harnesses, cwd)
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct ScanSettings<'context> {
    root: &'context Path,
    depth: usize,
    harness: &'context str,
    mark: Option<&'context str>,
}

fn continue_field(value: Option<&mut String>, line: &str) {
    let Some(value) = value else {
        return;
    };

    if !value.is_empty() {
        value.push(' ');
    }

    value.push_str(line.trim());
}
fn queue_directory(path: PathBuf, name: &str, level: usize, depth: usize, dirs: &mut Vec<(PathBuf, usize)>) {
    if level < depth && !matches!(name, "node_modules" | ".git" | "marketplaces") {
        dirs.push((path, level + 1));
    }
}
