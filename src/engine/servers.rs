//! MCP servers codehead owns: `<config>/mcp.toml`, 1 table per server. codehead connects each
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
pub struct Server {
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

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("CODEHEAD_MCP").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    // tests never read or write the owner's servers
    if cfg!(test) {
        return super::data_dir().join("mcp.toml");
    }
    super::config_dir().join("mcp.toml")
}

/// Server names: a letter, then letters, digits, `-` or `_`; max. 40 chars (tool names:
/// `mcp__<name>__<tool>`).
pub fn valid_name(name: &str) -> bool {
    let mut c = name.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic()) && name.len() <= 40 && c.all(|x| x.is_ascii_alphanumeric() || x == '-' || x == '_')
}

/// Every server, by name. A missing file: 0 servers; a broken one: Err.
pub fn load() -> Result<Vec<Server>, String> {
    let p = path();
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    let f: FileRec = toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(f.servers.into_iter().map(|(name, s)| Server { name, ..s }).collect())
}

/// Every server, 0 on a broken file (a session still starts).
pub fn all() -> Vec<Server> {
    load().unwrap_or_default()
}

pub fn save(servers: &[Server]) -> Result<(), String> {
    for s in servers {
        s.check()?;
    }
    let f = FileRec { servers: servers.iter().map(|s| (s.name.clone(), s.clone())).collect() };
    let text = toml::to_string_pretty(&f).map_err(|e| e.to_string())?;
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    super::write_atomic(&p, text.as_bytes()).map_err(|e| format!("{}: {e}", p.display()))
}

impl Server {
    /// Err(why) of a server codehead cannot start.
    pub fn check(&self) -> Result<(), String> {
        if !valid_name(&self.name) {
            return Err(format!("server name {:?}: a letter, then letters, digits, - or _ (max. 40)", self.name));
        }
        match self.kind.as_str() {
            "stdio" if self.command.as_deref().is_none_or(|c| c.trim().is_empty()) => Err(format!("{}: stdio needs a command", self.name)),
            "http" | "sse" if self.url.as_deref().is_none_or(|u| !u.starts_with("http")) => Err(format!("{}: {} needs an http(s) URL", self.name, self.kind)),
            "stdio" | "http" | "sse" => Ok(()),
            k => Err(format!("{}: type {k:?}: stdio, http or sse", self.name)),
        }
    }

    /// The command line or URL, for a list row.
    pub fn target(&self) -> String {
        match self.kind.as_str() {
            "stdio" => std::iter::once(self.command.clone().unwrap_or_default()).chain(self.args.iter().cloned()).collect::<Vec<_>>().join(" "),
            _ => self.url.clone().unwrap_or_default(),
        }
    }

    /// Env and header values in plain text (a hand-edited file).
    pub fn plain_secrets(&self) -> Vec<String> {
        self.env.iter().chain(&self.headers).filter(|(_, v)| !is_ref(v)).map(|(k, _)| k.clone()).collect()
    }
}

// ---------- secrets ----------

/// V is a secret reference: `keychain:<account>` or `${VAR}`.
pub fn is_ref(v: &str) -> bool {
    v.starts_with("keychain:") || env_ref(v).is_some()
}

fn env_ref(v: &str) -> Option<&str> {
    v.strip_prefix("${")?.strip_suffix('}').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// SECRET into the OS keychain as `<server>/<key>`: its reference, or Err(why) (0 keychain:
/// the caller offers a `${VAR}` reference).
pub fn store_secret(server: &str, key: &str, secret: &str) -> Result<String, String> {
    let account = format!("{server}/{key}");
    let e = keyring::Entry::new(SERVICE, &account).map_err(|e| format!("keychain: {e}"))?;
    e.set_password(secret).map_err(|e| format!("keychain: {e}"))?;
    Ok(format!("keychain:{account}"))
}

/// The keychain entry of reference V, if it is one, gone.
pub fn forget_secret(v: &str) {
    if let Some(account) = v.strip_prefix("keychain:")
        && let Ok(e) = keyring::Entry::new(SERVICE, account)
    {
        let _ = e.delete_credential();
    }
}

/// The value of V: a keychain entry, an env var, else V itself.
pub fn resolve(v: &str) -> Result<String, String> {
    if let Some(account) = v.strip_prefix("keychain:") {
        let e = keyring::Entry::new(SERVICE, account).map_err(|e| format!("keychain {account}: {e}"))?;
        return e.get_password().map_err(|e| format!("keychain {account}: {e}"));
    }
    if let Some(name) = env_ref(v) {
        return std::env::var(name).map_err(|_| format!("${{{name}}}: not set in codehead's environment"));
    }
    Ok(v.to_string())
}

// ---------- sessions ----------

/// The servers of agent A: those its profile lists, in the order of the file.
pub fn for_agent(a: &Agent) -> Vec<Server> {
    if a.mcp.is_empty() {
        return Vec::new();
    }
    all().into_iter().filter(|s| a.mcp.contains(&s.name)).collect()
}

/// A short stamp of what agent A's servers are: a session with another stamp runs an old set.
pub fn stamp(a: &Agent) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for s in for_agent(a) {
        toml::to_string(&s).unwrap_or_default().hash(&mut h);
        s.name.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// Claude Code: the `mcpServers` entries of SERVERS and the env the harness process gets. Secrets
/// stay out of the config (it is a process argument): `${CODEHEAD_MCP_<n>}`, Claude Code
/// expands them from its env. A server with a secret that does not resolve: left out, its error.
pub fn claude_config(servers: &[Server]) -> (serde_json::Map<String, Value>, Vec<(String, String)>, Vec<(String, String)>) {
    let mut entries = serde_json::Map::new();
    let mut env = Vec::new();
    let mut errors = Vec::new();
    'servers: for s in servers {
        let mut secret = |v: &str| -> Result<String, String> {
            let value = resolve(v)?;
            let var = format!("CODEHEAD_MCP_{}", env.len());
            env.push((var.clone(), value));
            Ok(format!("${{{var}}}"))
        };
        let mut map = |m: &BTreeMap<String, String>| -> Result<serde_json::Map<String, Value>, String> {
            m.iter().map(|(k, v)| Ok((k.clone(), Value::String(secret(v)?)))).collect()
        };
        let e = match s.kind.as_str() {
            "stdio" => map(&s.env).map(|env| json!({"type": "stdio", "command": s.command, "args": s.args, "env": env})),
            k => map(&s.headers).map(|h| json!({"type": k, "url": s.url, "headers": h})),
        };
        match e {
            Ok(v) => {
                entries.insert(s.name.clone(), v);
            }
            Err(why) => {
                errors.push((s.name.clone(), why));
                continue 'servers;
            }
        }
    }
    (entries, env, errors)
}

/// ACP: the `mcpServers` entries of SERVERS for an agent with HTTP / SSE support (`mcpCapabilities`),
/// with resolved values (they go over the pipe, not the command line). Left out: servers the
/// agent cannot run, servers with a secret that does not resolve; both with their error.
pub fn acp_config(servers: &[Server], http: bool, sse: bool) -> (Vec<Value>, Vec<(String, String)>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for s in servers {
        let pairs = |m: &BTreeMap<String, String>| -> Result<Vec<Value>, String> { m.iter().map(|(k, v)| Ok(json!({"name": k, "value": resolve(v)?}))).collect() };
        let entry = match s.kind.as_str() {
            "stdio" => pairs(&s.env).map(|env| json!({"name": s.name, "command": s.command, "args": s.args, "env": env})),
            "http" if !http => Err("harness: 0 HTTP MCP support (ACP mcpCapabilities.http)".into()),
            "sse" if !sse => Err("harness: 0 SSE MCP support (ACP mcpCapabilities.sse)".into()),
            k => pairs(&s.headers).map(|h| json!({"type": k, "name": s.name, "url": s.url, "headers": h})),
        };
        match entry {
            Ok(v) => out.push(v),
            Err(e) => errors.push((s.name.clone(), e)),
        }
    }
    (out, errors)
}

/// An ACP tool call TOOL (its title) of an auto-allowed server among SERVERS.
pub fn acp_auto_allowed(tool: &str, servers: &[Server]) -> bool {
    let t = tool.to_ascii_lowercase();
    servers.iter().filter(|s| s.auto_allow).any(|s| {
        let n = s.name.to_ascii_lowercase();
        ["__", "_", ".", "/", ":", " "].iter().any(|sep| t.starts_with(&format!("{n}{sep}")) || t.contains(&format!("mcp__{n}__")) || t.contains(&format!(" {n}{sep}")))
    })
}

// ---------- status ----------

/// What a harness reported of 1 server: `<data>/mcp-status.json`, `{harness: {server: Status}}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
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

pub fn statuses() -> BTreeMap<String, BTreeMap<String, Status>> {
    std::fs::read(status_path()).ok().and_then(|t| serde_json::from_slice(&t).ok()).unwrap_or_default()
}

/// HARNESS reported ROWS (server, status): merged into the file, under its lock (hosts of other
/// threads write it too). A row without a tool count keeps the last count.
pub fn record(harness: &str, rows: Vec<(String, Status)>) {
    if rows.is_empty() {
        return;
    }
    let dir = super::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = super::locked(&dir.join("mcp-status.lock"), || {
        let mut all = statuses();
        let h = all.entry(harness.to_string()).or_default();
        for (name, mut s) in rows {
            if s.tools.is_none() {
                s.tools = h.get(&name).and_then(|o| o.tools);
            }
            h.insert(name, s);
        }
        super::write_atomic(&status_path(), serde_json::to_string(&all).unwrap_or_default().as_bytes())
    });
}

/// Claude Code's `system`/`init` event V: (server, status) per entry of `mcp_servers`, the tool
/// count from `tools` (`mcp__<server>__<tool>`). Claude Code sanitizes server names in tool
/// names (`claude.ai Linear` → `claude_ai_Linear`).
pub fn from_init(v: &Value) -> Vec<(String, Status)> {
    let tools: Vec<&str> = v["tools"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let at = super::now_ms();
    v["mcp_servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let name = s["name"].as_str()?.to_string();
            // codehead's own server; `pending`: still connecting at init, 0 news
            if name == "codehead" || s["status"] == "pending" {
                return None;
            }
            let key: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let prefix = format!("mcp__{key}__");
            let n = tools.iter().filter(|t| t.starts_with(&prefix)).count();
            let state = match s["status"].as_str().unwrap_or_default() {
                "connected" => "connected",
                "needs-auth" | "needs_auth" => "needs-auth",
                "failed" => "failed",
                "disabled" => "off",
                _ => "unknown",
            };
            let source = s["source"].as_str().unwrap_or_default().to_string();
            Some((name, Status { state: state.into(), tools: (state == "connected").then_some(n), error: String::new(), source, at }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, kind: &str) -> Server {
        Server { name: name.into(), kind: kind.into(), auto_allow: true, ..Default::default() }
    }

    #[test]
    fn check_names_and_kinds() {
        assert!(valid_name("linear-2_x"));
        assert!(!valid_name("2x"));
        assert!(!valid_name("a b"));
        let mut s = server("files", "stdio");
        assert!(s.check().is_err());
        s.command = Some("npx".into());
        assert!(s.check().is_ok());
        let mut h = server("lin", "http");
        h.url = Some("ftp://x".into());
        assert!(h.check().is_err());
        h.url = Some("https://x/mcp".into());
        assert!(h.check().is_ok());
        assert!(server("x", "ws").check().is_err());
    }

    #[test]
    fn toml_round_trip_with_defaults() {
        let text = "[servers.lin]\ntype = \"http\"\nurl = \"https://x/mcp\"\nheaders = { Authorization = \"${LIN}\" }\n\n[servers.files]\ncommand = \"npx\"\nauto_allow = false\n";
        let f: FileRec = toml::from_str(text).unwrap();
        let lin = &f.servers["lin"];
        assert_eq!((lin.kind.as_str(), lin.auto_allow), ("http", true));
        let files = &f.servers["files"];
        assert_eq!((files.kind.as_str(), files.auto_allow), ("stdio", false));
        let back = toml::to_string_pretty(&f).unwrap();
        assert!(back.contains("type = \"http\"") && !back.contains("name"), "{back}");
    }

    #[test]
    fn references_resolve_and_plain_values_show() {
        assert!(is_ref("${A_1}") && is_ref("keychain:x/y") && !is_ref("abc") && !is_ref("${}") && !is_ref("${a-b}"));
        // SAFETY: tests of this module set only this var.
        unsafe { std::env::set_var("CODEHEAD_TEST_SECRET_9", "s3") };
        assert_eq!(resolve("${CODEHEAD_TEST_SECRET_9}").unwrap(), "s3");
        assert!(resolve("${CODEHEAD_TEST_UNSET_9}").is_err());
        assert_eq!(resolve("plain").unwrap(), "plain");
        let mut s = server("x", "stdio");
        s.env.insert("A".into(), "${X}".into());
        s.env.insert("B".into(), "plain".into());
        assert_eq!(s.plain_secrets(), ["B"]);
    }

    #[test]
    fn claude_config_keeps_secrets_out_of_the_argument() {
        unsafe { std::env::set_var("CODEHEAD_TEST_SECRET_8", "tok") };
        let mut h = server("lin", "http");
        h.url = Some("https://x/mcp".into());
        h.headers.insert("Authorization".into(), "${CODEHEAD_TEST_SECRET_8}".into());
        let mut bad = server("bad", "stdio");
        bad.command = Some("x".into());
        bad.env.insert("K".into(), "${CODEHEAD_TEST_UNSET_8}".into());
        let (entries, env, errors) = claude_config(&[h, bad]);
        assert_eq!(entries["lin"]["headers"]["Authorization"], "${CODEHEAD_MCP_0}");
        assert!(!Value::Object(entries.clone()).to_string().contains("tok"));
        assert_eq!(env, [("CODEHEAD_MCP_0".to_string(), "tok".to_string())]);
        assert!(!entries.contains_key("bad"));
        assert_eq!(errors[0].0, "bad");
    }

    #[test]
    fn acp_config_skips_transports_the_agent_lacks() {
        let mut h = server("lin", "http");
        h.url = Some("https://x/mcp".into());
        let mut f = server("files", "stdio");
        f.command = Some("npx".into());
        let (out, errors) = acp_config(&[h.clone(), f.clone()], false, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["name"], "files");
        assert!(errors[0].1.contains("HTTP"), "{errors:?}");
        let (out, errors) = acp_config(&[h, f], true, false);
        assert_eq!((out.len(), errors.len()), (2, 0));
        assert_eq!(out[0]["type"], "http");
    }

    /// The OS keychain for real: `cargo test keychain -- --ignored` (writes 1 entry, deletes it).
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        let r = store_secret("codehead-test", "TOKEN", "s3cret").expect("keychain");
        assert_eq!(r, "keychain:codehead-test/TOKEN");
        assert_eq!(resolve(&r).unwrap(), "s3cret");
        forget_secret(&r);
        assert!(resolve(&r).is_err());
    }

    #[test]
    fn acp_tool_titles_match_their_server() {
        let mut off = server("slack", "http");
        off.auto_allow = false;
        let s = [server("linear", "http"), off];
        assert!(acp_auto_allowed("linear__save_issue", &s));
        assert!(acp_auto_allowed("mcp__linear__get_issue", &s));
        assert!(acp_auto_allowed("Linear/get_issue", &s));
        assert!(!acp_auto_allowed("linearize", &s));
        assert!(!acp_auto_allowed("slack__send", &s));
    }

    #[test]
    fn init_events_give_states_and_tool_counts() {
        // the shape Claude Code 2.x sends
        let v = json!({"mcp_servers": [{"name": "claude.ai Linear", "status": "connected", "source": "claudeai"}, {"name": "plugin:general:figma", "status": "needs-auth", "source": "plugin"},
            {"name": "codehead", "status": "connected"}, {"name": "slow", "status": "pending"}, {"name": "plugin:x:y", "status": "connected", "source": "plugin"}],
            "tools": ["Bash", "mcp__claude_ai_Linear__get_issue", "mcp__claude_ai_Linear__save_issue", "mcp__codehead__send", "mcp__plugin_x_y__a"]});
        let r = from_init(&v);
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["claude.ai Linear", "plugin:general:figma", "plugin:x:y"]);
        assert_eq!((r[0].1.state.as_str(), r[0].1.tools, r[0].1.source.as_str()), ("connected", Some(2), "claudeai"));
        assert_eq!((r[1].1.state.as_str(), r[1].1.tools), ("needs-auth", None));
        assert_eq!(r[2].1.tools, Some(1));
    }
}
