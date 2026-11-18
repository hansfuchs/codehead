//! portless (vercel-labs/portless): dev servers at stable `https://<name>.localhost` URLs, 1 per
//! worktree (the branch's last segment as a subdomain). Setting `ports`: `auto` (default:
//! portless when it is on PATH), `portless`, `block` (only the $CODEHEAD_PORT block). The port
//! block stays for databases and other non-HTTP services.

use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    Portless,
    Block,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim() {
            "auto" => Some(Mode::Auto),
            "portless" => Some(Mode::Portless),
            "block" => Some(Mode::Block),
            _ => None,
        }
    }
}

/// portless on PATH.
pub fn on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("portless").is_file()))
}

/// Agents start dev servers through portless (setting `ports`, portless on PATH).
pub fn active() -> bool {
    crate::config::ports() != Mode::Block && on_path()
}

/// The worktree label of BRANCH as portless makes it: its last segment, lower case, letters,
/// digits and `-` (others: `-`), trimmed of `-`.
pub fn prefix(branch: &str) -> String {
    let last = branch.rsplit('/').next().unwrap_or(branch).to_lowercase();
    let s: String = last.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.trim_matches('-').chars().take(63).collect()
}

/// URLs of the live routes (`portless list --json`): (hostname, url).
pub fn routes() -> Vec<(String, String)> {
    let Ok(out) = Command::new("portless").args(["list", "--json"]).stdin(Stdio::null()).stderr(Stdio::null()).output() else { return Vec::new() };
    parse_routes(&String::from_utf8_lossy(&out.stdout))
}

fn parse_routes(json: &str) -> Vec<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap_or_default();
    v["routes"].as_array().into_iter().flatten().filter_map(|r| Some((r["hostname"].as_str()?.to_string(), r["url"].as_str()?.to_string()))).collect()
}

/// The URLs of ROUTES that belong to worktree label PREFIX (`<prefix>.<app>.localhost`).
pub fn urls_of(routes: &[(String, String)], prefix: &str) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    routes.iter().filter(|(h, _)| h.starts_with(&format!("{prefix}."))).map(|(_, u)| u.clone()).collect()
}

/// The system prompt lines for an agent in a worktree on BRANCH.
pub fn prompt(branch: Option<&str>) -> String {
    let p = branch.map(prefix).filter(|p| !p.is_empty());
    let host = p.as_deref().map_or("<app>.localhost".to_string(), |p| format!("{p}.<app>.localhost"));
    format!(
        "\nDev servers: start them through portless (`portless run <command>`, e.g. `portless run pnpm dev`): it picks a free port (PORT) and serves the app at {host} ($PORTLESS_PREFIX names this worktree; `portless get <app>` prints the full URL). Never pick HTTP ports by hand; give the owner the URL. Use $CODEHEAD_PORT only for non-HTTP services (databases, queues)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_the_branch_tail_as_a_dns_label() {
        assert_eq!(prefix("task/fix-ui"), "fix-ui");
        assert_eq!(prefix("task/Fix_UI.v2"), "fix-ui-v2");
        assert_eq!(prefix("main"), "main");
    }

    #[test]
    fn urls_of_a_worktree_from_the_route_list() {
        let r = parse_routes(r#"{"routes":[{"hostname":"fix-ui.shop.localhost","url":"https://fix-ui.shop.localhost","port":4123},{"hostname":"shop.localhost","url":"https://shop.localhost","port":4001},{"hostname":"api.fix-ui.shop.localhost","url":"https://api.fix-ui.shop.localhost"}]}"#);
        assert_eq!(r.len(), 3);
        assert_eq!(urls_of(&r, "fix-ui"), ["https://fix-ui.shop.localhost"]);
        assert!(urls_of(&r, "").is_empty());
        assert!(prompt(Some("task/fix-ui")).contains("fix-ui.<app>.localhost"));
    }
}
