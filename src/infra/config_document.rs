use super::{BTreeMap, File, parse};

pub(crate) fn read_file(path: &std::path::Path) -> Result<(String, File), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error_value) => return Err(error_value.to_string()),
    };

    let file = parse(&text)?;

    Ok((text, file))
}

pub(super) fn document(base: &str, file: &File) -> Result<String, String> {
    let mut doc = base
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| error.to_string())?;

    for (key, value) in [
        ("notes", &file.notes),
        ("home", &file.home),
        ("device", &file.device),
        ("repo", &file.repo),
        ("styles", &file.styles),
        ("agent", &file.agent),
        ("prefix", &file.prefix),
        ("models", &file.models),
        ("messages", &file.messages),
        ("thoughts", &file.thoughts),
        ("edits", &file.edits),
        ("rank", &file.rank),
        ("classify", &file.classify),
        ("review", &file.review),
        ("editor", &file.editor),
        ("ports", &file.ports),
        ("previews", &file.previews),
    ] {
        set_scalar(&mut doc, key, value.as_deref());
    }

    let keys: BTreeMap<String, String> = [("menu", &file.menu_key), ("command", &file.command_key)]
        .into_iter()
        .filter_map(|(key, value)| value.clone().map(|value| (key.to_string(), value)))
        .collect();
    let mut colors = file.colors.clone();

    if let Some(scheme) = &file.scheme {
        colors.insert("scheme".into(), scheme.clone());
    }

    sync_table(&mut doc, "keys", &keys, false)?;
    sync_table(&mut doc, "colors", &colors, false)?;
    sync_harness(&mut doc, &file.harness)?;
    sync_table(&mut doc, "aliases", &file.aliases, true)?;
    let out = doc.to_string();

    parse(&out)?;

    Ok(out)
}

fn set_scalar(doc: &mut toml_edit::DocumentMut, key: &str, value: Option<&str>) {
    let Some(value) = value else {
        doc.remove(key);
        return;
    };

    if doc.get(key).and_then(toml_edit::Item::as_str) == Some(value) {
        return;
    }

    let mut new = toml_edit::Value::from(value);

    if let Some(old) = doc.get(key).and_then(toml_edit::Item::as_value) {
        new.decor_mut().clone_from(old.decor());
    }

    doc[key] = toml_edit::Item::Value(new);
}

fn sync_table(
    doc: &mut toml_edit::DocumentMut,
    name: &str,
    values: &BTreeMap<String, String>,
    preserve_decor: bool,
) -> Result<(), String> {
    if values.is_empty() {
        doc.remove(name);
        return Ok(());
    }

    if !doc.contains_key(name) {
        doc[name] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    let table = doc[name]
        .as_table_like_mut()
        .ok_or_else(|| format!("{name}: must be a table"))?;

    sync_values(table, values, preserve_decor);
    Ok(())
}

fn sync_values(table: &mut dyn toml_edit::TableLike, values: &BTreeMap<String, String>, preserve_decor: bool) {
    let removed: Vec<String> = table
        .iter()
        .filter(|(key, _)| !values.contains_key(*key))
        .map(|(key, _)| key.to_string())
        .collect();

    for key in removed {
        table.remove(&key);
    }

    for (key, value) in values {
        set_table_value(table, key, value, preserve_decor);
    }
}

fn set_table_value(table: &mut dyn toml_edit::TableLike, key: &str, value: &str, preserve_decor: bool) {
    if table.get(key).and_then(toml_edit::Item::as_str) == Some(value) {
        return;
    }

    let mut new = toml_edit::Value::from(value);

    if preserve_decor {
        preserve_table_value(table, key, &mut new);
    }

    if preserve_decor && let Some(old) = table.get_mut(key) {
        *old = toml_edit::Item::Value(new);
        return;
    }

    table.insert(key, toml_edit::Item::Value(new));
}

fn preserve_table_value(table: &dyn toml_edit::TableLike, key: &str, new: &mut toml_edit::Value) {
    if let Some(old) = table.get(key).and_then(toml_edit::Item::as_value) {
        new.decor_mut().clone_from(old.decor());
    }
}

fn sync_harness(
    doc: &mut toml_edit::DocumentMut,
    harness: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<(), String> {
    if harness.is_empty() {
        doc.remove("harness");
        return Ok(());
    }

    if !doc.contains_key("harness") {
        let mut table = toml_edit::Table::new();

        table.set_implicit(true);
        doc["harness"] = toml_edit::Item::Table(table);
    }

    let all = doc["harness"].as_table_mut().ok_or("harness: must be a table")?;
    let removed: Vec<String> = all
        .iter()
        .filter(|(key, _)| !harness.contains_key(*key))
        .map(|(key, _)| key.to_string())
        .collect();

    for key in removed {
        all.remove(&key);
    }

    for (name, fields) in harness {
        if !all.contains_key(name) {
            all.insert(name, toml_edit::Item::Table(toml_edit::Table::new()));
        }

        let table = all[name.as_str()]
            .as_table_like_mut()
            .ok_or_else(|| format!("harness.{name}: must be a table"))?;

        sync_values(table, fields, false);
    }

    Ok(())
}

/// Preserve comments; reject a concurrent edit; replace the file atomically.
pub(crate) fn save_file(path: &std::path::Path, base: &str, file: &File) -> Result<String, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = target.as_path();
    let text = document(base, file)?;
    let (current, _) = read_file(path)?;

    if current != base {
        return Err("config changed outside codehead; reload before save".into());
    }

    if let Some(parent) = path.parent().filter(|path_value| !path_value.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), seq));
    let result = (|| {
        let mut out = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|error_value| error_value.to_string())?;

        out.write_all(text.as_bytes()).map_err(|error_2| error_2.to_string())?;
        out.sync_all().map_err(|error_3| error_3.to_string())?;

        // Check again after preparing the replacement.
        if read_file(path)?.0 != base {
            return Err("config changed outside codehead; reload before save".into());
        }

        std::fs::rename(&tmp, path).map_err(|error_4| error_4.to_string())?;
        Ok(text)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }

    result
}
