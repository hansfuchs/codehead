//! Settings › harnesses: 1 group per installed harness, its defaults for codehead's agents
//! (`[harness.<name>]` in codehead.toml). An agent's own value (agent form) wins.

use std::path::PathBuf;

use crate::engine::harnesses::{self, Drive, Harness};
use crate::settings::{Form, Spec};

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The fields of harness H.
fn fields(h: &Harness) -> Vec<Spec> {
    let key = |f: &str| leak(format!("harness.{}.{f}", h.name));
    match h.drive {
        Drive::Claude => vec![
            Spec::new("models", "models", "models of the agent form, comma-separated", "CODEHEAD_MODELS"),
            Spec::new(key("model"), "model", "model of agents with 0 model of their own", ""),
            Spec::new(key("effort"), "effort", "low, medium, high, xhigh or max, for agents with 0 effort of their own", ""),
            Spec::new(key("args"), "arguments", "extra arguments of every `claude` session, e.g. --add-dir ~/x", ""),
        ],
        Drive::Acp(_) => vec![
            Spec::new(key("model"), "model", "model of agents with 0 model of their own", ""),
            Spec::new(key("command"), "command", "ACP command (args included) of agents with 0 command of their own", ""),
        ],
    }
}

/// The installed harnesses, in registry order.
fn shown() -> Vec<&'static Harness> {
    if cfg!(test) { harnesses::ALL.iter().take(2).collect() } else { harnesses::installed() }
}

/// The harnesses tab's form.
pub fn form(path: PathBuf) -> Form {
    let mut specs = Vec::new();
    let mut sections = Vec::new();
    for h in shown() {
        let how = match h.drive {
            Drive::Claude => "stream-json",
            Drive::Acp(_) => "ACP",
        };
        let at = which(h.bin).unwrap_or_else(|| h.bin.to_string());
        sections.push((specs.len(), h.label, leak(format!("{at} · {how} · agent form values win"))));
        specs.extend(fields(h));
    }
    Form::with(path, specs, sections, false, 0)
}

fn which(bin: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(bin)).find(|p| p.is_file()).map(|p| p.display().to_string())
}

/// The placeholder of harness field KEY: what applies while it is empty.
pub fn default_of(key: &str) -> Option<String> {
    let (name, field) = key.strip_prefix("harness.")?.split_once('.')?;
    let h = harnesses::get(name)?;
    Some(match (field, &h.drive) {
        ("command", Drive::Acp(cmd)) => cmd.join(" "),
        ("args", _) => "none".into(),
        _ => format!("{}'s own", h.label),
    })
}

/// Harnesses not on PATH: (label, install command), for the tab's foot.
pub fn missing() -> Vec<(&'static str, &'static str)> {
    harnesses::ALL.iter().filter(|h| !harnesses::on_path(h.bin)).map(|h| (h.label, h.install)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_name_the_harness_and_its_command() {
        assert_eq!(default_of("harness.codex.command").as_deref(), Some("codex-acp"));
        assert_eq!(default_of("harness.claude.model").as_deref(), Some("Claude Code's own"));
        assert_eq!(default_of("harness.claude.args").as_deref(), Some("none"));
        assert_eq!(default_of("notes"), None);
        assert_eq!(default_of("harness.nope.model"), None);
    }

    #[test]
    fn harness_fields_round_trip_through_the_config_file() {
        let dir = std::env::temp_dir().join(format!("codehead-harnesstab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("codehead.toml");
        std::fs::write(&path, "notes = '~/n'\n[aliases]\nd = '~/d/'\n").unwrap();
        let (base, mut file) = crate::config::read_file(&path).unwrap();
        file.set("harness.codex.model", Some("gpt-5".into()));
        file.set("harness.claude.effort", Some("high".into()));
        let text = crate::config::save_file(&path, &base, &file).unwrap();
        assert!(text.contains("[harness.codex]") && text.contains("model = \"gpt-5\""), "{text}");
        // the other keys stay
        assert!(text.contains("notes = '~/n'") && text.contains("[aliases]"), "{text}");
        let (base, mut back) = crate::config::read_file(&path).unwrap();
        assert_eq!(back.get("harness.claude.effort").as_deref(), Some("high"));
        back.set("harness.codex.model", None);
        back.set("harness.claude.effort", None);
        let text = crate::config::save_file(&path, &base, &back).unwrap();
        assert!(!text.contains("harness"), "{text}");
        assert!(crate::config::validate("harness.claude.effort".into(), "huge").is_err());
        assert!(crate::config::validate("harness.nope.model".into(), "x").is_err());
        assert!(crate::config::validate("harness.codex.color".into(), "x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
