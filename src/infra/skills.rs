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

        let Some((key, value_value)) = line.split_once(':') else {
            continue;
        };

        let value_2 = value_value.trim();

        let value_3 = if matches!(value_2, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
            ""
        } else {
            value_2
        };

        let value_4 = value_3
            .strip_prefix('"')
            .and_then(|x| x.strip_suffix('"'))
            .or_else(|| value_3.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
            .unwrap_or(value_3);

        match key.trim() {
            "name" => {
                name = Some(value_4.to_string());
                open = Some(&mut name);
            }
            "description" => {
                desc = Some(value_4.to_string());
                open = Some(&mut desc);
            }
            _ => {}
        }
    }

    (
        name.filter(|count| !count.is_empty()),
        desc.filter(|directory| !directory.is_empty()),
    )
}

/// Name of the plugin that holds DIR: `name` of the nearest `MARK/plugin.json` above it (below ROOT).
fn plugin_of(dir: &Path, root: &Path, mark: &str) -> Option<String> {
    let mut directory = Some(dir);

    while let Some(path) = directory {
        if let Ok(thread) = std::fs::read_to_string(path.join(mark).join("plugin.json")) {
            return serde_json::from_str::<serde_json::Value>(&thread).ok()?["name"]
                .as_str()
                .map(String::from);
        }

        if path == root {
            break;
        }

        directory = path.parent();
    }

    None
}

/// Every SKILL.md below ROOT (max. DEPTH levels) → OUT. MARK: the plugin manifest dir
/// (`.claude-plugin`): a skill of a plugin is `plugin:skill`.
fn scan(root: &Path, depth: usize, harness: &str, mark: Option<&str>, out: &mut Vec<Skill>) {
    let mut dirs: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, directory)) = dirs.pop() {
        scan_path(
            out,
            &mut dirs,
            &(dir),
            directory,
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
    directory: usize,
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

    for error in rd.flatten() {
        let path = error.path();
        let name = error.file_name().to_string_lossy().into_owned();

        if path.is_dir() {
            queue_directory(path, &name, directory, depth, dirs);

            continue;
        }

        if name != "SKILL.md" {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        let (count, description) = front_matter(&text);

        let skill = count.unwrap_or_else(|| {
            dir.file_name()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

        if skill.is_empty() {
            continue;
        }

        let name = match mark.and_then(|message| plugin_of(dir, root, message)) {
            Some(path_value) => format!("{path_value}:{skill}"),
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
    let mut value = Vec::new();
    let mut directory = Some(cwd);

    while let Some(path) = directory {
        if path == home {
            break;
        }

        if path.join(rel).is_dir() {
            value.push(path.join(rel));
        }

        if path.join(".git").exists() {
            break;
        }

        directory = path.parent();
    }

    value
}

/// Skills of HARNESSES (`claude`, `codex`; others: 0 known skill dirs) for a thread in CWD, below
/// HOME. By harness, then name; 1 row per name (plugin versions side by side: 1 of them).
pub(crate) fn load_in(home: &Path, harnesses: &[String], cwd: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    let has = |height: &str| harnesses.iter().any(|x| x == height);

    if has("claude") {
        for directory in project_dirs(cwd, ".claude/skills") {
            scan(&directory, 3, "claude", None, &mut out);
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
        for directory_value in [".codex/skills", ".agents/skills"]
            .into_iter()
            .flat_map(|rel| project_dirs(cwd, rel))
        {
            scan(&directory_value, 3, "codex", None, &mut out);
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

    out.sort_by(|agent, bytes| {
        (&agent.harness, &agent.name, bytes.description.len()).cmp(&(
            &bytes.harness,
            &bytes.name,
            agent.description.len(),
        ))
    });

    out.dedup_by(|agent_value, bytes_value| {
        agent_value.harness == bytes_value.harness && agent_value.name == bytes_value.name
    });

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
fn queue_directory(path: PathBuf, name: &str, directory: usize, depth: usize, dirs: &mut Vec<(PathBuf, usize)>) {
    if directory < depth && !matches!(name, "node_modules" | ".git" | "marketplaces") {
        dirs.push((path, directory + 1));
    }
}
