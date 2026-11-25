//! Settings › harnesses: 1 group per installed harness, its defaults for codehead's agents
//! (`[harness.<name>]` in codehead.toml). An agent's own value (agent form) wins.

use std::path::PathBuf;

use crate::engine::harnesses::{self, Drive, Harness};
use crate::features::settings::{Form, Spec};

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The fields of harness H.
fn fields(harness: &Harness) -> Vec<Spec> {
    let key = |setting: &str| leak(format!("harness.{}.{setting}", harness.name));

    match harness.drive {
        Drive::Claude => vec![
            Spec::new(
                "models",
                "models",
                "models of the agent form, comma-separated",
                "CODEHEAD_MODELS",
            ),
            Spec::new(key("model"), "model", "model of agents with 0 model of their own", ""),
            Spec::new(
                key("effort"),
                "effort",
                "low, medium, high, xhigh or max, for agents with 0 effort of their own",
                "",
            ),
            Spec::new(
                key("args"),
                "arguments",
                "extra arguments of every `claude` session, e.g. --add-dir ~/x",
                "",
            ),
        ],
        Drive::Codex => vec![
            Spec::new(key("model"), "model", "model of agents with 0 model of their own", ""),
            Spec::new(
                key("effort"),
                "effort",
                "reasoning effort of the model (low, medium, high, …), for agents with 0 effort of their own",
                "",
            ),
            Spec::new(
                key("command"),
                "command",
                "an ACP command instead of the app server, for agents with 0 command of their own",
                "",
            ),
        ],
        Drive::Acp(_) => vec![
            Spec::new(key("model"), "model", "model of agents with 0 model of their own", ""),
            Spec::new(
                key("command"),
                "command",
                "ACP command (args included) of agents with 0 command of their own",
                "",
            ),
        ],
    }
}

/// The installed harnesses, in registry order.
fn shown() -> Vec<&'static Harness> {
    if cfg!(test) {
        harnesses::ALL.iter().take(2).collect()
    } else {
        harnesses::installed()
    }
}

/// The harnesses tab's form.
pub(crate) fn form(path: PathBuf) -> Form {
    let mut specs = Vec::new();
    let mut sections = Vec::new();

    for harness in shown() {
        let how = match harness.drive {
            Drive::Claude => "stream-json",
            Drive::Codex => "app server",
            Drive::Acp(_) => "ACP",
        };

        let at = which(harness.bin).unwrap_or_else(|| harness.bin.to_string());

        sections.push((
            specs.len(),
            harness.label,
            leak(format!("{at} · {how} · agent form values win")),
        ));
        specs.extend(fields(harness));
    }

    Form::with(path, specs, sections, false, 0)
}

fn which(binary: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;

    std::env::split_paths(&path)
        .map(|directory| directory.join(binary))
        .find(|candidate| candidate.is_file())
        .map(|found| found.display().to_string())
}

/// The placeholder of harness field KEY: what applies while it is empty.
pub(crate) fn default_of(key: &str) -> Option<String> {
    let (name, field) = key.strip_prefix("harness.")?.split_once('.')?;
    let harness = harnesses::get(name)?;

    Some(match (field, &harness.drive) {
        ("command", Drive::Acp(command)) => command.join(" "),
        ("command", Drive::Codex) => "codex app-server".into(),
        ("args", _) => "none".into(),
        _ => format!("{}'s own", harness.label),
    })
}

/// Harnesses not on PATH: (label, install command), for the tab's foot.
pub(crate) fn missing() -> Vec<(&'static str, &'static str)> {
    harnesses::ALL
        .iter()
        .filter(|harness| !harnesses::on_path(harness.bin))
        .map(|harness| (harness.label, harness.install))
        .collect()
}

#[cfg(test)]
#[path = "settings_harness_tab_tests.rs"]
mod tests;
