use super::categorize;
use super::{DLine, FileDiff, Hash, Hasher, Hunk, Kind, MAX_FILE, Path, Status};

impl FileDiff {
    pub(crate) fn new(path: String) -> FileDiff {
        FileDiff {
            cat: categorize(&path),
            path,
            old_path: None,
            status: Status::Modified,
            binary: false,
            hunks: Vec::new(),
            add: 0,
            del: 0,
            hash: 0,
        }
    }
    pub(crate) fn seal(&mut self) {
        self.add = self
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == Kind::Add)
            .count();
        self.del = self
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == Kind::Del)
            .count();
        self.cat = categorize(&self.path);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        (&self.path, self.binary, self.status as u8).hash(&mut hasher);

        for line in self
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind != Kind::Ctx)
        {
            (line.kind as u8, &line.text).hash(&mut hasher);
        }

        self.hash = hasher.finish();
    }
    /// The new-side line of the 1st change (deleted file: 1).
    pub(crate) fn first_line(&self) -> u32 {
        self.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .find(|line| line.kind != Kind::Ctx)
            .and_then(|line| line.new.or(line.old))
            .unwrap_or(1)
    }
}
/// Files of the unified diff DIFF (`git diff`, any -U).
pub(crate) fn parse(diff: &str) -> Vec<FileDiff> {
    let mut parser = DiffParser::default();

    for line in diff.lines() {
        parser.line(line);
    }

    parser.finish()
}

#[derive(Default)]
struct DiffParser {
    files: Vec<FileDiff>,
    hunk: Option<Hunk>,
    old_line: u32,
    new_line: u32,
}

impl DiffParser {
    fn line(&mut self, line: &str) {
        if let Some(header) = line.strip_prefix("diff --git ") {
            begin_file(header, self);
            return;
        }

        if self.files.is_empty() {
            return;
        }

        if self.hunk.is_none() || line.starts_with("@@ ") {
            self.header(line);
            return;
        }

        hunk_line(line, self);
    }

    fn header(&mut self, line: &str) {
        if let Some(header) = line.strip_prefix("@@ ") {
            begin_hunk(header, self);
            return;
        }

        if let Some(file) = self.files.last_mut() {
            update_file_header(file, line);
        }
    }

    fn flush(&mut self) {
        if let (Some(hunk), Some(file)) = (self.hunk.take(), self.files.last_mut()) {
            file.hunks.push(hunk);
        }
    }

    fn finish(mut self) -> Vec<FileDiff> {
        self.flush();

        for file in &mut self.files {
            file.seal();
        }

        self.files
    }
}

fn range_start(range: Option<&str>) -> u32 {
    range
        .and_then(|range| range[1..].split(',').next())
        .and_then(|number| number.parse::<u32>().ok())
        .unwrap_or(0)
}

fn update_file_header(file: &mut FileDiff, line: &str) {
    if line.starts_with("new file mode") {
        file.status = Status::New;
    } else if line.starts_with("deleted file mode") {
        file.status = Status::Deleted;
    } else if let Some(path) = line.strip_prefix("rename from ") {
        file.status = Status::Renamed;
        file.old_path = Some(path.to_string());
    } else if let Some(path) = line.strip_prefix("rename to ") {
        file.path = path.to_string();
    } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
        file.binary = true;
    } else if let Some(path) = line.strip_prefix("+++ ")
        && path != "/dev/null"
    {
        file.path = path.strip_prefix("b/").unwrap_or(path).to_string();
    }
}

/// Review order: category, then the most changed lines first, then the path.
pub(crate) fn order(files: &mut [FileDiff]) {
    files.sort_by(|left, right| {
        left.cat
            .cmp(&right.cat)
            .then((right.add + right.del).cmp(&(left.add + left.del)))
            .then(left.path.cmp(&right.path))
    });
}
/// A new-file diff of untracked file PATH in DIR.
pub(crate) fn untracked(dir: &Path, path: &str) -> FileDiff {
    let mut file = FileDiff::new(path.to_string());

    file.status = Status::New;
    let full = dir.join(path);
    let big = std::fs::metadata(&full).map_or(true, |metadata| metadata.len() > MAX_FILE);

    match (big, std::fs::read(&full)) {
        (false, Ok(bytes)) if !bytes.contains(&0) => {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<DLine> = text
                .lines()
                .enumerate()
                .map(|(i, line)| DLine {
                    kind: Kind::Add,
                    old: None,
                    new: Some(i as u32 + 1),
                    text: line.replace('\t', "    "),
                })
                .collect();

            if !lines.is_empty() {
                file.hunks.push(Hunk {
                    ctx: String::new(),
                    old: 0,
                    new: 1,
                    lines,
                });
            }
        }
        _ => file.binary = true,
    }

    file.seal();

    file
}
pub(crate) fn z_paths(text: &str) -> Vec<String> {
    text.split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

fn begin_file(header: &str, parser: &mut DiffParser) {
    parser.flush();
    let path = header
        .rsplit_once(" b/")
        .map_or(header, |(_, path)| path)
        .trim_matches('"')
        .to_string();

    parser.files.push(FileDiff::new(path));
}

fn begin_hunk(header: &str, parser: &mut DiffParser) {
    parser.flush();
    let (ranges, context) = header.split_once(" @@").unwrap_or((header, ""));
    let mut ranges = ranges.split(' ');

    parser.old_line = range_start(ranges.next());
    parser.new_line = range_start(ranges.next());
    parser.hunk = Some(Hunk {
        ctx: context.trim().to_string(),
        old: parser.old_line,
        new: parser.new_line,
        lines: Vec::new(),
    });
}

fn hunk_line(line: &str, parser: &mut DiffParser) {
    let Some(hunk) = parser.hunk.as_mut() else { return };

    let (kind, text) = match line.as_bytes().first() {
        Some(b'+') => (Kind::Add, &line[1..]),
        Some(b'-') => (Kind::Del, &line[1..]),
        Some(b' ') => (Kind::Ctx, &line[1..]),
        None => (Kind::Ctx, ""),
        _ => return,
    };

    let (old, new) = match kind {
        Kind::Ctx => (Some(parser.old_line), Some(parser.new_line)),
        Kind::Add => (None, Some(parser.new_line)),
        Kind::Del => (Some(parser.old_line), None),
    };

    if kind != Kind::Add {
        parser.old_line += 1;
    }

    if kind != Kind::Del {
        parser.new_line += 1;
    }

    hunk.lines.push(DLine {
        kind,
        old,
        new,
        text: text.replace('\t', "    "),
    });
}
