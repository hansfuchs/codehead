use super::{File, PLAIN_KEYS, PathBuf, expand, home_repo, read_file, save_file};

/// Where setting KEY lives once the home repo has its codehead dir: codehead.toml of this
/// machine (the pointer to home), this device's file in home, or home's shared settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    Local,
    Device,
    Shared,
}

pub(crate) fn scope(key: &str) -> Scope {
    match key {
        "home" | "device" => Scope::Local,
        // paths and programs of this machine
        "notes" | "styles" | "repo" | "editor" | "ports" | "previews" => Scope::Device,
        key_value if key_value.starts_with("aliases.") => Scope::Device,
        key_2 if key_2.starts_with("harness.") && key_2.ends_with(".command") => Scope::Device,
        _ => Scope::Shared,
    }
}

/// 1 settings file: its path, its text as read (the base of a save), its values.
#[derive(Clone, Debug)]
pub(crate) struct Layer {
    pub path: PathBuf,
    pub text: String,
    pub file: File,
}

impl Layer {
    fn read(path: PathBuf) -> Result<Layer, String> {
        let (text, file) = read_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;

        Ok(Layer { path, text, file })
    }
}

/// The settings files of this machine: codehead.toml; with the home repo's codehead dir also
/// home's shared settings (`codehead/settings.toml`) and this device's file
/// (`codehead/devices/<device>.toml`). Per key: device, then shared, then codehead.toml.
#[derive(Clone, Debug)]
pub(crate) struct Layers {
    pub local: Layer,
    pub shared: Option<Layer>,
    pub device: Option<Layer>,
}

impl Layers {
    /// The values of all files, 1 per key.
    pub(crate) fn merged(&self) -> File {
        let mut file_value = self.local.file.clone();

        for line in [&self.shared, &self.device].into_iter().flatten() {
            overlay(&mut file_value, &line.file);
        }

        // the pointer is this machine's alone
        file_value.home.clone_from(&self.local.file.home);
        file_value.device.clone_from(&self.local.file.device);
        file_value
    }
}

/// The values of TOP over BASE.
pub(super) fn overlay(base: &mut File, top: &File) {
    for key in PLAIN_KEYS {
        if let Some(value) = top.get(key) {
            base.set(key, Some(value));
        }
    }

    base.colors.extend(top.colors.clone());
    base.aliases.extend(top.aliases.clone());

    for (height, thread) in &top.harness {
        base.harness.entry(height.clone()).or_default().extend(thread.clone());
    }
}

/// F split by scope: [codehead.toml, shared, device].
pub(super) fn split(file: &File) -> [File; 3] {
    let mut out = [File::default(), File::default(), File::default()];
    let at = |key: &str| match scope(key) {
        Scope::Local => 0,
        Scope::Shared => 1,
        Scope::Device => 2,
    };

    for key_value in PLAIN_KEYS {
        if let Some(value) = file.get(key_value) {
            out[at(key_value)].set(key_value, Some(value));
        }
    }

    out[1].colors = file.colors.clone();
    out[2].aliases = file.aliases.clone();

    for (height, thread) in &file.harness {
        for (field, value_value) in thread {
            let key_2 = format!("harness.{height}.{field}");

            out[at(&key_2)].set(&key_2, Some(value_value.clone()));
        }
    }

    out
}

/// The codehead dir of home repo HOME (`<home>/codehead`), when it exists.
pub(crate) fn layout_of(home: &std::path::Path) -> Option<PathBuf> {
    let directory = home.join("codehead");

    directory.is_dir().then_some(directory)
}

/// The home repo's codehead dir: agents, flows, mcp.toml, settings live there. None: 0 home
/// repo or 0 codehead dir in it (tests: always None, never the owner's home).
pub(crate) fn layout() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }

    home_repo().and_then(|height| layout_of(&height))
}

/// S as a device name: lower case, letters, digits and `-`; the host name's 1st label.
pub(crate) fn slug(text: &str) -> String {
    let first = text.trim().split('.').next().unwrap_or_default().to_lowercase();
    let mut out = String::new();

    for character in first.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }

    let out = out.trim_end_matches('-').to_string();

    if out.is_empty() { "device".into() } else { out }
}

/// The host name as a device name.
pub(crate) fn host_device() -> String {
    let mut buf = [0u8; 256];
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let end = buf.iter().position(|&bytes| bytes == 0).unwrap_or(buf.len());

    slug(if ok {
        std::str::from_utf8(&buf[..end]).unwrap_or_default()
    } else {
        ""
    })
}

/// The device name of F: `device`, else the host name.
pub(crate) fn device_of(file: &File) -> String {
    file.device
        .as_deref()
        .filter(|directory| !directory.trim().is_empty())
        .map_or_else(host_device, slug)
}

/// The settings files of codehead.toml LOCAL (its `home` names the home repo; env never: a
/// test inside a codehead thread must not reach the owner's home).
pub(crate) fn read_layers(local: &std::path::Path) -> Result<Layers, String> {
    let local = Layer::read(local.to_path_buf())?;

    let Some(dir) = local
        .file
        .home
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .map(expand)
        .and_then(|height| layout_of(&height))
    else {
        return Ok(Layers {
            local,
            shared: None,
            device: None,
        });
    };

    let shared = Layer::read(dir.join("settings.toml"))?;
    let device = Layer::read(dir.join("devices").join(format!("{}.toml", device_of(&local.file))))?;

    Ok(Layers {
        local,
        shared: Some(shared),
        device: Some(device),
    })
}

/// Save F (all values, as `merged` gives them) into the files of LAYERS, each key into the file
/// of its scope; only changed files are written. A new `home` or `device`: only the pointer in
/// codehead.toml changes (the files of the new place stay). Ok: the files read again.
pub(crate) fn save_layers(layers: &Layers, file_value: &File) -> Result<Layers, String> {
    let (Some(shared), Some(device)) = (&layers.shared, &layers.device) else {
        save_file(&layers.local.path, &layers.local.text, file_value)?;
        return read_layers(&layers.local.path);
    };

    if file_value.home != layers.local.file.home || file_value.device != layers.local.file.device {
        let mut local = layers.local.file.clone();

        local.home.clone_from(&file_value.home);
        local.device.clone_from(&file_value.device);
        save_file(&layers.local.path, &layers.local.text, &local)?;
        return read_layers(&layers.local.path);
    }

    let [local, sh, dev] = split(file_value);

    for (layer, new) in [(&layers.local, local), (shared, sh), (device, dev)] {
        if layer.file != new {
            save_file(&layer.path, &layer.text, &new)?;
        }
    }

    read_layers(&layers.local.path)
}
