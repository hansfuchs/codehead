//! The harnesses codehead runs agents on: the coding agents' own CLIs. Claude Code goes over
//! its stream-json mode, Codex over `codex app-server` (`engine::codex`), every other harness
//! over the Agent Client Protocol (`engine::acp`). `$CODEHEAD_ACP_<NAME>` (or `command` in the
//! agent file) replaces the ACP command; for Codex it runs that ACP agent instead.

pub(crate) enum Drive {
    /// `claude -p`, stream-json.
    Claude,
    /// `codex app-server`: the CLI's own JSON-RPC protocol.
    Codex,
    /// An ACP agent on stdio: its command.
    Acp(&'static [&'static str]),
}

pub(crate) struct Harness {
    pub name: &'static str,
    pub label: &'static str,
    /// The CLI that must be on PATH.
    pub bin: &'static str,
    pub drive: Drive,
    pub install: &'static str,
}

pub(crate) const ALL: [Harness; 13] = [
    Harness {
        name: "claude",
        label: "Claude Code",
        bin: "claude",
        drive: Drive::Claude,
        install: "curl -fsSL https://claude.ai/install.sh | bash",
    },
    Harness {
        name: "codex",
        label: "Codex",
        bin: "codex",
        drive: Drive::Codex,
        install: "npm i -g @openai/codex",
    },
    Harness {
        name: "opencode",
        label: "OpenCode",
        bin: "opencode",
        drive: Drive::Acp(&["opencode", "acp"]),
        install: "npm i -g opencode-ai",
    },
    Harness {
        name: "goose",
        label: "Goose",
        bin: "goose",
        drive: Drive::Acp(&["goose", "acp"]),
        install: "https://goose-docs.ai/docs/getting-started/installation/",
    },
    Harness {
        name: "cursor",
        label: "Cursor",
        bin: "cursor-agent",
        drive: Drive::Acp(&["cursor-agent", "acp"]),
        install: "curl https://cursor.com/install -fsS | bash",
    },
    Harness {
        name: "pi",
        label: "Pi",
        bin: "pi",
        drive: Drive::Acp(&["pi-acp"]),
        install: "npm i -g @earendil-works/pi-coding-agent pi-acp",
    },
    Harness {
        name: "omp",
        label: "Oh My Pi",
        bin: "omp",
        drive: Drive::Acp(&["omp", "acp"]),
        install: "https://omp.sh",
    },
    Harness {
        name: "devin",
        label: "Devin",
        bin: "devin",
        drive: Drive::Acp(&["devin", "acp"]),
        install: "https://docs.devin.ai/cli",
    },
    Harness {
        name: "grok",
        label: "Grok Build",
        bin: "grok",
        drive: Drive::Acp(&["grok", "agent", "stdio"]),
        install: "https://docs.x.ai/build/overview",
    },
    Harness {
        name: "kimi",
        label: "Kimi Code",
        bin: "kimi",
        drive: Drive::Acp(&["kimi", "acp"]),
        install: "https://kimi.ai/download",
    },
    Harness {
        name: "amp",
        label: "Amp",
        bin: "amp",
        drive: Drive::Acp(&["amp-acp"]),
        install: "npm i -g @sourcegraph/amp amp-acp",
    },
    Harness {
        name: "hermes",
        label: "Hermes Agent",
        bin: "hermes",
        drive: Drive::Acp(&["hermes-acp"]),
        install: "https://hermes-agent.nousresearch.com",
    },
    Harness {
        name: "openclaw",
        label: "OpenClaw",
        bin: "openclaw",
        drive: Drive::Acp(&["openclaw", "acp"]),
        install: "https://docs.openclaw.ai/start/getting-started",
    },
];

/// Harness NAME (`claude`, `cursor`, …).
pub(crate) fn get(name: &str) -> Option<&'static Harness> {
    ALL.iter().find(|harness| harness.name == name)
}

pub(crate) fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(bin).is_file()))
}

/// The harnesses with their CLI on PATH.
pub(crate) fn installed() -> Vec<&'static Harness> {
    ALL.iter().filter(|height| on_path(height.bin)).collect()
}

/// The ACP command of HARNESS for an agent: the agent's own `command`, else
/// `$CODEHEAD_ACP_<NAME>`, else `[harness.<name>] command`, else the default. None: Claude Code,
/// Codex without an ACP command of its own (the app server).
pub(crate) fn acp_command(harness: &Harness, own: Option<&str>) -> Option<Vec<String>> {
    let default: &[&str] = match harness.drive {
        Drive::Claude => return None,
        Drive::Codex => &[],
        Drive::Acp(command) => command,
    };

    let env = std::env::var(format!("CODEHEAD_ACP_{}", harness.name.to_uppercase())).ok();

    match own
        .map(String::from)
        .or(env)
        .or_else(|| crate::infra::config::harness_opt(harness.name, "command"))
        .filter(|command| !command.trim().is_empty())
    {
        Some(command) => Some(split(&command)),
        None if default.is_empty() => None,
        None => Some(default.iter().map(std::string::ToString::to_string).collect()),
    }
}

/// A command line split at spaces, single and double quotes kept together.
pub(crate) fn split(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut quoted = false;

    for character in text.chars() {
        match (quote, character) {
            (Some(open), closing) if closing == open => quote = None,
            (Some(_), inside) => current.push(inside),
            (None, '\'' | '"') => {
                quote = Some(character);
                quoted = true;
            }
            (None, space) if space.is_whitespace() => {
                push_shell_argument(&mut out, &mut current, &mut quoted);
            }
            (None, plain) => current.push(plain),
        }
    }

    if !current.is_empty() || quoted {
        out.push(current);
    }

    out
}

fn push_shell_argument(out: &mut Vec<String>, current: &mut String, quoted: &mut bool) {
    if !current.is_empty() || *quoted {
        out.push(std::mem::take(current));
    }

    *quoted = false;
}

#[cfg(test)]
#[path = "harnesses_tests.rs"]
mod tests;
