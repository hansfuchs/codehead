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
    if let Some(path) = std::env::var_os("CODEHEAD_MCP").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
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
    let mut chars = name.chars();

    chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && name.len() <= 40
        && chars.all(|rest| rest.is_ascii_alphanumeric() || rest == '-' || rest == '_')
}

/// Every server, by name. A missing file: 0 servers; a broken one: Err.
pub(crate) fn load() -> Result<Vec<Server>, String> {
    let path = path();

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };

    let file: FileRec = toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;

    Ok(file
        .servers
        .into_iter()
        .map(|(name, server)| Server { name, ..server })
        .collect())
}

/// Every server, 0 on a broken file (a session still starts).
pub(crate) fn all() -> Vec<Server> {
    load().unwrap_or_default()
}

pub(crate) fn save(servers: &[Server]) -> Result<(), String> {
    for server in servers {
        server.check()?;
    }

    let file = FileRec {
        servers: servers
            .iter()
            .map(|server| (server.name.clone(), server.clone()))
            .collect(),
    };
    let text = toml::to_string_pretty(&file).map_err(|error| error.to_string())?;
    let path = path();

    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    }

    super::write_atomic(&path, text.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
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
            "stdio" if self.command.as_deref().is_none_or(|command| command.trim().is_empty()) => {
                Err(format!("{}: stdio needs a command", self.name))
            }
            "http" | "sse" if self.url.as_deref().is_none_or(|url| !url.starts_with("http")) => {
                Err(format!("{}: {} needs an http(s) URL", self.name, self.kind))
            }
            "stdio" | "http" | "sse" => Ok(()),
            kind => Err(format!("{}: type {kind:?}: stdio, http or sse", self.name)),
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
    value.strip_prefix("${")?.strip_suffix('}').filter(|var| {
        !var.is_empty()
            && var
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// SECRET into the OS keychain as `<server>/<key>`: its reference, or Err(why) (0 keychain:
/// the caller offers a `${VAR}` reference).
pub(crate) fn store_secret(server: &str, key: &str, secret: &str) -> Result<String, String> {
    let account = format!("{server}/{key}");
    let entry = keyring::Entry::new(SERVICE, &account).map_err(|error| format!("keychain: {error}"))?;

    entry
        .set_password(secret)
        .map_err(|error| format!("keychain: {error}"))?;
    Ok(format!("keychain:{account}"))
}

/// The keychain entry of reference V, if it is one, gone.
pub(crate) fn forget_secret(value: &str) {
    if let Some(account) = value.strip_prefix("keychain:")
        && let Ok(entry) = keyring::Entry::new(SERVICE, account)
    {
        let _ = entry.delete_credential();
    }
}

/// The value of V: a keychain entry, an env var, else V itself.
pub(crate) fn resolve(value: &str) -> Result<String, String> {
    if let Some(account) = value.strip_prefix("keychain:") {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|error| format!("keychain {account}: {error}"))?;

        return entry
            .get_password()
            .map_err(|error| format!("keychain {account}: {error}"));
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
        .filter(|server| agent.mcp.contains(&server.name))
        .collect()
}

/// A short stamp of what agent A's servers are: a session with another stamp runs an old set.
pub(crate) fn stamp(agent: &Agent) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    for server in for_agent(agent) {
        toml::to_string(&server).unwrap_or_default().hash(&mut hasher);
        server.name.hash(&mut hasher);
    }

    format!("{:016x}", hasher.finish())
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

    for server in servers {
        let mut secret = |reference: &str| -> Result<String, String> {
            let value = resolve(reference)?;
            let var = format!("CODEHEAD_MCP_{}", env.len());

            env.push((var.clone(), value));
            Ok(format!("${{{var}}}"))
        };

        let mut map = |values: &BTreeMap<String, String>| -> Result<serde_json::Map<String, Value>, String> {
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), Value::String(secret(value)?))))
                .collect()
        };

        let entry = match server.kind.as_str() {
            "stdio" => map(&server.env)
                .map(|env| json!({"type": "stdio", "command": server.command, "args": server.args, "env": env})),
            kind => map(&server.headers).map(|headers| json!({"type": kind, "url": server.url, "headers": headers})),
        };

        match entry {
            Ok(entry) => {
                entries.insert(server.name.clone(), entry);
            }
            Err(why) => {
                errors.push((server.name.clone(), why));
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

    for server in servers {
        let pairs = |values: &BTreeMap<String, String>| -> Result<Vec<Value>, String> {
            values
                .iter()
                .map(|(key, value)| Ok(json!({"name": key, "value": resolve(value)?})))
                .collect()
        };

        let entry = match server.kind.as_str() {
            "stdio" => pairs(&server.env)
                .map(|env| json!({"name": server.name, "command": server.command, "args": server.args, "env": env})),
            "http" if !http => Err("harness: 0 HTTP MCP support (ACP mcpCapabilities.http)".into()),
            "sse" if !sse => Err("harness: 0 SSE MCP support (ACP mcpCapabilities.sse)".into()),
            kind => pairs(&server.headers)
                .map(|headers| json!({"type": kind, "name": server.name, "url": server.url, "headers": headers})),
        };

        match entry {
            Ok(entry) => out.push(entry),
            Err(error) => errors.push((server.name.clone(), error)),
        }
    }

    (out, errors)
}

/// An ACP tool call TOOL (its title) of an auto-allowed server among SERVERS.
pub(crate) fn acp_auto_allowed(tool: &str, servers: &[Server]) -> bool {
    let tool = tool.to_ascii_lowercase();

    servers.iter().filter(|server| server.auto_allow).any(|server| {
        let name = server.name.to_ascii_lowercase();

        ["__", "_", ".", "/", ":", " "].iter().any(|sep| {
            tool.starts_with(&format!("{name}{sep}"))
                || tool.contains(&format!("mcp__{name}__"))
                || tool.contains(&format!(" {name}{sep}"))
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
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
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
        let known = all.entry(harness.to_string()).or_default();

        for (name, mut status) in rows {
            retain_tool_count(known, &name, &mut status);

            known.insert(name, status);
        }

        super::write_atomic(
            &status_path(),
            serde_json::to_string(&all).unwrap_or_default().as_bytes(),
        )
    });
}

fn retain_tool_count(known: &mut BTreeMap<String, Status>, name: &String, status: &mut Status) {
    if status.tools.is_none() {
        status.tools = known.get(name).and_then(|last| last.tools);
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
        .filter_map(|entry| {
            let name = entry["name"].as_str()?.to_string();

            // codehead's own server; `pending`: still connecting at init, 0 news
            if name == "codehead" || entry["status"] == "pending" {
                return None;
            }

            let key: String = name.chars().map(sanitized_char).collect();

            let prefix = format!("mcp__{key}__");
            let tools_count = tools.iter().filter(|tool| tool.starts_with(&prefix)).count();

            let state = match entry["status"].as_str().unwrap_or_default() {
                "connected" => "connected",
                "needs-auth" | "needs_auth" => "needs-auth",
                "failed" => "failed",
                "disabled" => "off",
                _ => "unknown",
            };

            let source = entry["source"].as_str().unwrap_or_default().to_string();

            Some((
                name,
                Status {
                    state: state.into(),
                    tools: (state == "connected").then_some(tools_count),
                    error: String::new(),
                    source,
                    at,
                },
            ))
        })
        .collect()
}

fn sanitized_char(character: char) -> char {
    if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
        character
    } else {
        '_'
    }
}

#[cfg(test)]
#[path = "servers_tests.rs"]
mod tests;
