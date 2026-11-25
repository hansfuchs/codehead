//! portless (vercel-labs/portless): dev servers at stable `https://<name>.localhost` URLs, 1 per
//! worktree (the branch's last segment as a subdomain). Setting `ports`: `auto` (default:
//! portless when it is on PATH), `portless`, `block` (only the $`CODEHEAD_PORT` block). The port
//! block stays for databases and other non-HTTP services.

use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Auto,
    Portless,
    Block,
}

impl Mode {
    pub(crate) fn parse(text: &str) -> Option<Mode> {
        match text.trim() {
            "auto" => Some(Mode::Auto),
            "portless" => Some(Mode::Portless),
            "block" => Some(Mode::Block),
            _ => None,
        }
    }
}

/// portless on PATH.
pub(crate) fn on_path() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("portless").is_file()))
}

/// Agents start dev servers through portless (setting `ports`, portless on PATH).
pub(crate) fn active() -> bool {
    crate::infra::config::ports() != Mode::Block && on_path()
}

/// The worktree label of BRANCH as portless makes it: its last segment, lower case, letters,
/// digits and `-` (others: `-`), trimmed of `-`.
pub(crate) fn prefix(branch: &str) -> String {
    let last = branch.rsplit('/').next().unwrap_or(branch).to_lowercase();

    let label: String = last
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();

    label.trim_matches('-').chars().take(63).collect()
}

/// URLs of the live routes (`portless list --json`): (hostname, url).
pub(crate) fn routes() -> Vec<(String, String)> {
    let Ok(out) = Command::new("portless")
        .args(["list", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };

    parse_routes(&String::from_utf8_lossy(&out.stdout))
}

fn parse_routes(json: &str) -> Vec<(String, String)> {
    let root: serde_json::Value = serde_json::from_str(json.trim()).unwrap_or_default();

    root["routes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|route| {
            Some((
                route["hostname"].as_str()?.to_string(),
                route["url"].as_str()?.to_string(),
            ))
        })
        .collect()
}

/// The URLs of ROUTES that belong to worktree label PREFIX (`<prefix>.<app>.localhost`).
pub(crate) fn urls_of(routes: &[(String, String)], prefix: &str) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }

    routes
        .iter()
        .filter(|(hostname, _)| hostname.starts_with(&format!("{prefix}.")))
        .map(|(_, url)| url.clone())
        .collect()
}

/// The system prompt lines for an agent in a worktree on BRANCH.
pub(crate) fn prompt(branch: Option<&str>) -> String {
    let label = branch.map(prefix).filter(|label| !label.is_empty());

    let host = label.as_deref().map_or("<app>.localhost".to_string(), |label| {
        format!("{label}.<app>.localhost")
    });

    format!(
        "\nDev servers: start them through portless (`portless run <command>`, e.g. `portless run pnpm dev`): it picks a free port (PORT) and serves the app at {host} ($PORTLESS_PREFIX names this worktree; `portless get <app>` prints the full URL). Never pick HTTP ports by hand; give the owner the URL. Use $CODEHEAD_PORT only for non-HTTP services (databases, queues)."
    )
}

#[cfg(test)]
#[path = "portless_tests.rs"]
mod tests;
