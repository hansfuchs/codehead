//! The folder tree: every module under a layer folder, tests in `*_tests.rs` files.
//!
//! Layers: `engine`, `infra`, `domain` (0 UI), `ui` (shared widgets), `features` (1 folder per
//! feature), `screens` (compose features), `perf` (perf suite). `src/main.rs`: CLI dispatch only.

use std::path::Component;

use crate::{Finding, Source};

pub(crate) const LAYERS: [&str; 7] = ["engine", "infra", "domain", "ui", "features", "screens", "perf"];

pub(crate) fn layers(sources: &[Source]) -> Vec<Finding> {
    sources
        .iter()
        .filter(|source| source.path.starts_with("src"))
        .filter(|source| {
            let parts = parts(source);
            let at_root = parts.len() == 2 && parts[1] != "main.rs";
            let unknown = parts.len() > 2 && !LAYERS.contains(&parts[1].as_str());

            at_root || unknown
        })
        .map(|source| {
            source.finding(
                1,
                format!("module outside the layers: move it under src/{{{}}}", LAYERS.join(",")),
            )
        })
        .collect()
}

pub(crate) fn tests(source: &Source) -> Vec<Finding> {
    let name = source
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let is_test_file = name.ends_with("_tests.rs");
    let lines = source.lines();
    let mut findings = Vec::new();

    if !source.path.starts_with("src") {
        return findings;
    }

    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        let next = lines.get(index + 1).map_or("", |next| next.trim());

        if line == "#[test]" && !is_test_file {
            findings.push(source.finding(index + 1, "test outside a *_tests.rs file"));
        }

        if line == "#[cfg(test)]" && next.starts_with("mod ") && next.ends_with('{') {
            findings.push(source.finding(index + 2, "inline test module: move it to a *_tests.rs file"));
        }
    }

    if let Some(feature) =
        feature_of(source).filter(|feature| is_test_file && !name.starts_with(&format!("{feature}_")))
    {
        findings.push(source.finding(
            1,
            format!("test file of feature `{feature}`: name it {feature}_*tests.rs"),
        ));
    }

    findings
}

pub(crate) fn parts(source: &Source) -> Vec<String> {
    source
        .path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str().map(String::from),
            _ => None,
        })
        .collect()
}

pub(crate) fn feature_of(source: &Source) -> Option<String> {
    let parts = parts(source);
    let is_feature = parts.len() > 3 && parts[0] == "src" && (parts[1] == "features" || parts[1] == "screens");

    is_feature.then(|| parts[2].clone())
}
