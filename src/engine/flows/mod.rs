//! Flows: the steps an agent takes for a kind of task (new feature, bug fix, refactor, ...).
//!
//! - step: 1 Markdown file, front matter (`description`, `gate`, `done`, `needs`) + the
//!   instruction text. Its file stem is its name. Flows share steps: an edit reaches each flow.
//! - flow: 1 TOML file, `[[step]]` nodes in order. A node uses a step (`use`) or holds a one-off
//!   text (`text`); `gate`, `loop`, `label`, `note` per node. Every step runs on the agent the
//!   flow belongs to (roles are gone: an old `role` key gives a warning, 0 effect).
//! - layers, the last wins per name: built-in (in the binary) → user (`<config>/flows`,
//!   `<config>/steps`) → repo (`<repo>/.codehead/flows`, `.codehead/steps`). An edit of a built-in
//!   writes a user file over it; reset removes that file.
//! - an agent's flow (`FlowState` in `thread.json`, 1 per agent of a thread): the current node,
//!   loop rounds, a wait (gate, loop max), a queued switch. The host moves it: the agent reports
//!   a step done (`flow_step`), the owner answers gates, skips, jumps, switches.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub(crate) mod run;

/// Where a step or a flow comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Layer {
    #[default]
    Builtin,
    User,
    Repo,
}

impl Layer {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Layer::Builtin => "built-in",
            Layer::User => "user",
            Layer::Repo => "repo",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Step {
    /// File stem: letters, digits, `-`, `_`.
    pub name: String,
    pub description: String,
    /// A `role:` line of an old step file: 0 effect (the step runs on the flow's agent), a warning.
    pub old_role: Option<String>,
    pub gate: bool,
    /// Exit criteria.
    pub done: String,
    /// What the agent of the step needs: `skill:<name>`, `mcp:<server>`, `harness:<name>`.
    pub needs: Vec<String>,
    pub text: String,
    pub layer: Layer,
    /// A user / repo file over a built-in of the same name.
    pub edited: bool,
}

/// A node's gate: `gate = true` / `false`, or `gate = { changes = "plan" }` (more outcomes than
/// approve → next: outcome → node id).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Gate {
    Flag(bool),
    Edges(BTreeMap<String, String>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Loop {
    /// Node id the loop goes back to (a node above).
    pub to: String,
    /// Exit criteria of the loop: false → back to TO.
    #[serde(default)]
    pub until: String,
    #[serde(default = "three")]
    pub max: u32,
}

fn three() -> u32 {
    3
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Node {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub uses: Option<String>,
    /// One-off step: its instruction text (0 `use`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Flow-specific text after the step's text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// An old flow file's node role: 0 effect, a warning; never written again.
    #[serde(default, skip_serializing)]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub loops: Option<Loop>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Flow {
    /// File stem.
    #[serde(skip)]
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// An old flow file's roles: 0 effect, never written again.
    #[serde(default, skip_serializing)]
    pub roles: Vec<String>,
    #[serde(default, rename = "step")]
    pub steps: Vec<Node>,
    #[serde(skip)]
    pub layer: Layer,
    #[serde(skip)]
    pub edited: bool,
}

/// A node with its step filled in: what the runtime and the views use.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Resolved {
    /// Node id: `id`, else `use`, else `step-N` (unique: `-2`, `-3` on repeats).
    pub id: String,
    pub title: String,
    /// The shared step it uses (None: one-off).
    pub step: Option<String>,
    pub text: String,
    pub done: String,
    pub gate: bool,
    /// Gate outcomes besides approve → next: (outcome, node index).
    pub edges: Vec<(String, usize)>,
    /// Loop: (target index, until, max).
    pub loops: Option<(usize, String, u32)>,
    pub needs: Vec<String>,
    /// Problems: a missing step, a loop / edge target that is not there.
    pub errors: Vec<String>,
}

// ---------- steps: Markdown + front matter ----------

// ---------- layers ----------

const BUILTIN_STEPS: &[(&str, &str)] = &[
    ("scrutinize-task", include_str!("builtin/steps/scrutinize-task.md")),
    ("open-questions", include_str!("builtin/steps/open-questions.md")),
    ("reproduce", include_str!("builtin/steps/reproduce.md")),
    ("investigate", include_str!("builtin/steps/investigate.md")),
    ("options", include_str!("builtin/steps/options.md")),
    ("plan", include_str!("builtin/steps/plan.md")),
    ("owner-gate", include_str!("builtin/steps/owner-gate.md")),
    ("implement", include_str!("builtin/steps/implement.md")),
    ("verify", include_str!("builtin/steps/verify.md")),
    ("peer-review", include_str!("builtin/steps/peer-review.md")),
    ("review-diff", include_str!("builtin/steps/review-diff.md")),
    ("address-review", include_str!("builtin/steps/address-review.md")),
    ("measure", include_str!("builtin/steps/measure.md")),
    ("write-up", include_str!("builtin/steps/write-up.md")),
    ("report", include_str!("builtin/steps/report.md")),
];

const BUILTIN_FLOWS: &[(&str, &str)] = &[
    ("new-feature", include_str!("builtin/flows/new-feature.toml")),
    ("bug-fix", include_str!("builtin/flows/bug-fix.toml")),
    (
        "bug-investigation",
        include_str!("builtin/flows/bug-investigation.toml"),
    ),
    ("refactor", include_str!("builtin/flows/refactor.toml")),
    ("concept", include_str!("builtin/flows/concept.toml")),
    ("performance", include_str!("builtin/flows/performance.toml")),
    ("migration", include_str!("builtin/flows/migration.toml")),
    ("hotfix", include_str!("builtin/flows/hotfix.toml")),
    ("code-review", include_str!("builtin/flows/code-review.toml")),
    ("chore", include_str!("builtin/flows/chore.toml")),
];

/// User layer dir: `<shared>/flows` or `<shared>/steps` (tests: the test data dir).
pub(crate) fn user_dir(kind: &str) -> PathBuf {
    if cfg!(test) {
        return super::data_dir().join("flowcfg").join(kind);
    }

    super::shared_dir().join(kind)
}

/// Repo root of DIR: the nearest dir with `.git` above it.
pub(crate) fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|directory| directory.join(".git").exists())
        .map(Path::to_path_buf)
}

pub(crate) fn repo_dir(root: &Path, kind: &str) -> PathBuf {
    root.join(".codehead").join(kind)
}

pub(crate) fn dir_of(layer: Layer, kind: &str, repo: Option<&Path>) -> Option<PathBuf> {
    match layer {
        Layer::Builtin => None,
        Layer::User => Some(user_dir(kind)),
        Layer::Repo => repo.map(|result| repo_dir(result, kind)),
    }
}

fn files(dir: &Path, ext: &str) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut value: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|error| {
            let path_value = error.path();

            if path_value.extension()?.to_str()? != ext {
                return None;
            }

            Some((
                path_value.file_stem()?.to_str()?.to_string(),
                std::fs::read_to_string(&path_value).ok()?,
            ))
        })
        .collect();

    value.sort();
    value
}

/// Every step and flow of the layers (built-in, user, repo of REPO), the last layer wins per
/// name. Flows: built-ins in their order first, then the rest by name.
#[derive(Clone, Debug, Default)]
pub(crate) struct Library {
    pub steps: Vec<Step>,
    pub flows: Vec<Flow>,
    /// Files that did not parse: (path, error).
    pub errors: Vec<(String, String)>,
    pub repo: Option<PathBuf>,
}

impl Library {
    /// The layers for work dir DIR (None: built-in + user).
    pub(crate) fn load(dir: Option<&Path>) -> Library {
        let repo = dir.and_then(repo_root);
        let mut lib = Library {
            repo: repo.clone(),
            ..Default::default()
        };

        for (count, thread) in BUILTIN_STEPS {
            lib.steps.push(parse_step(count, thread));
        }

        for (count_value, thread_value) in BUILTIN_FLOWS {
            match parse_flow(count_value, thread_value) {
                Ok(file) => lib.flows.push(file),
                Err(error) => lib.errors.push((format!("built-in {count_value}"), error)),
            }
        }

        let mut layers = vec![(Layer::User, user_dir("steps"), user_dir("flows"))];

        if let Some(result) = &repo {
            layers.push((Layer::Repo, repo_dir(result, "steps"), repo_dir(result, "flows")));
        }

        load_layers(&mut lib, layers);

        let builtin = BUILTIN_FLOWS.len().min(lib.flows.len());

        lib.flows[builtin..].sort_by_key(|file_2| file_2.name.to_lowercase());
        lib.steps.sort_by(|agent, bytes| agent.name.cmp(&bytes.name));

        lib
    }

    pub(crate) fn step(&self, name: &str) -> Option<&Step> {
        self.steps.iter().find(|text| text.name == name)
    }

    /// Flow by slug, else by name (any case).
    pub(crate) fn flow(&self, key: &str) -> Option<&Flow> {
        self.flows.iter().find(|file| file.slug == key).or_else(|| {
            self.flows
                .iter()
                .find(|file_value| file_value.name.eq_ignore_ascii_case(key))
        })
    }

    /// The flows that use step NAME.
    pub(crate) fn users_of(&self, name: &str) -> Vec<&Flow> {
        self.flows
            .iter()
            .filter(|file| file.steps.iter().any(|count| count.uses.as_deref() == Some(name)))
            .collect()
    }

    pub(crate) fn is_builtin_step(name: &str) -> bool {
        BUILTIN_STEPS.iter().any(|(count, _)| *count == name)
    }

    /// The layer an edit of LAYER writes to: built-in → user.
    fn write_layer(layer: Layer) -> Layer {
        if layer == Layer::Repo { Layer::Repo } else { Layer::User }
    }

    /// Write step S into its layer (built-in → user).
    pub(crate) fn save_step(&self, text: &Step) -> Result<PathBuf, String> {
        if !valid_name(&text.name) {
            return Err(format!("step name {:?}: letters, digits, - and _ only", text.name));
        }

        let dir =
            dir_of(Self::write_layer(text.layer), "steps", self.repo.as_deref()).ok_or("0 repo for a repo step")?;

        std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = dir.join(format!("{}.md", text.name));

        super::write_atomic(&path, render_step(text).as_bytes())
            .map_err(|error_value| format!("{}: {error_value}", path.display()))?;

        Ok(path)
    }

    pub(crate) fn save_flow(&self, file: &Flow) -> Result<PathBuf, String> {
        if !valid_name(&file.slug) {
            return Err(format!("flow file name {:?}: letters, digits, - and _ only", file.slug));
        }

        let dir =
            dir_of(Self::write_layer(file.layer), "flows", self.repo.as_deref()).ok_or("0 repo for a repo flow")?;

        std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = dir.join(format!("{}.toml", file.slug));

        super::write_atomic(&path, render_flow(file)?.as_bytes())
            .map_err(|error_value| format!("{}: {error_value}", path.display()))?;

        Ok(path)
    }

    /// Remove the file of step NAME in LAYER (a built-in: back to the built-in text).
    pub(crate) fn remove_step_file(&self, name: &str, layer: Layer) -> Result<(), String> {
        let dir = dir_of(layer, "steps", self.repo.as_deref()).ok_or("a built-in step has 0 file")?;
        let path = dir.join(format!("{name}.md"));

        std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub(crate) fn remove_flow_file(&self, slug: &str, layer: Layer) -> Result<(), String> {
        let dir = dir_of(layer, "flows", self.repo.as_deref()).ok_or("a built-in flow has 0 file")?;
        let path = dir.join(format!("{slug}.toml"));

        std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// A free name: BASE, else BASE-2, BASE-3, ...
    pub(crate) fn free_step_name(&self, base: &str) -> String {
        free(base, |count| self.step(count).is_some())
    }

    pub(crate) fn free_flow_slug(&self, base: &str) -> String {
        free(base, |count| self.flows.iter().any(|file| file.slug == count))
    }

    /// Rename step OLD to NEW: the file moves, each flow that uses it (user / repo layer; a
    /// built-in flow gets a user copy) points to NEW.
    pub(crate) fn rename_step(&self, old: &str, new: &str) -> Result<(), String> {
        if !valid_name(new) {
            return Err(format!("step name {new:?}: letters, digits, - and _ only"));
        }

        if self.step(new).is_some() {
            return Err(format!("step {new} exists already"));
        }

        let mut text = self.step(old).cloned().ok_or_else(|| format!("no step {old}"))?;

        if Self::is_builtin_step(old) {
            return Err("a built-in step keeps its name: duplicate it".into());
        }

        text.name = new.to_string();
        self.save_step(&text)?;
        self.remove_step_file(old, text.layer)?;

        for file in self.users_of(old) {
            let mut file_value = file.clone();

            rename_step_uses(old, new, &mut file_value);

            self.save_flow(&file_value)?;
        }

        Ok(())
    }

    /// Delete step NAME: each flow that uses it gets the text as a one-off node.
    pub(crate) fn delete_step(&self, name: &str) -> Result<usize, String> {
        let text = self.step(name).cloned().ok_or_else(|| format!("no step {name}"))?;
        let users: Vec<Flow> = self.users_of(name).into_iter().cloned().collect();

        for mut file in users.iter().cloned() {
            inline_removed_step(name, &text, &mut file);

            self.save_flow(&file)?;
        }

        match text.layer {
            Layer::Builtin => return Err("a built-in step stays: edit it, or remove it from the flows".into()),
            line => self.remove_step_file(name, line)?,
        }

        Ok(users.len())
    }
}

fn load_layers(lib: &mut Library, layers: Vec<(Layer, PathBuf, PathBuf)>) {
    for (layer, sd, fd) in layers {
        load_steps(lib, layer, &sd);

        load_flows(lib, layer, &fd);
    }
}

fn load_flows(lib: &mut Library, layer: Layer, fd: &Path) {
    for (count_3, thread_3) in files(fd, "toml") {
        match parse_flow(&count_3, &thread_3) {
            Ok(file_value) => {
                replace_flow(lib, layer, &count_3, file_value);
            }
            Err(error_value) => lib
                .errors
                .push((fd.join(format!("{count_3}.toml")).display().to_string(), error_value)),
        }
    }
}

fn replace_flow(lib: &mut Library, layer: Layer, count_3: &String, mut file_value: Flow) {
    file_value.layer = layer;

    match lib.flows.iter().position(|x| x.slug == *count_3) {
        Some(i) => {
            file_value.edited = lib.flows[i].layer == Layer::Builtin || lib.flows[i].edited;
            lib.flows[i] = file_value;
        }
        None => lib.flows.push(file_value),
    }
}

fn load_steps(lib: &mut Library, layer: Layer, sd: &Path) {
    for (count_2, thread_2) in files(sd, "md") {
        let mut text = parse_step(&count_2, &thread_2);

        text.layer = layer;

        match lib.steps.iter().position(|x| x.name == count_2) {
            Some(i) => {
                text.edited = lib.steps[i].layer == Layer::Builtin || lib.steps[i].edited;
                lib.steps[i] = text;
            }
            None => lib.steps.push(text),
        }
    }
}

fn inline_removed_step(name: &str, text: &Step, file: &mut Flow) {
    for count in &mut file.steps {
        if count.uses.as_deref() == Some(name) {
            *count = inline(count, text);
        }
    }
}

fn rename_step_uses(old: &str, new: &str, file_value: &mut Flow) {
    for count in &mut file_value.steps {
        if count.uses.as_deref() == Some(old) {
            count.uses = Some(new.to_string());
        }
    }
}

fn free(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }

    let mut key = 2;

    loop {
        let candidate = format!("{base}-{key}");

        if !taken(&candidate) {
            return candidate;
        }

        key += 1;
    }
}

#[cfg(test)]
#[path = "flows_tests.rs"]
mod tests;

#[path = "format.rs"]
mod format;
pub(crate) use format::*;

#[path = "resolve.rs"]
mod resolve;
pub(crate) use resolve::*;
