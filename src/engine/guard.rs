//! What an ACP agent in autopilot may run without the owner: everything but the calls that are
//! hard to take back. Those wait for an answer, like in supervised mode.

use serde_json::Value;

/// Shell commands that destroy work or reach past the project.
const RISKY: [&str; 22] = [
    r"\brm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\s+)+(/|~|\$HOME|\.\.|\*)",
    r"\bgit\s+push\b.*(\s-f\b|--force)",
    r"\bgit\s+reset\s+--hard\b",
    r"\bgit\s+clean\s+-[a-zA-Z]*f",
    r"\bgit\s+branch\s+-D\b",
    r"\bgit\s+(checkout|restore)\s+(--\s+)?\.\s*$",
    r"\bgit\s+filter-(branch|repo)\b",
    r"\bgit\s+stash\s+(drop|clear)\b",
    r"\bsudo\b",
    r"\bdd\s+.*\bof=",
    r"\bmkfs\b",
    r"\bchmod\s+-R\s+777\b",
    r"\bchown\s+-R\b",
    r"(curl|wget)\b.*\|\s*(sh|bash|zsh)\b",
    r"\b(shutdown|reboot|halt|poweroff)\b",
    r"(?i)\bdrop\s+(database|table)\b",
    r">\s*/dev/sd",
    r"\bkill\s+-9\s+-1\b",
    r"\bnpm\s+publish\b",
    r"\bcargo\s+publish\b",
    r"\bgh\s+(repo\s+delete|release\s+delete)\b",
    r"\bterraform\s+destroy\b",
];

fn patterns() -> &'static Vec<regex::Regex> {
    static P: std::sync::OnceLock<Vec<regex::Regex>> = std::sync::OnceLock::new();
    P.get_or_init(|| RISKY.iter().filter_map(|p| regex::Regex::new(p).ok()).collect())
}

/// A shell command of INPUT that is hard to take back.
pub fn risky(input: &Value) -> bool {
    let cmd = input["command"]
        .as_str()
        .map(String::from)
        .or_else(|| input["command"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")))
        .unwrap_or_default();
    !cmd.is_empty() && patterns().iter().any(|p| p.is_match(&cmd))
}

/// The host answers this permission request of an ACP agent in MODE itself: TOOL its name,
/// KIND the ACP kind (read, edit, execute, …).
pub fn auto_allow(mode: &str, tool: &str, kind: &str, input: &Value) -> bool {
    if tool.contains("codehead") {
        return true;
    }
    let safe = matches!(kind, "read" | "search" | "think");
    match mode {
        "supervised" | "plan" => safe,
        "edits" => safe || kind == "edit",
        _ => !risky(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn guard_and_modes() {
        assert!(risky(&json!({"command": "rm -rf ~/x"})));
        assert!(risky(&json!({"command": ["git", "reset", "--hard"]})));
        assert!(risky(&json!({"command": "sudo make install"})));
        assert!(!risky(&json!({"command": "rm -rf target"})));
        assert!(!risky(&json!({"command": "cargo test"})));
        assert!(auto_allow("autopilot", "Bash", "execute", &json!({"command": "ls"})));
        assert!(!auto_allow("autopilot", "Bash", "execute", &json!({"command": "sudo ls"})));
        assert!(auto_allow("edits", "Edit", "edit", &json!({})));
        assert!(!auto_allow("edits", "Bash", "execute", &json!({"command": "ls"})));
        assert!(!auto_allow("supervised", "Edit", "edit", &json!({})));
        assert!(auto_allow("supervised", "codehead_send", "other", &json!({})));
    }
}
