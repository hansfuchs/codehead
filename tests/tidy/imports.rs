//! Imports between layers. engine, infra, domain: 0 UI. ui: 0 features. features: 0 code of
//! another feature, 0 screens. screens compose the features.

use crate::tree::{feature_of, parts};
use crate::{Finding, Source};

struct Rules {
    layer: String,
    forbidden: &'static [&'static str],
    own: Option<String>,
    depth: Option<usize>,
}

pub(crate) fn check(source: &Source) -> Vec<Finding> {
    let parts = parts(source);

    if parts.len() < 3 || parts[0] != "src" {
        return Vec::new();
    }

    let rules = Rules {
        layer: parts[1].clone(),
        forbidden: forbidden(&parts[1]),
        own: (parts[1] == "features").then(|| feature_of(source)).flatten(),
        depth: feature_depth(&parts),
    };

    source
        .lines()
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().starts_with("//"))
        .flat_map(|(index, line)| {
            problems(&rules, line.trim())
                .into_iter()
                .map(move |message| (index, message))
        })
        .map(|(index, message)| source.finding(index + 1, message))
        .collect()
}

fn problems(rules: &Rules, code: &str) -> Vec<String> {
    let layer = &rules.layer;
    let mut problems: Vec<String> = rules
        .forbidden
        .iter()
        .filter(|forbidden| names(code, &format!("crate::{forbidden}")).next().is_some())
        .map(|forbidden| format!("`{layer}` must not import `crate::{forbidden}`"))
        .collect();

    if let Some(own) = &rules.own {
        let message = |other| format!("feature `{own}` imports feature `{other}`: move the shared code down a layer");

        problems.extend(other_features(code, own).map(message));
    }

    if rules
        .depth
        .is_some_and(|depth| code.contains(&"super::".repeat(depth + 1)))
    {
        problems.push("`super::` leaves the feature folder".to_string());
    }

    problems
}

fn forbidden(layer: &str) -> &'static [&'static str] {
    match layer {
        "engine" | "infra" | "domain" => &["ui", "features", "screens", "perf"],
        "ui" => &["features", "screens", "perf"],
        "features" => &["screens", "perf"],
        _ => &[],
    }
}

/// Module levels below the feature folder: `mod.rs` 0, `a.rs` 1, `a/b.rs` 2. None: not a feature.
fn feature_depth(parts: &[String]) -> Option<usize> {
    let rest = parts.get(3..).filter(|_| parts[1] == "features")?;
    let is_mod = rest.last().is_some_and(|name| name == "mod.rs");

    Some(rest.len() - usize::from(is_mod))
}

/// The identifiers right after each `prefix` in `code` that end at a word boundary.
fn names<'code>(code: &'code str, prefix: &str) -> impl Iterator<Item = &'code str> {
    code.match_indices(prefix).filter_map(move |(at, _)| {
        let before = code[..at].chars().next_back();
        let rest = &code[at + prefix.len()..];
        let boundary = rest
            .chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && next != '_');
        let starts_word = before.is_none_or(|prev| !prev.is_alphanumeric() && prev != '_');

        (boundary && starts_word).then_some(rest)
    })
}

fn other_features<'code>(code: &'code str, own: &str) -> impl Iterator<Item = &'code str> {
    let own = own.to_string();

    code.match_indices("features::").filter_map(move |(at, prefix)| {
        let rest = &code[at + prefix.len()..];
        let end = rest
            .find(|next: char| !next.is_alphanumeric() && next != '_')
            .unwrap_or(rest.len());
        let name = &rest[..end];

        (!name.is_empty() && name != own).then_some(name)
    })
}
