use std::ops::ControlFlow;

use super::{Path, Task};

/// The tasks of the project around DIR: the nearest justfile, package.json and Makefile from
/// DIR up to the repo top (else home, else the root).
pub(crate) fn tasks(dir: &Path) -> Vec<Task> {
    let home = crate::infra::config::home();
    let (mut just, mut pkg, mut make) = (None, None, None);

    for directory in dir.ancestors() {
        let find = |names: &[&str]| {
            names
                .iter()
                .map(|count| directory.join(count))
                .find(|path| path.is_file())
        };

        just = just.or_else(|| find(&["justfile", "Justfile", ".justfile"]));
        pkg = pkg.or_else(|| find(&["package.json"]).map(|path_value| (path_value, directory.to_path_buf())));
        make = make.or_else(|| find(&["GNUmakefile", "makefile", "Makefile"]));

        if directory.join(".git").exists() || directory == home {
            break;
        }
    }

    let read = |path_2: &Path| std::fs::read_to_string(path_2).unwrap_or_default();
    let mut out = Vec::new();

    if let Some(path_3) = just {
        out.extend(just_recipes(&read(&path_3)).into_iter().map(|(name, doc)| Task {
            runner: "just",
            run: format!("just {name}"),
            name,
            doc,
        }));
    }

    if let Some((path_4, root)) = pkg {
        let pm = package_manager(&root);

        out.extend(package_scripts(&read(&path_4)).into_iter().map(|(name, doc)| {
            let run = run(pm, &name);

            Task {
                runner: pm,
                run,
                name,
                doc,
            }
        }));
    }

    if let Some(path_5) = make {
        out.extend(make_targets(&read(&path_5)).into_iter().map(|(name, doc)| Task {
            runner: "make",
            run: format!("make {name}"),
            name,
            doc,
        }));
    }

    out
}

fn run(pm: &str, name: &String) -> String {
    if matches!(pm, "pnpm" | "yarn") {
        format!("{pm} {name}")
    } else {
        format!("{pm} run {name}")
    }
}

/// The package manager of the package in DIR (its lockfile, up to the repo top): pnpm, yarn,
/// bun, else npm.
pub(super) fn package_manager(dir: &Path) -> &'static str {
    for directory in dir.ancestors() {
        if let Some(value) = nearest_lockfile(directory) {
            return value;
        }

        if directory.join(".git").exists() {
            break;
        }
    }

    "npm"
}

fn nearest_lockfile(directory: &Path) -> Option<&'static str> {
    for (file, pm) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("pnpm-workspace.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
    ] {
        if directory.join(file).is_file() {
            return Some(pm);
        }
    }

    None
}

/// Recipes of a justfile: (name, doc: the comment line above it or `[doc("…")]`). Private ones
/// (`_name`, `[private]`) left out.
pub(super) fn just_recipes(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let (mut doc, mut private) = (String::new(), false);

    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            continue;
        }

        let thread = line.trim();

        if thread.is_empty() {
            (doc, private) = (String::new(), false);
            continue;
        }

        if let Some(character) = thread.strip_prefix('#') {
            read_recipe_doc_comment(&mut doc, character);

            continue;
        }

        if let Some(agent) = thread.strip_prefix('[') {
            private |= agent.starts_with("private");

            read_recipe_doc_attribute(&mut doc, agent);

            continue;
        }

        let body = thread.strip_prefix('@').unwrap_or(thread);

        let name: String = body
            .chars()
            .take_while(|character_value| {
                character_value.is_alphanumeric() || *character_value == '_' || *character_value == '-'
            })
            .collect();

        let rest = &body[name.len()..];
        let keyword = matches!(name.as_str(), "alias" | "set" | "export" | "import" | "mod") && rest.starts_with(' ');

        // `name args: deps`, not `name := value`
        let colon = rest.find(':').filter(|&i| !rest[i + 1..].starts_with('='));

        if !name.is_empty()
            && !keyword
            && colon.is_some()
            && !rest.contains(":=")
            && !private
            && !name.starts_with('_')
            && !out.iter().any(|(count, _)| *count == name)
        {
            out.push((name, std::mem::take(&mut doc)));
        }

        (doc, private) = (String::new(), false);
    }

    out
}

fn read_recipe_doc_attribute(doc: &mut String, agent: &str) {
    if let Some(directory_value) = agent
        .strip_prefix("doc(")
        .and_then(|directory| directory.split('"').nth(1))
    {
        *doc = directory_value.to_string();
    }
}

fn read_recipe_doc_comment(doc: &mut String, character: &str) {
    if !character.starts_with('!') {
        *doc = character.trim().to_string();
    }
}

/// Scripts of a package.json: (name, command).
pub(super) fn package_scripts(text: &str) -> Vec<(String, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };

    value
        .get("scripts")
        .and_then(|text_value| text_value.as_object())
        .map(|output| {
            output
                .iter()
                .map(|(key, value_value)| (key.clone(), value_value.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Targets of a Makefile: (name, doc: `## doc` after it, else the comment line above it).
/// Special (`.PHONY`), pattern (`%.o`) and variable targets left out.
pub(super) fn make_targets(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut doc = String::new();

    for line in text.lines() {
        if line.starts_with(['\t', ' ']) {
            continue;
        }

        if let Some(character) = line.strip_prefix('#') {
            doc = character.trim_start_matches('#').trim().to_string();
            continue;
        }

        let Some(i) = line.find(':') else {
            doc.clear();
            continue;
        };

        let (head, rest) = (&line[..i], &line[i + 1..]);

        if rest.starts_with('=') || rest.starts_with(':') && rest[1..].starts_with('=') || head.contains('=') {
            doc.clear();
            continue;
        }

        let own = rest.split_once("##").map(|(_, directory)| directory.trim().to_string());

        append_make_targets(&mut out, &doc, head, own.as_deref());

        doc.clear();
    }

    out
}

fn append_make_targets(out: &mut Vec<(String, String)>, doc: &str, head: &str, own: Option<&str>) {
    for name in head.split_whitespace() {
        if let ControlFlow::Break(()) = check_make_target(&*out, name) {
            continue;
        }

        out.push((name.to_string(), own.unwrap_or(doc).to_owned()));
    }
}

fn check_make_target(out: &[(String, String)], name: &str) -> ControlFlow<()> {
    if name.starts_with('.') || name.contains(['%', '$', '(']) || out.iter().any(|(count, _)| count == name) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}
