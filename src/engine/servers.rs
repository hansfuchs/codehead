//! MCP servers codehead owns: `<shared>/mcp.toml` (`engine::shared_dir`), 1 table per server. codehead connects each
//! server to every agent whose profile lists it (`mcp = ["linear"]`), in any harness: Claude Code
//! through `--mcp-config`, ACP agents through `mcpServers` of `session/new`. The servers of a
//! harness's own config stay with the harness (union).
//!
//! ```toml
//! [servers.linear]
//! type = "http"                   # stdio | http | sse
//! url = "https://mcp.linear.app/mcp"
//! headers = { Authorization = "keychain:linear/Authorization" }
//! auto_allow = true               # its tools run without an ask (default)
//!
//! [servers.files]
//! type = "stdio"
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
//! env = { TOKEN = "${FILES_TOKEN}" }
//! ```
//!
//! Values of `env` and `headers`: 0 plain text. `keychain:<account>`: the OS keychain (service
//! `codehead-mcp`); `${VAR}`: an env var of codehead, where 0 keychain works.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::roster::Agent;

/// Keychain service of every codehead secret.
const SERVICE: &str = "codehead-mcp";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Server {
    /// The table name, not stored in the table.
    #[serde(skip)]
    pub name: String,
    /// `stdio`, `http` or `sse`.
    #[serde(rename = "type", default = "stdio")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub auto_allow: bool,
}

fn stdio() -> String {
    "stdio".into()
}

fn yes() -> bool {
    true
}

#[derive(Default, Serialize, Deserialize)]
struct FileRec {
    #[serde(default)]
    servers: BTreeMap<String, Server>,
}

pub(crate) fn path() -> PathBuf {
    if let Some(path_2) = std::env::var_os("CODEHEAD_MCP").filter(|path_value| !path_value.is_empty()) {
        return PathBuf::from(path_2);
    }

    // tests never read or write the owner's servers
    if cfg!(test) {
        return super::data_dir().join("mcp.toml");
    }

    super::shared_dir().join("mcp.toml")
}

/// Server names: a letter, then letters, digits, `-` or `_`; max. 40 chars (tool names:
/// `mcp__<name>__<tool>`).
pub(crate) fn valid_name(name: &str) -> bool {
    let mut character = name.chars();

    character.next().is_some_and(|file| file.is_ascii_alphabetic())
        && name.len() <= 40
        && character.all(|x| x.is_ascii_alphanumeric() || x == '-' || x == '_')
}

/// Every server, by name. A missing file: 0 servers; a broken one: Err.
pub(crate) fn load() -> Result<Vec<Server>, String> {
    let path_value = path();

    let text = match std::fs::read_to_string(&path_value) {
        Ok(thread) => thread,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error_value) => return Err(format!("{}: {error_value}", path_value.display())),
    };

    let file: FileRec = toml::from_str(&text).map_err(|error_2| format!("{}: {error_2}", path_value.display()))?;

    Ok(file
        .servers
        .into_iter()
        .map(|(name, text_value)| Server { name, ..text_value })
        .collect())
}

/// Every server, 0 on a broken file (a session still starts).
pub(crate) fn all() -> Vec<Server> {
    load().unwrap_or_default()
}

pub(crate) fn save(servers: &[Server]) -> Result<(), String> {
    for text_value in servers {
        text_value.check()?;
    }

    let file = FileRec {
        servers: servers
            .iter()
            .map(|text_2| (text_2.name.clone(), text_2.clone()))
            .collect(),
    };
    let text = toml::to_string_pretty(&file).map_err(|error| error.to_string())?;
    let path_value = path();

    if let Some(directory) = path_value.parent() {
        std::fs::create_dir_all(directory).map_err(|error_value| format!("{}: {error_value}", directory.display()))?;
    }

    super::write_atomic(&path_value, text.as_bytes()).map_err(|error_2| format!("{}: {error_2}", path_value.display()))
}

impl Server {
    /// Err(why) of a server codehead cannot start.
    pub(crate) fn check(&self) -> Result<(), String> {
        if !valid_name(&self.name) {
            return Err(format!(
                "server name {:?}: a letter, then letters, digits, - or _ (max. 40)",
                self.name
            ));
        }

        match self.kind.as_str() {
            "stdio"
                if self
                    .command
                    .as_deref()
                    .is_none_or(|character| character.trim().is_empty()) =>
            {
                Err(format!("{}: stdio needs a command", self.name))
            }
            "http" | "sse" if self.url.as_deref().is_none_or(|user| !user.starts_with("http")) => {
                Err(format!("{}: {} needs an http(s) URL", self.name, self.kind))
            }
            "stdio" | "http" | "sse" => Ok(()),
            key => Err(format!("{}: type {key:?}: stdio, http or sse", self.name)),
        }
    }

    /// The command line or URL, for a list row.
    pub(crate) fn target(&self) -> String {
        match self.kind.as_str() {
            "stdio" => std::iter::once(self.command.clone().unwrap_or_default())
                .chain(self.args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
            _ => self.url.clone().unwrap_or_default(),
        }
    }

    /// Env and header values in plain text (a hand-edited file).
    pub(crate) fn plain_secrets(&self) -> Vec<String> {
        self.env
            .iter()
            .chain(&self.headers)
            .filter(|(_, value)| !is_ref(value))
            .map(|(key, _)| key.clone())
            .collect()
    }
}

// ---------- secrets ----------

/// V is a secret reference: `keychain:<account>` or `${VAR}`.
pub(crate) fn is_ref(value: &str) -> bool {
    value.starts_with("keychain:") || env_ref(value).is_some()
}

fn env_ref(value: &str) -> Option<&str> {
    value.strip_prefix("${")?.strip_suffix('}').filter(|count| {
        !count.is_empty()
            && count
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// SECRET into the OS keychain as `<server>/<key>`: its reference, or Err(why) (0 keychain:
/// the caller offers a `${VAR}` reference).
pub(crate) fn store_secret(server: &str, key: &str, secret: &str) -> Result<String, String> {
    let account = format!("{server}/{key}");
    let error_value = keyring::Entry::new(SERVICE, &account).map_err(|error| format!("keychain: {error}"))?;

    error_value
        .set_password(secret)
        .map_err(|error_2| format!("keychain: {error_2}"))?;
    Ok(format!("keychain:{account}"))
}

/// The keychain entry of reference V, if it is one, gone.
pub(crate) fn forget_secret(value: &str) {
    if let Some(account) = value.strip_prefix("keychain:")
        && let Ok(error) = keyring::Entry::new(SERVICE, account)
    {
        let _ = error.delete_credential();
    }
}

/// The value of V: a keychain entry, an env var, else V itself.
pub(crate) fn resolve(value: &str) -> Result<String, String> {
    if let Some(account) = value.strip_prefix("keychain:") {
        let error_value =
            keyring::Entry::new(SERVICE, account).map_err(|error| format!("keychain {account}: {error}"))?;

        return error_value
            .get_password()
            .map_err(|error_2| format!("keychain {account}: {error_2}"));
    }

    if let Some(name) = env_ref(value) {
        return std::env::var(name).map_err(|_| format!("${{{name}}}: not set in codehead's environment"));
    }

    Ok(value.to_string())
}

// ---------- sessions ----------

/// The servers of agent A: those its profile lists, in the order of the file.
pub(crate) fn for_agent(agent: &Agent) -> Vec<Server> {
    if agent.mcp.is_empty() {
        return Vec::new();
    }

    all()
        .into_iter()
        .filter(|text| agent.mcp.contains(&text.name))
        .collect()
}

/// A short stamp of what agent A's servers are: a session with another stamp runs an old set.
pub(crate) fn stamp(agent: &Agent) -> String {
    use std::hash::{Hash, Hasher};
    let mut height = std::collections::hash_map::DefaultHasher::new();

    for text in for_agent(agent) {
        toml::to_string(&text).unwrap_or_default().hash(&mut height);
        text.name.hash(&mut height);
    }

    format!("{:016x}", height.finish())
}

/// Claude Code: the `mcpServers` entries of SERVERS and the env the harness process gets. Secrets
/// stay out of the config (it is a process argument): `${CODEHEAD_MCP_<n>}`, Claude Code
/// expands them from its env. A server with a secret that does not resolve: left out, its error.
type ProcessEnvironment = Vec<(String, String)>;
type ServerErrors = Vec<(String, String)>;
type ClaudeConfig = (serde_json::Map<String, Value>, ProcessEnvironment, ServerErrors);

pub(crate) fn claude_config(servers: &[Server]) -> ClaudeConfig {
    let mut entries = serde_json::Map::new();
    let mut env = Vec::new();
    let mut errors = Vec::new();

    for text in servers {
        let mut secret = |value_value: &str| -> Result<String, String> {
            let value = resolve(value_value)?;
            let var = format!("CODEHEAD_MCP_{}", env.len());

            env.push((var.clone(), value));
            Ok(format!("${{{var}}}"))
        };

        let mut map = |message: &BTreeMap<String, String>| -> Result<serde_json::Map<String, Value>, String> {
            message
                .iter()
                .map(|(key, value_2)| Ok((key.clone(), Value::String(secret(value_2)?))))
                .collect()
        };

        let error = match text.kind.as_str() {
            "stdio" => map(&text.env)
                .map(|env| json!({"type": "stdio", "command": text.command, "args": text.args, "env": env})),
            key_value => {
                map(&text.headers).map(|height| json!({"type": key_value, "url": text.url, "headers": height}))
            }
        };

        match error {
            Ok(value_3) => {
                entries.insert(text.name.clone(), value_3);
            }
            Err(why) => {
                errors.push((text.name.clone(), why));
            }
        }
    }

    (entries, env, errors)
}

/// ACP: the `mcpServers` entries of SERVERS for an agent with HTTP / SSE support (`mcpCapabilities`),
/// with resolved values (they go over the pipe, not the command line). Left out: servers the
/// agent cannot run, servers with a secret that does not resolve; both with their error.
pub(crate) fn acp_config(servers: &[Server], http: bool, sse: bool) -> (Vec<Value>, Vec<(String, String)>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();

    for text in servers {
        let pairs = |message: &BTreeMap<String, String>| -> Result<Vec<Value>, String> {
            message
                .iter()
                .map(|(key, value)| Ok(json!({"name": key, "value": resolve(value)?})))
                .collect()
        };

        let entry = match text.kind.as_str() {
            "stdio" => pairs(&text.env)
                .map(|env| json!({"name": text.name, "command": text.command, "args": text.args, "env": env})),
            "http" if !http => Err("harness: 0 HTTP MCP support (ACP mcpCapabilities.http)".into()),
            "sse" if !sse => Err("harness: 0 SSE MCP support (ACP mcpCapabilities.sse)".into()),
            key_value => pairs(&text.headers)
                .map(|height| json!({"type": key_value, "name": text.name, "url": text.url, "headers": height})),
        };

        match entry {
            Ok(value_value) => out.push(value_value),
            Err(error) => errors.push((text.name.clone(), error)),
        }
    }

    (out, errors)
}

/// An ACP tool call TOOL (its title) of an auto-allowed server among SERVERS.
pub(crate) fn acp_auto_allowed(tool: &str, servers: &[Server]) -> bool {
    let thread = tool.to_ascii_lowercase();

    servers.iter().filter(|text| text.auto_allow).any(|text_value| {
        let count = text_value.name.to_ascii_lowercase();

        ["__", "_", ".", "/", ":", " "].iter().any(|sep| {
            thread.starts_with(&format!("{count}{sep}"))
                || thread.contains(&format!("mcp__{count}__"))
                || thread.contains(&format!(" {count}{sep}"))
        })
    })
}

// ---------- status ----------

/// What a harness reported of 1 server: `<data>/mcp-status.json`, `{harness: {server: Status}}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Status {
    /// connected, needs-auth, failed, pending, off, unknown
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<usize>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Where the harness has it from: codehead, user, project, local, claudeai, plugin.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default)]
    pub at: u64,
}

fn status_path() -> PathBuf {
    super::data_dir().join("mcp-status.json")
}

pub(crate) fn statuses() -> BTreeMap<String, BTreeMap<String, Status>> {
    std::fs::read(status_path())
        .ok()
        .and_then(|thread| serde_json::from_slice(&thread).ok())
        .unwrap_or_default()
}

/// HARNESS reported ROWS (server, status): merged into the file, under its lock (hosts of other
/// threads write it too). A row without a tool count keeps the last count.
pub(crate) fn record(harness: &str, rows: Vec<(String, Status)>) {
    if rows.is_empty() {
        return;
    }

    let dir = super::data_dir();
    let _ = std::fs::create_dir_all(&dir);

    let _ = super::locked(&dir.join("mcp-status.lock"), || {
        let mut all = statuses();
        let height = all.entry(harness.to_string()).or_default();

        for (name, mut text) in rows {
            retain_tool_count(height, &name, &mut text);

            height.insert(name, text);
        }

        super::write_atomic(
            &status_path(),
            serde_json::to_string(&all).unwrap_or_default().as_bytes(),
        )
    });
}

fn retain_tool_count(height: &mut BTreeMap<String, Status>, name: &String, text: &mut Status) {
    if text.tools.is_none() {
        text.tools = height.get(name).and_then(|output| output.tools);
    }
}

/// Claude Code's `system`/`init` event V: (server, status) per entry of `mcp_servers`, the tool
/// count from `tools` (`mcp__<server>__<tool>`). Claude Code sanitizes server names in tool
/// names (`claude.ai Linear` → `claude_ai_Linear`).
pub(crate) fn from_init(value: &Value) -> Vec<(String, Status)> {
    let tools: Vec<&str> = value["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let at = super::now_ms();

    value["mcp_servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|text| {
            let name = text["name"].as_str()?.to_string();

            // codehead's own server; `pending`: still connecting at init, 0 news
            if name == "codehead" || text["status"] == "pending" {
                return None;
            }

            let key: String = name.chars().map(server_name_character).collect();

            let prefix = format!("mcp__{key}__");
            let count_value = tools.iter().filter(|thread| thread.starts_with(&prefix)).count();

            let state = match text["status"].as_str().unwrap_or_default() {
                "connected" => "connected",
                "needs-auth" | "needs_auth" => "needs-auth",
                "failed" => "failed",
                "disabled" => "off",
                _ => "unknown",
            };

            let source = text["source"].as_str().unwrap_or_default().to_string();

            Some((
                name,
                Status {
                    state: state.into(),
                    tools: (state == "connected").then_some(count_value),
                    error: String::new(),
                    source,
                    at,
                },
            ))
        })
        .collect()
}

fn server_name_character(character: char) -> char {
    if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
        character
    } else {
        '_'
    }
}

#[cfg(test)]
#[path = "servers_tests.rs"]
mod tests;
