//! Skills of the harnesses (`> skills`): every SKILL.md the harness of an agent can load, with its
//! name + description (front matter). Claude Code: ~/.claude/skills, installed plugins
//! (~/.claude/plugins/synced + cache), the project's .claude/skills.
//! Codex: ~/.codex/skills, its plugin cache, the project's .codex/skills + .agents/skills.
//! Skills built into a harness binary have 0 file: not listed.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    /// `skill`, or `plugin:skill` for a plugin's skill.
    pub name: String,
    pub description: String,
    pub harness: String,
}

/// `name` + `description` of a SKILL.md's front matter (a folded / multi-line value: joined).
pub fn front_matter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let (mut name, mut desc) = (None, None);
    let mut open: Option<&mut Option<String>> = None;
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        // an indented line: more of the value above
        if l.starts_with([' ', '\t']) {
            if let Some(Some(v)) = open.as_deref_mut() {
                if !v.is_empty() {
                    v.push(' ');
                }
                v.push_str(l.trim());
            }
            continue;
        }
        open = None;
        let Some((k, v)) = l.split_once(':') else { continue };
        let v = v.trim();
        let v = if matches!(v, ">" | "|" | ">-" | "|-" | ">+" | "|+") { "" } else { v };
        let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(v);
        match k.trim() {
            "name" => {
                name = Some(v.to_string());
                open = Some(&mut name);
            }
            "description" => {
                desc = Some(v.to_string());
                open = Some(&mut desc);
            }
            _ => {}
        }
    }
    (name.filter(|n| !n.is_empty()), desc.filter(|d| !d.is_empty()))
}

/// Name of the plugin that holds DIR: `name` of the nearest `MARK/plugin.json` above it (below ROOT).
fn plugin_of(dir: &Path, root: &Path, mark: &str) -> Option<String> {
    let mut d = Some(dir);
    while let Some(p) = d {
        if let Ok(t) = std::fs::read_to_string(p.join(mark).join("plugin.json")) {
            return serde_json::from_str::<serde_json::Value>(&t).ok()?["name"].as_str().map(String::from);
        }
        if p == root {
            break;
        }
        d = p.parent();
    }
    None
}

/// Every SKILL.md below ROOT (max. DEPTH levels) → OUT. MARK: the plugin manifest dir
/// (`.claude-plugin`): a skill of a plugin is `plugin:skill`.
fn scan(root: &Path, depth: usize, harness: &str, mark: Option<&str>, out: &mut Vec<Skill>) {
    let mut dirs: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, d)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if d < depth && !matches!(name.as_str(), "node_modules" | ".git" | "marketplaces") {
                    dirs.push((path, d + 1));
                }
                continue;
            }
            if name != "SKILL.md" {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let (n, description) = front_matter(&text);
            let skill = n.unwrap_or_else(|| dir.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default());
            if skill.is_empty() {
                continue;
            }
            let name = match mark.and_then(|m| plugin_of(&dir, root, m)) {
                Some(p) => format!("{p}:{skill}"),
                None => skill,
            };
            out.push(Skill { name, description: description.unwrap_or_default(), harness: harness.to_string() });
        }
    }
}

/// Project skill dirs: REL (`.claude/skills`) in CWD and each dir above it, up to the git top / home.
fn project_dirs(cwd: &Path, rel: &str) -> Vec<PathBuf> {
    let home = crate::config::home();
    let mut v = Vec::new();
    let mut d = Some(cwd);
    while let Some(p) = d {
        if p == home {
            break;
        }
        if p.join(rel).is_dir() {
            v.push(p.join(rel));
        }
        if p.join(".git").exists() {
            break;
        }
        d = p.parent();
    }
    v
}

/// Skills of HARNESSES (`claude`, `codex`; others: 0 known skill dirs) for a thread in CWD, below
/// HOME. By harness, then name; 1 row per name (plugin versions side by side: 1 of them).
pub fn load_in(home: &Path, harnesses: &[String], cwd: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    let has = |h: &str| harnesses.iter().any(|x| x == h);
    if has("claude") {
        for d in project_dirs(cwd, ".claude/skills") {
            scan(&d, 3, "claude", None, &mut out);
        }
        scan(&home.join(".claude/skills"), 4, "claude", None, &mut out);
        scan(&home.join(".claude/plugins"), 8, "claude", Some(".claude-plugin"), &mut out);
    }
    if has("codex") {
        for rel in [".codex/skills", ".agents/skills"] {
            for d in project_dirs(cwd, rel) {
                scan(&d, 3, "codex", None, &mut out);
            }
        }
        scan(&home.join(".codex/skills"), 4, "codex", None, &mut out);
        scan(&home.join(".agents/skills"), 4, "codex", None, &mut out);
        scan(&home.join(".codex/plugins"), 8, "codex", Some(".codex-plugin"), &mut out);
    }
    out.sort_by(|a, b| (&a.harness, &a.name, b.description.len()).cmp(&(&b.harness, &b.name, a.description.len())));
    out.dedup_by(|a, b| a.harness == b.harness && a.name == b.name);
    out
}

pub fn load(harnesses: &[String], cwd: &Path) -> Vec<Skill> {
    load_in(&crate::config::home(), harnesses, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_values() {
        let (n, d) = front_matter("---\nname: tmux\ndescription: Use the tmux CLI: tabs, panes.\nmetadata:\n  x: \"2\"\n---\nbody\nname: no");
        assert_eq!((n.as_deref(), d.as_deref()), (Some("tmux"), Some("Use the tmux CLI: tabs, panes.")));
        let (n, d) = front_matter("---\nname: \"a-b\"\ndescription: >\n  line 1\n  line 2\nother: x\n---\n");
        assert_eq!((n.as_deref(), d.as_deref()), (Some("a-b"), Some("line 1 line 2")));
        assert_eq!(front_matter("# no front matter"), (None, None));
    }

    #[test]
    fn skills_of_the_harnesses() {
        let home = std::env::temp_dir().join(format!("codehead-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let put = |rel: &str, text: &str| {
            let p = home.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        put(".claude/skills/tmux/SKILL.md", "---\nname: tmux\ndescription: tmux cli\n---\n");
        put(".claude/skills/bare/SKILL.md", "no front matter");
        put(".claude/plugins/synced/x/general/.claude-plugin/plugin.json", "{\"name\":\"general\"}");
        put(".claude/plugins/synced/x/general/skills/antislop/SKILL.md", "---\nname: antislop\ndescription: fix slop\n---\n");
        put(".claude/plugins/synced/x/general~g2/.claude-plugin/plugin.json", "{\"name\":\"general\"}");
        put(".claude/plugins/synced/x/general~g2/skills/antislop/SKILL.md", "---\nname: antislop\ndescription: fix slop\n---\n");
        // a marketplace catalog: not installed, not listed
        put(".claude/plugins/marketplaces/m/p/skills/nope/SKILL.md", "---\nname: nope\n---\n");
        put(".codex/skills/.system/imagegen/SKILL.md", "---\nname: imagegen\ndescription: images\n---\n");
        put("proj/.git/HEAD", "");
        put("proj/.claude/skills/deploy/SKILL.md", "---\nname: deploy\ndescription: ship it\n---\n");
        let cwd = home.join("proj/sub");
        std::fs::create_dir_all(&cwd).unwrap();
        let names = |h: &[&str]| load_in(&home, &h.iter().map(|x| x.to_string()).collect::<Vec<_>>(), &cwd).into_iter().map(|s| format!("{}/{}", s.harness, s.name)).collect::<Vec<_>>();
        assert_eq!(names(&["claude"]), ["claude/bare", "claude/deploy", "claude/general:antislop", "claude/tmux"]);
        assert_eq!(names(&["codex", "claude"]).len(), 5);
        assert_eq!(names(&["codex"]), ["codex/imagegen"]);
        assert!(names(&["pi"]).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }
}
