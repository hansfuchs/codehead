use super::{Event, Read, Seek, SeekFrom, Tail, Write, events_path};

/// Append EVENT to thread ID's log: 1 `write` per line (`O_APPEND`: lines never interleave).
pub(crate) fn append(id: &str, event: &Event) -> Result<(), String> {
    let path = events_path(id);
    let mut line = serde_json::to_vec(event).map_err(|error| error.to_string())?;

    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;

    file.write_all(&line)
        .map_err(|error| format!("{}: {error}", path.display()))
}
/// The events in the last BYTES of thread ID's log (a cut 1st line left out).
pub(crate) fn events_tail(id: &str, bytes: u64) -> Vec<Event> {
    let Ok(mut file) = std::fs::File::open(events_path(id)) else {
        return Vec::new();
    };

    let len = file.metadata().map_or(0, |metadata| metadata.len());
    let from = len.saturating_sub(bytes);

    if file.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }

    let mut buf = Vec::new();

    if file.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }

    let body = if from > 0 {
        buf.iter()
            .position(|&byte| byte == b'\n')
            .map_or(&[][..], |i| &buf[i + 1..])
    } else {
        &buf[..]
    };

    body.split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}
/// Every event of thread ID.
pub(crate) fn events(id: &str) -> Vec<Event> {
    let mut tail = Tail::new(id);

    tail.read()
}
impl Tail {
    pub(crate) fn new(id: &str) -> Tail {
        Tail {
            path: events_path(id),
            offset: 0,
            partial: Vec::new(),
        }
    }
    /// A tail from the last BYTES of the log (its 1st whole line on): the start of the 1st line it
    /// reads, the events before it are `head`'s.
    pub(crate) fn from_end(id: &str, bytes: u64) -> (Tail, u64) {
        let path = events_path(id);
        let len = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());

        if len <= bytes {
            return (
                Tail {
                    path,
                    offset: 0,
                    partial: Vec::new(),
                },
                0,
            );
        }

        let from = len - bytes;

        // the line cut at FROM: its rest skipped
        let mut start = from;

        if let Ok(mut file) = std::fs::File::open(&path)
            && file.seek(SeekFrom::Start(from - 1)).is_ok()
        {
            let cut = Vec::new();

            skip_partial_line(from, &mut start, cut, file);
        }

        (
            Tail {
                path,
                offset: start,
                partial: Vec::new(),
            },
            start,
        )
    }
}

fn skip_partial_line(from: u64, start: &mut u64, mut cut: Vec<u8>, file: std::fs::File) {
    if std::io::BufRead::read_until(&mut std::io::BufReader::new(file), b'\n', &mut cut).is_ok() {
        *start = from - 1 + cut.len() as u64;
    }
}
/// The events of thread ID's log before byte END (the part a `Tail::from_end` skipped).
pub(crate) fn head(id: &str, end: u64) -> Vec<Event> {
    let Ok(mut file) = std::fs::File::open(events_path(id)) else {
        return Vec::new();
    };

    let mut buf = vec![0u8; end as usize];

    if file.read_exact(&mut buf).is_err() {
        return Vec::new();
    }

    buf.split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}
impl Tail {
    /// Events written since the last read.
    pub(crate) fn read(&mut self) -> Vec<Event> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };

        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }

        let mut buf = Vec::new();

        if file.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }

        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);

        let Some(end) = self.partial.iter().rposition(|&byte| byte == b'\n') else {
            return Vec::new();
        };

        let rest = self.partial.split_off(end + 1);
        let done = std::mem::replace(&mut self.partial, rest);

        done.split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect()
    }
}
