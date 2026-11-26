use super::{File, validate, validate_alias};

pub(super) fn parse(text: &str) -> Result<File, String> {
    let table: toml::Table = text.parse().map_err(|error: toml::de::Error| error.to_string())?;
    let mut file = File::default();

    for (key, value) in table {
        parse_entry(&mut file, &key, &value)?;
    }

    Ok(file)
}

fn parse_entry(file: &mut File, key: &str, value: &toml::Value) -> Result<(), String> {
    match key {
        "keys" => parse_keys(file, value),
        "colors" => parse_colors(file, value),
        "harness" => parse_harness(file, value),
        "aliases" => parse_aliases(file, value),
        "prices" => super::prices::parse_prices(file, value),
        _ => parse_scalar(file, key, value),
    }
}

fn parse_keys(file: &mut File, value: &toml::Value) -> Result<(), String> {
    for (name, value) in value.as_table().ok_or("keys: must be a table")? {
        let value = value
            .as_str()
            .ok_or_else(|| format!("keys.{name}: must be a string"))?
            .to_string();

        validate(&format!("keys.{name}"), &value)?;

        match name.as_str() {
            "menu" => file.menu_key = Some(value),
            "command" => file.command_key = Some(value),
            _ => return Err(format!("unknown key keys.{name}")),
        }
    }

    Ok(())
}

fn parse_colors(file: &mut File, value: &toml::Value) -> Result<(), String> {
    for (name, value) in value.as_table().ok_or("colors: must be a table")? {
        let value = value
            .as_str()
            .ok_or_else(|| format!("colors.{name}: must be a string"))?
            .to_string();

        validate(&format!("colors.{name}"), &value)?;

        if name == "scheme" {
            file.scheme = Some(value);
        } else {
            file.colors
                .insert(crate::domain::palette::slot_name(name).to_string(), value);
        }
    }

    Ok(())
}

fn parse_harness(file: &mut File, value: &toml::Value) -> Result<(), String> {
    for (name, table) in value.as_table().ok_or("harness: must be a table")? {
        for (field, value) in table
            .as_table()
            .ok_or_else(|| format!("harness.{name}: must be a table"))?
        {
            let key = format!("harness.{name}.{field}");
            let value = value
                .as_str()
                .ok_or_else(|| format!("{key}: must be a string"))?
                .to_string();

            validate(&key, &value)?;
            file.harness
                .entry(name.clone())
                .or_default()
                .insert(field.clone(), value);
        }
    }

    Ok(())
}

fn parse_aliases(file: &mut File, value: &toml::Value) -> Result<(), String> {
    for (name, target) in value.as_table().ok_or("aliases: must be a table")? {
        let target = target
            .as_str()
            .ok_or_else(|| format!("alias {name}: must be a string"))?;

        validate_alias(name, target)?;
        file.aliases.insert(name.clone(), target.to_string());
    }

    Ok(())
}

fn parse_scalar(file: &mut File, key: &str, value: &toml::Value) -> Result<(), String> {
    let value = value
        .as_str()
        .ok_or_else(|| format!("{key}: must be a string"))?
        .to_string();

    if matches!(
        key,
        "thoughts" | "messages" | "edits" | "previews" | "ports" | "rank" | "classify" | "review"
    ) {
        validate(key, &value)?;
    }

    match key {
        "notes" | "home" | "device" | "repo" | "styles" | "agent" | "prefix" | "models" | "thoughts" | "messages"
        | "edits" | "editor" | "previews" | "ports" | "rank" | "classify" | "review" => file.set(key, Some(value)),
        _ => return Err(format!("unknown key {key}")),
    }

    Ok(())
}
