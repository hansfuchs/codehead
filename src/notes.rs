//! Notes on disk: .md files under the notes root, folder tree, fuzzy search (name + content).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Notes {
    pub root: PathBuf,
    /// Relative paths (`dir/name.md`), sorted.
    pub files: Vec<String>,
    mtimes: HashMap<String, SystemTime>,
}

#[derive(Clone, Debug)]
pub enum Row {
    Dir { path: String, name: String, depth: usize, count: usize, collapsed: bool },
    File { path: String, name: String, depth: usize },
    /// Search hit: full relative path + matched line.
    Hit { path: String, snippet: String },
}

impl Row {
    pub fn path(&self) -> &str {
        match self {
            Row::Dir { path, .. } | Row::File { path, .. } | Row::Hit { path, .. } => path,
        }
    }
    pub fn is_dir(&self) -> bool {
        matches!(self, Row::Dir { .. })
    }
}

impl Notes {
    pub fn load(root: &Path) -> Notes {
        let mut files = Vec::new();
        let mut mtimes = HashMap::new();
        walk(root, root, &mut files, &mut mtimes);
        files.sort();
        Notes { root: root.to_path_buf(), files, mtimes }
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_end_matches('/'))
    }

    pub fn mtime(&self, rel: &str) -> SystemTime {
        self.mtimes.get(rel).copied().unwrap_or(SystemTime::UNIX_EPOCH)
    }

    pub fn age(&self, rel: &str) -> String {
        ago(self.mtime(rel))
    }

    /// Files inside DIR (`dir/`), or the file itself.
    pub fn inside(&self, path: &str) -> Vec<String> {
        if path.ends_with('/') {
            self.files.iter().filter(|f| f.starts_with(path)).cloned().collect()
        } else {
            self.files.iter().filter(|f| *f == path).cloned().collect()
        }
    }

    /// Tree rows; folders first (by name), files newest first.
    pub fn tree(&self, collapsed: &HashSet<String>) -> Vec<Row> {
        #[derive(Default)]
        struct Node {
            dirs: BTreeMap<String, Node>,
            files: Vec<String>,
        }
        fn count(n: &Node) -> usize {
            n.files.len() + n.dirs.values().map(count).sum::<usize>()
        }
        let mut root = Node::default();
        for f in &self.files {
            let parts: Vec<&str> = f.split('/').collect();
            let mut node = &mut root;
            for d in &parts[..parts.len() - 1] {
                node = node.dirs.entry(d.to_string()).or_default();
            }
            node.files.push(f.clone());
        }
        fn walk(n: &Node, prefix: &str, depth: usize, notes: &Notes, collapsed: &HashSet<String>, out: &mut Vec<Row>) {
            for (name, child) in &n.dirs {
                let path = format!("{prefix}{name}/");
                let closed = collapsed.contains(&path);
                out.push(Row::Dir { path: path.clone(), name: name.clone(), depth, count: count(child), collapsed: closed });
                if !closed {
                    walk(child, &path, depth + 1, notes, collapsed, out);
                }
            }
            let mut files = n.files.clone();
            files.sort_by_key(|f| std::cmp::Reverse(notes.mtime(f)));
            for f in files {
                let name = f.rsplit('/').next().unwrap_or(&f).trim_end_matches(".md").to_string();
                out.push(Row::File { path: f, name, depth });
            }
        }
        let mut out = Vec::new();
        walk(&root, "", 0, self, collapsed, &mut out);
        out
    }

    /// Ranked search rows. Empty query: every file, newest first.
    pub fn search(&self, query: &str) -> Vec<Row> {
        let terms: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
        let mut ranked: Vec<(i64, &String, String)> = Vec::new();
        for p in &self.files {
            if terms.is_empty() {
                ranked.push((0, p, String::new()));
                continue;
            }
            let Ok(content) = std::fs::read_to_string(self.root.join(p)) else { continue };
            let lines: Vec<&str> = content.lines().map(|l| truncate_chars(l, 400)).collect();
            let (mut total, mut snip, mut ok) = (0i64, String::new(), true);
            for term in &terms {
                let name_score = fuzzy(term, p);
                let mut best: Option<(&str, i64)> = None;
                for line in &lines {
                    let sc = fuzzy(term, line).filter(|s| *s >= 12 * term.chars().count() as i64);
                    if let Some(s) = sc
                        && best.is_none_or(|(_, b)| s > b)
                    {
                        best = Some((line, s));
                    }
                }
                if name_score.is_none() && best.is_none() {
                    ok = false;
                    break;
                }
                let name2 = name_score.unwrap_or(0) * 2;
                total += name2.max(best.map(|b| b.1).unwrap_or(0));
                if snip.is_empty()
                    && let Some((line, s)) = best
                    && s >= name2
                {
                    snip = snippet(line, term);
                }
            }
            if ok {
                ranked.push((total, p, snip));
            }
        }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| self.mtime(b.1).cmp(&self.mtime(a.1))));
        ranked.into_iter().map(|(_, p, snippet)| Row::Hit { path: p.clone(), snippet }).collect()
    }
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<String>, mtimes: &mut HashMap<String, SystemTime>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            walk(root, &path, files, mtimes);
        } else if name.ends_with(".md") {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
            mtimes.insert(rel.clone(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));
            files.push(rel);
        }
    }
}

pub fn ago(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    for (unit, n) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if s >= n {
            return format!("{}{unit}", s / n);
        }
    }
    "now".into()
}

fn truncate_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Score TERM as a fuzzy subsequence of TEXT (case-insensitive). None = no match.
pub fn fuzzy(term: &str, text: &str) -> Option<i64> {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let q: Vec<char> = term.chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let alnum = |i: usize| t[i].is_alphanumeric();
    // contiguous match wins
    if let Some(i) = (0..t.len()).find(|&i| t[i..].starts_with(&q)) {
        let boundary = if i == 0 || !alnum(i - 1) { 20 } else { 0 };
        return Some(100 + 12 * q.len() as i64 + boundary);
    }
    let find = |c: char, from: usize| (from..t.len()).find(|&i| t[i] == c);
    let mut best: Option<i64> = None;
    let mut start = find(q[0], 0);
    let mut tries = 0;
    while let Some(s0) = start {
        if tries >= 20 {
            break;
        }
        tries += 1;
        let (mut score, mut pos, mut prev): (i64, usize, i64) = (0, s0, -2);
        let mut matched = true;
        for &ch in &q {
            let Some(p) = find(ch, pos) else {
                matched = false;
                break;
            };
            score += 10;
            if p as i64 == prev + 1 {
                score += 8;
            } else if prev >= 0 {
                score -= (p as i64 - prev - 1).min(10);
            }
            if p == 0 || !alnum(p - 1) {
                score += 6;
            }
            prev = p as i64;
            pos = p + 1;
        }
        if !matched {
            break;
        }
        best = Some(best.map_or(score, |b| b.max(score)));
        start = find(q[0], s0 + 1);
    }
    best
}

fn snippet(line: &str, term: &str) -> String {
    let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = line.chars().collect();
    let lower: Vec<char> = line.to_lowercase().chars().collect();
    let head: Vec<char> = term.chars().take(3).collect();
    let i = (0..lower.len()).find(|&i| lower[i..].starts_with(&head)).unwrap_or(0);
    let s = i.saturating_sub(20);
    let e = (s + 70).min(chars.len());
    format!("{}{}{}", if s > 0 { "…" } else { "" }, chars[s..e].iter().collect::<String>(), if e < chars.len() { "…" } else { "" })
}

pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
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
mod tests {
    use super::*;

    #[test]
    fn slug_cuts_and_trims() {
        assert_eq!(slug("Fix the Build, now!"), "fix-the-build-now");
        assert_eq!(slug("  "), "");
    }

    #[test]
    fn fuzzy_prefers_contiguous() {
        assert!(fuzzy("kub", "use kubectl").unwrap() > fuzzy("kbl", "use kubectl").unwrap());
        assert!(fuzzy("zz", "abc").is_none());
    }
}
