//! Repo rules that rustfmt and clippy cannot check: file size, fn length, blank lines between
//! blocks, test file names, imports between layers. `TIDY_FIX=1 cargo test --test tidy` inserts
//! the missing blank lines.

mod blank_lines;
mod imports;
mod sizes;
mod tree;

use std::fmt;
use std::path::{Path, PathBuf};

const MAX_FILE_LINES: usize = 600;
const MAX_FN_LINES: usize = 60;

struct Finding {
    path: PathBuf,
    line: usize,
    message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}

struct Source {
    path: PathBuf,
    text: String,
}

impl Source {
    fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    fn finding(&self, line: usize, message: impl Into<String>) -> Finding {
        Finding {
            path: self.path.clone(),
            line,
            message: message.into(),
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn sources() -> Vec<Source> {
    let root = root();
    let mut paths = Vec::new();

    rust_files(&root.join("src"), &mut paths);
    rust_files(&root.join("tests"), &mut paths);
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let path = path.strip_prefix(&root).map(Path::to_path_buf).unwrap_or(path);

            Source { path, text }
        })
        .collect()
}

fn report(findings: &[Finding]) {
    let shown: Vec<String> = findings.iter().take(5000).map(ToString::to_string).collect();
    let more = findings.len().saturating_sub(shown.len());

    assert!(
        findings.is_empty(),
        "{} tidy findings:\n{}\n(+{more} more)",
        findings.len(),
        shown.join("\n")
    );
}

#[test]
fn layout() {
    let fix = std::env::var_os("TIDY_FIX").is_some();
    let mut findings = Vec::new();

    for source in sources() {
        if fix && blank_lines::fix(&root().join(&source.path), &source) {
            continue;
        }

        findings.extend(blank_lines::check(&source));
    }

    report(&findings);
}

#[test]
fn sizes() {
    let findings: Vec<Finding> = sources().iter().flat_map(sizes::check).collect();

    report(&findings);
}

#[test]
fn layers() {
    let sources = sources();
    let mut findings = tree::layers(&sources);

    findings.extend(sources.iter().flat_map(tree::tests));
    findings.extend(sources.iter().flat_map(imports::check));

    report(&findings);
}
