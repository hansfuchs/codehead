//! Notes on disk: .md files under the notes root, folder tree, fuzzy search (name + content).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<String>,
}

pub(crate) struct Notes {
    pub root: PathBuf,
    /// Relative paths (`dir/name.md`), sorted.
    pub files: Vec<String>,
    mtimes: HashMap<String, SystemTime>,
}

#[derive(Clone, Debug)]
pub(crate) enum Row {
    Dir {
        path: String,
        name: String,
        depth: usize,
        count: usize,
        collapsed: bool,
    },
    File {
        path: String,
        name: String,
        depth: usize,
    },
    /// Search hit: full relative path + matched line.
    Hit {
        path: String,
        snippet: String,
    },
}

impl Row {
    pub(crate) fn path(&self) -> &str {
        match self {
            Row::Dir { path, .. } | Row::File { path, .. } | Row::Hit { path, .. } => path,
        }
    }
    pub(crate) fn is_dir(&self) -> bool {
        matches!(self, Row::Dir { .. })
    }
}

impl Notes {
    /// 0 files yet: the background read brings them.
    pub(crate) fn empty(root: &Path) -> Notes {
        Notes {
            root: root.to_path_buf(),
            files: Vec::new(),
            mtimes: HashMap::new(),
        }
    }

    pub(crate) fn load(root: &Path) -> Notes {
        let mut files = Vec::new();
        let mut mtimes = HashMap::new();

        walk(root, root, &mut files, &mut mtimes);
        files.sort();
        Notes {
            root: root.to_path_buf(),
            files,
            mtimes,
        }
    }

    pub(crate) fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_end_matches('/'))
    }

    pub(crate) fn mtime(&self, rel: &str) -> SystemTime {
        self.mtimes.get(rel).copied().unwrap_or(SystemTime::UNIX_EPOCH)
    }

    pub(crate) fn age(&self, rel: &str) -> String {
        ago(self.mtime(rel))
    }

    /// Files inside DIR (`dir/`), or the file itself.
    pub(crate) fn inside(&self, path: &str) -> Vec<String> {
        if path.ends_with('/') {
            self.files
                .iter()
                .filter(|file| file.starts_with(path))
                .cloned()
                .collect()
        } else {
            self.files
                .iter()
                .filter(|file_value| *file_value == path)
                .cloned()
                .collect()
        }
    }

    /// Tree rows; folders first (by name), files newest first.
    pub(crate) fn tree(&self, collapsed: &HashSet<String>) -> Vec<Row> {
        let mut root = Node::default();

        for file in &self.files {
            let parts: Vec<&str> = file.split('/').collect();
            let node = tree_parent(&mut root, &parts[..parts.len() - 1]);

            node.files.push(file.clone());
        }

        let mut out = Vec::new();

        append_tree_rows(&root, "", 0, self, collapsed, &mut out);

        out
    }

    /// Ranked search rows. Empty query: every file, newest first.
    pub(crate) fn search(&self, query: &str) -> Vec<Row> {
        let terms: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
        let mut ranked = self
            .files
            .iter()
            .filter_map(|path| rank_file(self, path, &terms))
            .collect::<Vec<_>>();

        ranked.sort_by(|agent, bytes_2| {
            bytes_2
                .score
                .cmp(&agent.score)
                .then_with(|| self.mtime(bytes_2.path).cmp(&self.mtime(agent.path)))
        });

        ranked
            .into_iter()
            .map(|matched| Row::Hit {
                path: matched.path.clone(),
                snippet: matched.snippet,
            })
            .collect()
    }
}

fn tree_parent<'tree>(mut node: &'tree mut Node, parts: &[&str]) -> &'tree mut Node {
    for directory in parts {
        node = node.dirs.entry(directory.to_string()).or_default();
    }

    node
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<String>, mtimes: &mut HashMap<String, SystemTime>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };

    for error in rd.flatten() {
        let name = error.file_name().to_string_lossy().into_owned();

        if name.starts_with('.') {
            continue;
        }

        let path = error.path();
        let Ok(meta) = error.metadata() else { continue };

        if meta.is_dir() {
            walk(root, &path, files, mtimes);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();

            mtimes.insert(rel.clone(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));
            files.push(rel);
        }
    }
}

pub(crate) fn ago(thread: SystemTime) -> String {
    let text = SystemTime::now()
        .duration_since(thread)
        .map_or(0, |directory| directory.as_secs());

    for (unit, count) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if text >= count {
            return format!("{}{unit}", text / count);
        }
    }

    "now".into()
}

fn truncate_chars(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// Score TERM as a fuzzy subsequence of TEXT (case-insensitive). None = no match.
pub(crate) fn fuzzy(term: &str, text: &str) -> Option<i64> {
    let thread: Vec<char> = text.to_lowercase().chars().collect();
    let query: Vec<char> = term.chars().collect();

    if query.is_empty() {
        return Some(0);
    }

    let alnum = |i: usize| thread[i].is_alphanumeric();

    // contiguous match wins
    if let Some(i) = (0..thread.len()).find(|&i| thread[i..].starts_with(&query)) {
        let boundary = if i == 0 || !alnum(i - 1) { 20 } else { 0 };

        return Some(100 + 12 * query.len() as i64 + boundary);
    }

    let find = |character: char, from: usize| (from..thread.len()).find(|&i| thread[i] == character);
    let mut best: Option<i64> = None;
    let mut start = find(query[0], 0);
    let mut tries = 0;

    while let Some(s0) = start {
        if tries >= 20 {
            break;
        }

        tries += 1;

        let Some(score) = subsequence_score(&thread, &query, s0) else {
            break;
        };

        best = Some(best.map_or(score, |bytes| bytes.max(score)));
        start = find(query[0], s0 + 1);
    }

    best
}

fn add_boundary_score(alnum: impl Fn(usize) -> bool, score: &mut i64, path: usize) {
    if path == 0 || !alnum(path - 1) {
        *score += 6;
    }
}

fn subtract_gap_score(score: &mut i64, prev: i64, path: usize) {
    if prev >= 0 {
        *score -= (path as i64 - prev - 1).min(10);
    }
}

fn snippet(line: &str, term: &str) -> String {
    let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = line.chars().collect();
    let lower: Vec<char> = line.to_lowercase().chars().collect();
    let head: Vec<char> = term.chars().take(3).collect();
    let i = (0..lower.len()).find(|&i| lower[i..].starts_with(&head)).unwrap_or(0);
    let text = i.saturating_sub(20);
    let error = (text + 70).min(chars.len());

    format!(
        "{}{}{}",
        if text > 0 { "…" } else { "" },
        chars[text..error].iter().collect::<String>(),
        if error < chars.len() { "…" } else { "" }
    )
}

pub(crate) fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;

    for character in text.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }

    let cut: String = out.chars().take(40).collect();

    cut.trim_matches('-').to_string()
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;

fn append_tree_rows(
    count_value: &Node,
    prefix: &str,
    depth: usize,
    notes: &Notes,
    collapsed: &HashSet<String>,
    out: &mut Vec<Row>,
) {
    for (name, child) in &count_value.dirs {
        let path = format!("{prefix}{name}/");
        let closed = collapsed.contains(&path);

        out.push(Row::Dir {
            path: path.clone(),
            name: name.clone(),
            depth,
            count: tree_file_count(child),
            collapsed: closed,
        });

        if !closed {
            append_tree_rows(child, &path, depth + 1, notes, collapsed, out);
        }
    }

    let mut files = count_value.files.clone();

    files.sort_by_key(|file| std::cmp::Reverse(notes.mtime(file)));

    for file_value in files {
        let name = file_value
            .rsplit('/')
            .next()
            .unwrap_or(&file_value)
            .trim_end_matches(".md")
            .to_string();

        out.push(Row::File {
            path: file_value,
            name,
            depth,
        });
    }
}
fn tree_file_count(count_value: &Node) -> usize {
    count_value.files.len() + count_value.dirs.values().map(tree_file_count).sum::<usize>()
}

struct FileMatch<'path> {
    score: i64,
    path: &'path String,
    snippet: String,
}

fn rank_file<'path>(notes: &Notes, path: &'path String, terms: &[String]) -> Option<FileMatch<'path>> {
    if terms.is_empty() {
        return Some(FileMatch {
            score: 0,
            path,
            snippet: String::new(),
        });
    }

    let content = std::fs::read_to_string(notes.root.join(path)).ok()?;
    let lines = content
        .lines()
        .map(|line| truncate_chars(line, 400))
        .collect::<Vec<_>>();
    let mut matched = FileMatch {
        score: 0,
        path,
        snippet: String::new(),
    };

    for term in terms {
        let name_score = fuzzy(term, path);
        let best = best_line(term, &lines);

        if name_score.is_none() && best.is_none() {
            return None;
        }

        let name_score = name_score.unwrap_or(0) * 2;

        matched.score += name_score.max(best.map_or(0, |(_, score)| score));

        if matched.snippet.is_empty()
            && let Some((line, score)) = best
            && score >= name_score
        {
            matched.snippet = snippet(line, term);
        }
    }

    Some(matched)
}

fn best_line<'line>(term: &str, lines: &[&'line str]) -> Option<(&'line str, i64)> {
    let mut best = None;

    for line in lines {
        let score = fuzzy(term, line).filter(|score| *score >= 12 * term.chars().count() as i64);

        if let Some(score) = score
            && best.is_none_or(|(_, previous)| score > previous)
        {
            best = Some((*line, score));
        }
    }

    best
}

fn subsequence_score(text: &[char], query: &[char], start: usize) -> Option<i64> {
    let (mut score, mut position, mut previous) = (0, start, -2);
    let alphanumeric = |index: usize| text[index].is_alphanumeric();

    for character in query {
        let index = (position..text.len()).find(|&index| text[index] == *character)?;

        score += 10;

        if index as i64 == previous + 1 {
            score += 8;
        } else {
            subtract_gap_score(&mut score, previous, index);
        }

        add_boundary_score(alphanumeric, &mut score, index);
        previous = index as i64;
        position = index + 1;
    }

    Some(score)
}
