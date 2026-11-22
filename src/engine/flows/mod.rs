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

pub mod run;

/// Where a step or a flow comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    #[default]
    Builtin,
    User,
    Repo,
}

impl Layer {
    pub fn label(self) -> &'static str {
        match self {
            Layer::Builtin => "built-in",
            Layer::User => "user",
            Layer::Repo => "repo",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
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
pub enum Gate {
    Flag(bool),
    Edges(BTreeMap<String, String>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loop {
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
pub struct Node {
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
pub struct Flow {
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
pub struct Resolved {
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

/// A valid step / flow / node id name.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// NAME as a file stem: lower case, runs of other chars → `-`.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            s.extend(c.to_lowercase());
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "flow".into() } else { s }
}

// ---------- steps: Markdown + front matter ----------

/// Step NAME from TEXT (front matter + body).
pub fn parse_step(name: &str, text: &str) -> Step {
    let mut s = Step { name: name.to_string(), ..Default::default() };
    let text = text.replace("\r\n", "\n");
    let body = match text.strip_prefix("---\n") {
        Some(rest) => match if rest.starts_with("---") { Some(0) } else { rest.find("\n---").map(|e| e + 1) } {
            Some(end) => {
                for l in rest[..end].lines() {
                    let Some((k, v)) = l.split_once(':') else { continue };
                    let v = unquote(v.trim());
                    match k.trim() {
                        "description" => s.description = v.to_string(),
                        "role" => s.old_role = Some(v.to_string()).filter(|r| !r.is_empty()),
                        "gate" => s.gate = v == "true" || v == "yes",
                        "done" => s.done = v.to_string(),
                        "needs" => s.needs = list(v),
                        _ => {}
                    }
                }
                rest[end + 3..].trim_start_matches(['-']).trim_start_matches('\n')
            }
            None => &text[..],
        },
        None => &text[..],
    };
    s.text = body.trim().to_string();
    s
}

fn unquote(v: &str) -> &str {
    v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(v)
}

/// `[a, b]` or `a, b`.
fn list(v: &str) -> Vec<String> {
    let v = v.trim().trim_start_matches('[').trim_end_matches(']');
    v.split(',').map(|x| unquote(x.trim()).to_string()).filter(|x| !x.is_empty()).collect()
}

/// Step S as its file text.
pub fn render_step(s: &Step) -> String {
    let one = |v: &str| v.replace('\n', " ");
    let mut out = String::from("---\n");
    if !s.description.is_empty() {
        out.push_str(&format!("description: {}\n", one(&s.description)));
    }
    if s.gate {
        out.push_str("gate: true\n");
    }
    if !s.done.is_empty() {
        out.push_str(&format!("done: {}\n", one(&s.done)));
    }
    if !s.needs.is_empty() {
        out.push_str(&format!("needs: [{}]\n", s.needs.join(", ")));
    }
    out.push_str("---\n");
    out.push_str(s.text.trim());
    out.push('\n');
    out
}

pub fn parse_flow(slug: &str, text: &str) -> Result<Flow, String> {
    let mut f: Flow = toml::from_str(text).map_err(|e| e.to_string())?;
    f.slug = slug.to_string();
    if f.name.trim().is_empty() {
        f.name = slug.to_string();
    }
    Ok(f)
}

pub fn render_flow(f: &Flow) -> Result<String, String> {
    toml::to_string_pretty(f).map_err(|e| e.to_string())
}

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
    ("bug-investigation", include_str!("builtin/flows/bug-investigation.toml")),
    ("refactor", include_str!("builtin/flows/refactor.toml")),
    ("concept", include_str!("builtin/flows/concept.toml")),
    ("performance", include_str!("builtin/flows/performance.toml")),
    ("migration", include_str!("builtin/flows/migration.toml")),
    ("hotfix", include_str!("builtin/flows/hotfix.toml")),
    ("code-review", include_str!("builtin/flows/code-review.toml")),
    ("chore", include_str!("builtin/flows/chore.toml")),
];

/// User layer dir: `<shared>/flows` or `<shared>/steps` (tests: the test data dir).
pub fn user_dir(kind: &str) -> PathBuf {
    if cfg!(test) {
        return super::data_dir().join("flowcfg").join(kind);
    }
    super::shared_dir().join(kind)
}

/// Repo root of DIR: the nearest dir with `.git` above it.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

pub fn repo_dir(root: &Path, kind: &str) -> PathBuf {
    root.join(".codehead").join(kind)
}

pub fn dir_of(layer: Layer, kind: &str, repo: Option<&Path>) -> Option<PathBuf> {
    match layer {
        Layer::Builtin => None,
        Layer::User => Some(user_dir(kind)),
        Layer::Repo => repo.map(|r| repo_dir(r, kind)),
    }
}

fn files(dir: &Path, ext: &str) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()?.to_str()? != ext {
                return None;
            }
            Some((p.file_stem()?.to_str()?.to_string(), std::fs::read_to_string(&p).ok()?))
        })
        .collect();
    v.sort();
    v
}

/// Every step and flow of the layers (built-in, user, repo of REPO), the last layer wins per
/// name. Flows: built-ins in their order first, then the rest by name.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub steps: Vec<Step>,
    pub flows: Vec<Flow>,
    /// Files that did not parse: (path, error).
    pub errors: Vec<(String, String)>,
    pub repo: Option<PathBuf>,
}

impl Library {
    /// The layers for work dir DIR (None: built-in + user).
    pub fn load(dir: Option<&Path>) -> Library {
        let repo = dir.and_then(repo_root);
        let mut lib = Library { repo: repo.clone(), ..Default::default() };
        for (n, t) in BUILTIN_STEPS {
            lib.steps.push(parse_step(n, t));
        }
        for (n, t) in BUILTIN_FLOWS {
            match parse_flow(n, t) {
                Ok(f) => lib.flows.push(f),
                Err(e) => lib.errors.push((format!("built-in {n}"), e)),
            }
        }
        let mut layers = vec![(Layer::User, user_dir("steps"), user_dir("flows"))];
        if let Some(r) = &repo {
            layers.push((Layer::Repo, repo_dir(r, "steps"), repo_dir(r, "flows")));
        }
        for (layer, sd, fd) in layers {
            for (n, t) in files(&sd, "md") {
                let mut s = parse_step(&n, &t);
                s.layer = layer;
                match lib.steps.iter().position(|x| x.name == n) {
                    Some(i) => {
                        s.edited = lib.steps[i].layer == Layer::Builtin || lib.steps[i].edited;
                        lib.steps[i] = s;
                    }
                    None => lib.steps.push(s),
                }
            }
            for (n, t) in files(&fd, "toml") {
                match parse_flow(&n, &t) {
                    Ok(mut f) => {
                        f.layer = layer;
                        match lib.flows.iter().position(|x| x.slug == n) {
                            Some(i) => {
                                f.edited = lib.flows[i].layer == Layer::Builtin || lib.flows[i].edited;
                                lib.flows[i] = f;
                            }
                            None => lib.flows.push(f),
                        }
                    }
                    Err(e) => lib.errors.push((fd.join(format!("{n}.toml")).display().to_string(), e)),
                }
            }
        }
        let builtin = BUILTIN_FLOWS.len().min(lib.flows.len());
        lib.flows[builtin..].sort_by_key(|f| f.name.to_lowercase());
        lib.steps.sort_by(|a, b| a.name.cmp(&b.name));
        lib
    }

    pub fn step(&self, name: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.name == name)
    }

    /// Flow by slug, else by name (any case).
    pub fn flow(&self, key: &str) -> Option<&Flow> {
        self.flows.iter().find(|f| f.slug == key).or_else(|| self.flows.iter().find(|f| f.name.eq_ignore_ascii_case(key)))
    }

    /// The flows that use step NAME.
    pub fn users_of(&self, name: &str) -> Vec<&Flow> {
        self.flows.iter().filter(|f| f.steps.iter().any(|n| n.uses.as_deref() == Some(name))).collect()
    }

    pub fn is_builtin_step(name: &str) -> bool {
        BUILTIN_STEPS.iter().any(|(n, _)| *n == name)
    }

    /// The layer an edit of LAYER writes to: built-in → user.
    fn write_layer(layer: Layer) -> Layer {
        if layer == Layer::Repo { Layer::Repo } else { Layer::User }
    }

    /// Write step S into its layer (built-in → user).
    pub fn save_step(&self, s: &Step) -> Result<PathBuf, String> {
        if !valid_name(&s.name) {
            return Err(format!("step name {:?}: letters, digits, - and _ only", s.name));
        }
        let dir = dir_of(Self::write_layer(s.layer), "steps", self.repo.as_deref()).ok_or("0 repo for a repo step")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let p = dir.join(format!("{}.md", s.name));
        super::write_atomic(&p, render_step(s).as_bytes()).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(p)
    }

    pub fn save_flow(&self, f: &Flow) -> Result<PathBuf, String> {
        if !valid_name(&f.slug) {
            return Err(format!("flow file name {:?}: letters, digits, - and _ only", f.slug));
        }
        let dir = dir_of(Self::write_layer(f.layer), "flows", self.repo.as_deref()).ok_or("0 repo for a repo flow")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let p = dir.join(format!("{}.toml", f.slug));
        super::write_atomic(&p, render_flow(f)?.as_bytes()).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(p)
    }

    /// Remove the file of step NAME in LAYER (a built-in: back to the built-in text).
    pub fn remove_step_file(&self, name: &str, layer: Layer) -> Result<(), String> {
        let dir = dir_of(layer, "steps", self.repo.as_deref()).ok_or("a built-in step has 0 file")?;
        let p = dir.join(format!("{name}.md"));
        std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn remove_flow_file(&self, slug: &str, layer: Layer) -> Result<(), String> {
        let dir = dir_of(layer, "flows", self.repo.as_deref()).ok_or("a built-in flow has 0 file")?;
        let p = dir.join(format!("{slug}.toml"));
        std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))
    }

    /// A free name: BASE, else BASE-2, BASE-3, ...
    pub fn free_step_name(&self, base: &str) -> String {
        free(base, |n| self.step(n).is_some())
    }

    pub fn free_flow_slug(&self, base: &str) -> String {
        free(base, |n| self.flows.iter().any(|f| f.slug == n))
    }

    /// Rename step OLD to NEW: the file moves, each flow that uses it (user / repo layer; a
    /// built-in flow gets a user copy) points to NEW.
    pub fn rename_step(&self, old: &str, new: &str) -> Result<(), String> {
        if !valid_name(new) {
            return Err(format!("step name {new:?}: letters, digits, - and _ only"));
        }
        if self.step(new).is_some() {
            return Err(format!("step {new} exists already"));
        }
        let mut s = self.step(old).cloned().ok_or_else(|| format!("no step {old}"))?;
        if Self::is_builtin_step(old) {
            return Err("a built-in step keeps its name: duplicate it".into());
        }
        s.name = new.to_string();
        self.save_step(&s)?;
        self.remove_step_file(old, s.layer)?;
        for f in self.users_of(old) {
            let mut f = f.clone();
            for n in &mut f.steps {
                if n.uses.as_deref() == Some(old) {
                    n.uses = Some(new.to_string());
                }
            }
            self.save_flow(&f)?;
        }
        Ok(())
    }

    /// Delete step NAME: each flow that uses it gets the text as a one-off node.
    pub fn delete_step(&self, name: &str) -> Result<usize, String> {
        let s = self.step(name).cloned().ok_or_else(|| format!("no step {name}"))?;
        let users: Vec<Flow> = self.users_of(name).into_iter().cloned().collect();
        for mut f in users.iter().cloned() {
            for n in &mut f.steps {
                if n.uses.as_deref() == Some(name) {
                    *n = inline(n, &s);
                }
            }
            self.save_flow(&f)?;
        }
        match s.layer {
            Layer::Builtin => return Err("a built-in step stays: edit it, or remove it from the flows".into()),
            l => self.remove_step_file(name, l)?,
        }
        Ok(users.len())
    }
}

fn free(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base}-{k}")).find(|n| !taken(n)).unwrap_or_default()
}

/// Node N (it uses step S) as a one-off with S's text and fields.
pub fn inline(n: &Node, s: &Step) -> Node {
    Node {
        id: n.id.clone().or_else(|| Some(s.name.clone())),
        uses: None,
        text: Some(s.text.clone()),
        label: n.label.clone().or_else(|| Some(s.name.clone())),
        note: n.note.clone(),
        role: None,
        done: n.done.clone().or_else(|| Some(s.done.clone()).filter(|d| !d.is_empty())),
        gate: n.gate.clone().or(if s.gate { Some(Gate::Flag(true)) } else { None }),
        loops: n.loops.clone(),
    }
}

/// One-off node N as step NAME (saved by the caller): the node then uses it.
pub fn promote(n: &Node, name: &str) -> (Step, Node) {
    let s = Step {
        name: name.to_string(),
        description: n.label.clone().unwrap_or_default(),
        old_role: None,
        gate: matches!(n.gate, Some(Gate::Flag(true)) | Some(Gate::Edges(_))),
        done: n.done.clone().unwrap_or_default(),
        needs: Vec::new(),
        text: n.text.clone().unwrap_or_default(),
        layer: Layer::User,
        edited: false,
    };
    let node = Node { id: n.id.clone(), uses: Some(name.to_string()), text: None, label: None, note: n.note.clone(), role: None, done: None, gate: match &n.gate {
        Some(Gate::Edges(e)) => Some(Gate::Edges(e.clone())),
        _ => None,
    }, loops: n.loops.clone() };
    (s, node)
}

/// Title of node N: label, else the step name, else the 1st line of the one-off text.
pub fn node_title(n: &Node) -> String {
    if let Some(l) = n.label.as_deref().filter(|l| !l.trim().is_empty()) {
        return l.trim().to_string();
    }
    if let Some(u) = &n.uses {
        return u.clone();
    }
    let t = n.text.as_deref().unwrap_or_default().lines().find(|l| !l.trim().is_empty()).unwrap_or("one-off step").trim();
    let t: String = t.chars().take(48).collect();
    t
}

/// Node ids of F, in order (unique).
pub fn node_ids(f: &Flow) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for (i, n) in f.steps.iter().enumerate() {
        let base = n.id.clone().or_else(|| n.uses.clone()).unwrap_or_else(|| format!("step-{}", i + 1));
        let id = free(&base, |x| ids.iter().any(|y| y == x));
        ids.push(id);
    }
    ids
}

/// F's nodes with their steps filled in.
pub fn resolve(f: &Flow, lib: &Library) -> Vec<Resolved> {
    let ids = node_ids(f);
    let at = |id: &str| ids.iter().position(|x| x == id);
    f.steps
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let step = n.uses.as_deref().and_then(|u| lib.step(u));
            let mut errors = Vec::new();
            if let (Some(u), None) = (&n.uses, step) {
                errors.push(format!("step {u} not found"));
            }

            // an old file's role: gone, the step runs on the flow's agent
            if let Some(r) = n.role.as_ref().or_else(|| step.and_then(|s| s.old_role.as_ref())).filter(|r| r.as_str() != "lead") {
                errors.push(format!("role {r}: roles are gone, the step runs on the flow's agent"));
            }
            let mut text = match (step, &n.text) {
                (Some(s), _) => s.text.clone(),
                (None, Some(t)) => t.clone(),
                (None, None) => String::new(),
            };
            if let Some(note) = n.note.as_deref().filter(|x| !x.trim().is_empty()) {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(note.trim());
            }
            let (gate, edges) = match &n.gate {
                Some(Gate::Flag(b)) => (*b, Vec::new()),
                Some(Gate::Edges(m)) => (
                    true,
                    m.iter()
                        .filter_map(|(k, v)| match at(v) {
                            Some(j) => Some((k.clone(), j)),
                            None => {
                                errors.push(format!("gate outcome {k}: node {v} not found"));
                                None
                            }
                        })
                        .collect(),
                ),
                None => (step.is_some_and(|s| s.gate), Vec::new()),
            };
            let loops = n.loops.as_ref().and_then(|l| match at(&l.to) {
                Some(j) if j <= i => Some((j, l.until.clone(), l.max.max(1))),
                Some(_) => {
                    errors.push(format!("loop to {}: a loop goes to a node above", l.to));
                    None
                }
                None => {
                    errors.push(format!("loop to {}: node not found", l.to));
                    None
                }
            });
            Resolved {
                id: ids[i].clone(),
                title: node_title(n),
                step: n.uses.clone(),
                text,
                done: n.done.clone().or_else(|| step.map(|s| s.done.clone())).unwrap_or_default(),
                gate,
                edges,
                loops,
                needs: step.map(|s| s.needs.clone()).unwrap_or_default(),
                errors,
            }
        })
        .collect()
}

/// `#name`s in TEXT that name 0 shortcut.
pub fn unknown_shortcuts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices('#') {
        if i > 0 && text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '#') {
            continue;
        }
        let w: String = text[i + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
        if w.is_empty() || w.chars().all(|c| c.is_ascii_digit()) || out.contains(&w) {
            continue;
        }
        if crate::shortcuts::get(&w).is_none() {
            out.push(w);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_round_trip_through_front_matter() {
        let s = Step { name: "x".into(), description: "d".into(), gate: true, done: "it is done".into(), needs: vec!["skill:code-review".into(), "mcp:gh".into()], text: "line 1\n\nline 2".into(), ..Default::default() };
        assert_eq!(parse_step("x", &render_step(&s)), s);
        // 0 front matter: all body
        assert_eq!(parse_step("y", "just text").text, "just text");
    }

    /// Roles are gone: an old step / flow file with roles parses, its role gives a warning (the
    /// step runs on the flow's agent), a save drops it.
    #[test]
    fn old_roles_parse_warn_and_go_on_save() {
        let s = parse_step("rev", "---\nrole: reviewer\ngate: true\n---\nreview it");
        assert_eq!(s.old_role.as_deref(), Some("reviewer"));
        assert!(!render_step(&s).contains("role"), "{}", render_step(&s));
        let f = parse_flow("t", "name = \"t\"\nroles = [\"lead\", \"tester\"]\n[[step]]\ntext = \"a\"\nrole = \"tester\"\n[[step]]\ntext = \"b\"\nrole = \"lead\"\n").unwrap();
        let r = resolve(&f, &Library::default());
        assert_eq!(r[0].errors, vec!["role tester: roles are gone, the step runs on the flow's agent".to_string()]);
        assert!(r[1].errors.is_empty(), "role lead: 0 warning");
        let text = render_flow(&f).unwrap();
        assert!(!text.contains("role"), "{text}");
    }

    #[test]
    fn every_builtin_parses_and_resolves_without_errors() {
        let lib = Library::load(None);
        assert_eq!(lib.flows.len(), BUILTIN_FLOWS.len(), "{:?}", lib.errors);
        assert!(lib.errors.is_empty(), "{:?}", lib.errors);
        for f in &lib.flows {
            for r in resolve(f, &lib) {
                assert!(r.errors.is_empty(), "{}: {}: {:?}", f.slug, r.id, r.errors);
                assert!(!r.text.is_empty(), "{}: {} has 0 text", f.slug, r.id);
            }
        }
        let nf = lib.flow("new feature").unwrap();
        let r = resolve(nf, &lib);
        let gate = r.iter().position(|n| n.title == "plan approval").unwrap();
        assert!(r[gate].gate);
        assert_eq!(r[gate].edges, vec![("changes".to_string(), r.iter().position(|n| n.id == "plan").unwrap())]);
        let ar = r.iter().find(|n| n.id == "address-review").unwrap();
        assert_eq!(ar.loops.as_ref().unwrap().0, r.iter().position(|n| n.id == "review").unwrap());
    }

    #[test]
    fn loops_must_go_up_and_targets_must_exist() {
        let f = parse_flow(
            "t",
            r#"name = "t"
[[step]]
text = "a"
loop = { to = "b" }
[[step]]
id = "b"
text = "b"
gate = { back = "nowhere" }
"#,
        )
        .unwrap();
        let r = resolve(&f, &Library::default());
        assert!(r[0].loops.is_none() && !r[0].errors.is_empty());
        assert!(r[1].gate && r[1].edges.is_empty() && !r[1].errors.is_empty());
        assert_eq!(r[0].id, "step-1");
        assert_eq!(r[0].title, "a");
    }

    #[test]
    fn user_layer_wins_and_rename_and_delete_keep_flows_whole() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_dir_all(user_dir("steps").parent().unwrap());
        let lib = Library::load(None);
        // edit of a built-in: a user file over it
        let mut s = lib.step("report").unwrap().clone();
        s.text = "my report".into();
        lib.save_step(&s).unwrap();
        let lib = Library::load(None);
        assert_eq!(lib.step("report").unwrap().text, "my report");
        assert!(lib.step("report").unwrap().edited);
        lib.remove_step_file("report", Layer::User).unwrap();
        // a user step in a user flow: rename, then delete → one-off node with its text
        let lib = Library::load(None);
        lib.save_step(&Step { name: "mine".into(), text: "do mine".into(), layer: Layer::User, ..Default::default() }).unwrap();
        let f = Flow { slug: "my-flow".into(), name: "my flow".into(), steps: vec![Node { uses: Some("mine".into()), ..Default::default() }], layer: Layer::User, ..Default::default() };
        lib.save_flow(&f).unwrap();
        let lib = Library::load(None);
        assert_eq!(lib.users_of("mine").len(), 1);
        lib.rename_step("mine", "ours").unwrap();
        let lib = Library::load(None);
        assert_eq!(lib.flow("my-flow").unwrap().steps[0].uses.as_deref(), Some("ours"));
        assert_eq!(lib.delete_step("ours").unwrap(), 1);
        let lib = Library::load(None);
        let n = &lib.flow("my-flow").unwrap().steps[0];
        assert_eq!((n.uses.as_deref(), n.text.as_deref()), (None, Some("do mine")));
        // promote it back
        let (s, n2) = promote(n, "again");
        assert_eq!((s.text.as_str(), n2.uses.as_deref()), ("do mine", Some("again")));
        let _ = std::fs::remove_dir_all(user_dir("steps").parent().unwrap());
    }

    #[test]
    fn slugs_and_titles() {
        assert_eq!(slug("Migration / Upgrade"), "migration-upgrade");
        assert_eq!(node_title(&Node { text: Some("\n  update CHANGELOG.md\nmore".into()), ..Default::default() }), "update CHANGELOG.md");
    }
}
