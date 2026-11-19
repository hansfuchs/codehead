//! The harnesses codehead runs agents on: the coding agents' own CLIs. Claude Code goes over
//! its stream-json mode; every other harness over the Agent Client Protocol (`engine::acp`).
//! `$CODEHEAD_ACP_<NAME>` (or `command` in the agent file) replaces the ACP command.

pub enum Drive {
    /// `claude -p`, stream-json.
    Claude,
    /// An ACP agent on stdio: its command.
    Acp(&'static [&'static str]),
}

pub struct Harness {
    pub name: &'static str,
    pub label: &'static str,
    /// The CLI that must be on PATH.
    pub bin: &'static str,
    pub drive: Drive,
    pub install: &'static str,
}

pub const ALL: [Harness; 13] = [
    Harness { name: "claude", label: "Claude Code", bin: "claude", drive: Drive::Claude, install: "curl -fsSL https://claude.ai/install.sh | bash" },
    Harness { name: "codex", label: "Codex", bin: "codex", drive: Drive::Acp(&["codex-acp"]), install: "npm i -g @openai/codex @zed-industries/codex-acp" },
    Harness { name: "opencode", label: "OpenCode", bin: "opencode", drive: Drive::Acp(&["opencode", "acp"]), install: "npm i -g opencode-ai" },
    Harness { name: "goose", label: "Goose", bin: "goose", drive: Drive::Acp(&["goose", "acp"]), install: "https://goose-docs.ai/docs/getting-started/installation/" },
    Harness { name: "cursor", label: "Cursor", bin: "cursor-agent", drive: Drive::Acp(&["cursor-agent", "acp"]), install: "curl https://cursor.com/install -fsS | bash" },
    Harness { name: "pi", label: "Pi", bin: "pi", drive: Drive::Acp(&["pi-acp"]), install: "npm i -g @earendil-works/pi-coding-agent pi-acp" },
    Harness { name: "omp", label: "Oh My Pi", bin: "omp", drive: Drive::Acp(&["omp", "acp"]), install: "https://omp.sh" },
    Harness { name: "devin", label: "Devin", bin: "devin", drive: Drive::Acp(&["devin", "acp"]), install: "https://docs.devin.ai/cli" },
    Harness { name: "grok", label: "Grok Build", bin: "grok", drive: Drive::Acp(&["grok", "agent", "stdio"]), install: "https://docs.x.ai/build/overview" },
    Harness { name: "kimi", label: "Kimi Code", bin: "kimi", drive: Drive::Acp(&["kimi", "acp"]), install: "https://kimi.ai/download" },
    Harness { name: "amp", label: "Amp", bin: "amp", drive: Drive::Acp(&["amp-acp"]), install: "npm i -g @sourcegraph/amp amp-acp" },
    Harness { name: "hermes", label: "Hermes Agent", bin: "hermes", drive: Drive::Acp(&["hermes-acp"]), install: "https://hermes-agent.nousresearch.com" },
    Harness { name: "openclaw", label: "OpenClaw", bin: "openclaw", drive: Drive::Acp(&["openclaw", "acp"]), install: "https://docs.openclaw.ai/start/getting-started" },
];

/// Harness NAME (`claude`, `cursor`, …).
pub fn get(name: &str) -> Option<&'static Harness> {
    ALL.iter().find(|h| h.name == name)
}

pub fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// The harnesses with their CLI on PATH.
pub fn installed() -> Vec<&'static Harness> {
    ALL.iter().filter(|h| on_path(h.bin)).collect()
}

/// The ACP command of harness H for an agent: the agent's own `command`, else
/// `$CODEHEAD_ACP_<NAME>`, else `[harness.<name>] command`, else the default. None: Claude Code.
pub fn acp_command(h: &Harness, own: Option<&str>) -> Option<Vec<String>> {
    let Drive::Acp(default) = h.drive else { return None };
    let env = std::env::var(format!("CODEHEAD_ACP_{}", h.name.to_uppercase())).ok();
    match own.map(String::from).or(env).or_else(|| crate::config::harness_opt(h.name, "command")).filter(|c| !c.trim().is_empty()) {
        Some(c) => Some(split(&c)),
        None => Some(default.iter().map(|s| s.to_string()).collect()),
    }
}

/// A command line split at spaces, single and double quotes kept together.
pub fn split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() || any {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_and_commands() {
        assert_eq!(ALL.len(), 13);
        assert!(acp_command(get("claude").unwrap(), None).is_none());
        assert_eq!(acp_command(get("grok").unwrap(), None).unwrap(), ["grok", "agent", "stdio"]);
        assert_eq!(acp_command(get("pi").unwrap(), Some("npx 'pi-acp' --x")).unwrap(), ["npx", "pi-acp", "--x"]);
        assert_eq!(split(r#"a "b c" ''"#), ["a", "b c", ""]);
    }
}
