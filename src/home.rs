//! The home repo's list of other repos (its AGENTS.md, "Other repos": 1 line per repo with its
//! name, a local path and a clone URL in backticks): the repos a new thread can start in.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub name: String,
    pub path: PathBuf,
    /// Clone URL (git@…, https://…, ssh://…); None: 0 in the line.
    pub url: Option<String>,
}

/// Repos of the lines of TEXT (an AGENTS.md): `- name: what it is. `~/path`. `git@…``.
pub fn parse(text: &str) -> Vec<Repo> {
    let home = crate::config::home();
    let mut out: Vec<Repo> = Vec::new();
    for l in text.lines() {
        let Some(rest) = l.trim_start().strip_prefix("- ") else { continue };
        let Some((name, _)) = rest.split_once(':') else { continue };
        let name = name.trim().trim_matches('*').trim();
        if name.is_empty() || name.contains(' ') || name.contains('`') {
            continue;
        }
        let ticks: Vec<&str> = rest.split('`').skip(1).step_by(2).collect();
        let Some(path) = ticks.iter().find(|t| t.starts_with("~/") || t.starts_with('/')) else { continue };
        let path = match path.strip_prefix("~/") {
            Some(r) => home.join(r),
            None => PathBuf::from(path),
        };
        let url = ticks.iter().find(|t| t.starts_with("git@") || t.starts_with("https://") || t.starts_with("ssh://")).map(|s| s.to_string());
        if !out.iter().any(|r| r.path == path) {
            out.push(Repo { name: name.to_string(), path, url });
        }
    }
    out
}

/// The repos of the home repo's AGENTS.md (0 home repo: 0 repos).
pub fn repos() -> Vec<Repo> {
    let Some(home) = crate::config::home_repo() else { return Vec::new() };
    std::fs::read_to_string(home.join("AGENTS.md")).map(|t| parse(&t)).unwrap_or_default()
}

/// Clone URL into PATH (its parent made first). Err: git's 1st error line.
pub fn clone(url: &str, path: &Path) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let out = std::process::Command::new("git").arg("clone").arg("-q").arg(url).arg(path).env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null()).output().map_err(|e| format!("git clone: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("git clone {url}: {}", String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_name_path_and_url_per_line() {
        let text = "# x\n\n## Other repos\n\n### doinstruct\n\n- platform: core apps. `~/projects/doinstruct/platform`. `git@github.com:doinstruct/platform.git`. Notes: `notes/platform/`.\n- security: GHAS replacement. `/abs/security`. `https://github.com/doinstruct/security.git`.\n- 0 path: `notes/x`\n- line with no colon `~/x`\n- dup: again. `~/projects/doinstruct/platform`.\n";
        let r = parse(text);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name, "platform");
        assert!(r[0].path.ends_with("projects/doinstruct/platform") && r[0].path.is_absolute());
        assert_eq!(r[0].url.as_deref(), Some("git@github.com:doinstruct/platform.git"));
        assert_eq!((r[1].path.to_str(), r[1].url.as_deref()), (Some("/abs/security"), Some("https://github.com/doinstruct/security.git")));
    }
}
