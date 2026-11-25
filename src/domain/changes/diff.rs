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
            .flat_map(|height| &height.lines)
            .filter(|line| line.kind == Kind::Add)
            .count();
        self.del = self
            .hunks
            .iter()
            .flat_map(|height_value| &height_value.lines)
            .filter(|line_value| line_value.kind == Kind::Del)
            .count();
        self.cat = categorize(&self.path);
        let mut height_2 = std::collections::hash_map::DefaultHasher::new();

        (&self.path, self.binary, self.status as u8).hash(&mut height_2);

        for line_3 in self
            .hunks
            .iter()
            .flat_map(|height_3| &height_3.lines)
            .filter(|line_2| line_2.kind != Kind::Ctx)
        {
            (line_3.kind as u8, &line_3.text).hash(&mut height_2);
        }

        self.hash = height_2.finish();
    }
    /// The new-side line of the 1st change (deleted file: 1).
    pub(crate) fn first_line(&self) -> u32 {
        self.hunks
            .iter()
            .flat_map(|height| &height.lines)
            .find(|line| line.kind != Kind::Ctx)
            .and_then(|line_value| line_value.new.or(line_value.old))
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

fn line_number(range: Option<&str>) -> u32 {
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
    files.sort_by(|agent, bytes| {
        agent
            .cat
            .cmp(&bytes.cat)
            .then((bytes.add + bytes.del).cmp(&(agent.add + agent.del)))
            .then(agent.path.cmp(&bytes.path))
    });
}
/// A new-file diff of untracked file PATH in DIR.
pub(crate) fn untracked(dir: &Path, path: &str) -> FileDiff {
    let mut file = FileDiff::new(path.to_string());

    file.status = Status::New;
    let full = dir.join(path);
    let big = std::fs::metadata(&full).map_or(true, |message| message.len() > MAX_FILE);

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

fn begin_file(header: &str, model_state: &mut DiffParser) {
    model_state.flush();
    let path = header
        .rsplit_once(" b/")
        .map_or(header, |(_, path)| path)
        .trim_matches('"')
        .to_string();

    model_state.files.push(FileDiff::new(path));
}

fn begin_hunk(header: &str, model_state: &mut DiffParser) {
    model_state.flush();
    let (ranges, context) = header.split_once(" @@").unwrap_or((header, ""));
    let mut ranges = ranges.split(' ');

    model_state.old_line = line_number(ranges.next());
    model_state.new_line = line_number(ranges.next());
    model_state.hunk = Some(Hunk {
        ctx: context.trim().to_string(),
        old: model_state.old_line,
        new: model_state.new_line,
        lines: Vec::new(),
    });
}

fn hunk_line(line: &str, model_state: &mut DiffParser) {
    let Some(hunk) = model_state.hunk.as_mut() else { return };

    let (kind, text) = match line.as_bytes().first() {
        Some(b'+') => (Kind::Add, &line[1..]),
        Some(b'-') => (Kind::Del, &line[1..]),
        Some(b' ') => (Kind::Ctx, &line[1..]),
        None => (Kind::Ctx, ""),
        _ => return,
    };

    let (old, new) = match kind {
        Kind::Ctx => (Some(model_state.old_line), Some(model_state.new_line)),
        Kind::Add => (None, Some(model_state.new_line)),
        Kind::Del => (Some(model_state.old_line), None),
    };

    if kind != Kind::Add {
        model_state.old_line += 1;
    }

    if kind != Kind::Del {
        model_state.new_line += 1;
    }

    hunk.lines.push(DLine {
        kind,
        old,
        new,
        text: text.replace('\t', "    "),
    });
}
