use super::{HashMap, Item, Path, SystemTime, age, secs};
use std::path::PathBuf;

/// Artifact urls (`https://claude.ai/…artifact/<id>`) in TEXT, the share key (`?sk=…`) left out.
pub(crate) fn artifact_urls(text: &[u8]) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::bytes::Regex> = std::sync::OnceLock::new();

    let re = RE.get_or_init(|| {
        regex::bytes::Regex::new(r"https://claude\.ai/(?:[a-z]+/)?artifact/[A-Za-z0-9_-]{8,}").expect("valid regex")
    });

    let mut urls: Vec<String> = re
        .find_iter(text)
        .map(|found| String::from_utf8_lossy(found.as_bytes()).into_owned())
        .collect();

    urls.sort();
    urls.dedup();

    urls
}

/// Titles explicitly attached to artifact links in decoded transcript text.
pub(super) fn artifact_titles(text: &str, titles: &mut HashMap<String, String>) {
    static LINKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

    let re = LINKS.get_or_init(|| {
        regex::Regex::new(r"\[([^\]\n]+)\]\(<?(https://claude\.ai/(?:[a-z]+/)?artifact/[A-Za-z0-9_-]{8,})[^)]*\)")
            .unwrap()
    });

    for captures in re.captures_iter(text) {
        let title = captures[1].trim();

        if !title.is_empty() && !title.contains("claude.ai/") && !title.starts_with("http") {
            titles.insert(captures[2].to_string(), title.to_string());
        }
    }
}

pub(super) fn transcript_titles(value: &serde_json::Value, titles: &mut HashMap<String, String>, depth: usize) {
    if depth > 24 {
        return;
    }

    match value {
        serde_json::Value::String(text) => {
            artifact_titles(text, titles);

            if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(text) {
                transcript_titles(&decoded, titles, depth + 1);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                transcript_titles(value, titles, depth + 1);
            }
        }
        serde_json::Value::Object(values) => {
            object_titles(titles, depth, values);
        }
        _ => {}
    }
}

fn object_titles(
    titles: &mut HashMap<String, String>,
    depth: usize,
    values: &serde_json::Map<String, serde_json::Value>,
) {
    // Only a direct artifact URL/title pair, never the enclosing issue's title.
    if let Some(title) = values
        .get("title")
        .or_else(|| values.get("name"))
        .and_then(|value| value.as_str())
        .filter(|text| !text.trim().is_empty())
    {
        for key in ["url", "uri", "href", "artifact_url", "artifactUrl"] {
            record_transcript_titles(titles, values, key, title);
        }
    }

    for value in values.values() {
        transcript_titles(value, titles, depth + 1);
    }
}

fn record_transcript_titles(
    titles: &mut HashMap<String, String>,
    values: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    title: &str,
) {
    if let Some(link) = values.get(key).and_then(|value| value.as_str()) {
        for url in artifact_urls(link.as_bytes()) {
            titles.insert(url, title.trim().to_string());
        }
    }
}

pub(super) fn page_title(html: &str) -> Option<String> {
    static TITLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = TITLE
        .get_or_init(|| regex::Regex::new(r"(?s)<[tT][iI][tT][lL][eE][^>]*>(.*?)</[tT][iI][tT][lL][eE]>").unwrap());
    let raw = re.captures(html)?.get(1)?.as_str();
    let title = raw
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title
        .strip_suffix(" - Claude")
        .or_else(|| title.strip_suffix(" | Claude"))
        .unwrap_or(&title)
        .trim();

    if title.is_empty()
        || [
            "claude",
            "claude artifact",
            "claude artifacts",
            "just a moment...",
            "access denied",
        ]
        .contains(&title.to_lowercase().as_str())
    {
        None
    } else {
        Some(title.to_string())
    }
}

/// Public metadata only, bounded time/body size. Never send browser credentials.
pub(super) fn remote_artifact_title(url: &str) -> Option<String> {
    let name = format!("artifact-title-{}", crate::infra::background::scope(Path::new(url)));
    let now = secs(SystemTime::now());

    if let Some((at, title)) = crate::infra::background::snapshot::<(u64, Option<String>)>(&name)
        && now.saturating_sub(at) < 3600
    {
        return title;
    }

    let title = std::process::Command::new("curl")
        .args([
            "--silent",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "2",
            "--max-time",
            "5",
            "--max-filesize",
            "2097152",
            url,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| page_title(&String::from_utf8_lossy(&output.stdout)));

    crate::infra::background::save(&name, &(now, &title));

    title
}

pub(crate) fn scan_artifacts(root: &Path) -> Vec<Item> {
    let mut scan = ArtifactScan {
        root,
        found: HashMap::new(),
        titles: HashMap::new(),
        directories: vec![(root.to_path_buf(), 0)],
    };

    while let Some((directory, depth)) = scan.directories.pop() {
        scan_directory(&mut scan, &directory, depth);
    }

    let ArtifactScan { found, mut titles, .. } = scan;

    let mut items: Vec<Item> = found
        .into_iter()
        .map(|(url, (time, place))| {
            let name = format!("artifact-title-{}", crate::infra::background::scope(Path::new(&url)));
            let saved = crate::infra::background::snapshot::<(u64, Option<String>)>(&name).and_then(|(_, title)| title);
            let label = titles
                .remove(&url)
                .or(saved)
                .unwrap_or_else(|| format!("untitled artifact · {}", url.rsplit('/').next().unwrap_or_default()));
            let detail = [place, age(time)]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");

            Item {
                label,
                detail,
                insert: url,
                time,
                info: String::new(),
            }
        })
        .collect();

    items.sort_by(|left, right| (left.time, &left.label).cmp(&(right.time, &right.label)));

    items
}

struct ArtifactScan<'root> {
    root: &'root Path,
    found: HashMap<String, (u64, String)>,
    titles: HashMap<String, String>,
    directories: Vec<(PathBuf, usize)>,
}

fn scan_directory(scan: &mut ArtifactScan<'_>, directory: &Path, depth: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        scan_entry(scan, &entry, depth);
    }
}

fn scan_entry(scan: &mut ArtifactScan<'_>, entry: &std::fs::DirEntry, depth: usize) {
    let path = entry.path();
    let Ok(metadata) = entry.metadata() else { return };

    if metadata.is_dir() {
        if depth < 3 {
            scan.directories.push((path, depth + 1));
        }

        return;
    }

    if path.extension().is_none_or(|extension| extension != "jsonl") {
        return;
    }

    let Ok(bytes) = std::fs::read(&path) else { return };

    if artifact_urls(&bytes).is_empty() {
        return;
    }

    collect_transcript_titles(&bytes, &mut scan.titles);
    let time = metadata.modified().map_or(0, secs);
    let project = path
        .strip_prefix(scan.root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    let place = project.rsplit('-').next().unwrap_or_default().to_string();

    for url in artifact_urls(&bytes) {
        let entry = scan.found.entry(url).or_insert((0, String::new()));

        if time >= entry.0 {
            *entry = (time, place.clone());
        }
    }
}

fn collect_transcript_titles(bytes: &[u8], titles: &mut HashMap<String, String>) {
    for line in bytes.split(|byte| *byte == b'\n') {
        if let Ok(value) = serde_json::from_slice(line) {
            transcript_titles(&value, titles, 0);
        }
    }
}
