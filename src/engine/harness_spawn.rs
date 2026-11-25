use super::{
    BufRead, BufReader, ClaudeStream, Command, Out, Parser, Sender, Session, Spec, Stdio, Value, json,
    report_mcp_errors,
};

/// JSON lines of R → F until R ends or F says stop (false). Bytes, not text: a line with
/// invalid UTF-8 is skipped, the reader goes on.
pub(crate) fn json_lines(result: impl std::io::Read, mut file: impl FnMut(Value) -> bool) {
    let mut result_value = BufReader::new(result);
    let mut buf = Vec::new();

    loop {
        buf.clear();

        match result_value.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }

        let Ok(value) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };

        if !file(value) {
            return;
        }
    }
}
/// Spawn options of a harness process: its own process group (a kill reaches its children).
pub(crate) fn harness_command(bin: &str, text: &Spec) -> Command {
    use std::os::unix::process::CommandExt;
    let mut character = Command::new(bin);

    character
        .current_dir(text.dir)
        .env("CODEHEAD_THREAD", text.thread)
        .env("CODEHEAD_AGENT", &text.agent.name)
        // `git` through codehead's shim: 1 push per checkout at a time (crate::infra::pushlock)
        .env("PATH", crate::infra::pushlock::path_with_shim())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    if let Some(height) = crate::infra::config::home_repo() {
        character.env("CODEHEAD_HOME_REPO", height);
    }

    if let Some(thread_value) = crate::engine::store::load(text.thread) {
        if let Some(path) = thread_value.port {
            character.env("CODEHEAD_PORT", path.to_string());
        }

        // portless: the worktree's label, so its URLs stay fixed
        if crate::infra::portless::active()
            && thread_value.worktree.is_some()
            && let Some(bytes) = &thread_value.branch
        {
            character.env("PORTLESS_PREFIX", crate::infra::portless::prefix(bytes));
        }
    }

    character
}
/// Claude Code's arguments. SERVERS: the agent's codehead MCP servers (`servers::claude_config`),
/// ALLOW: their tool prefixes that run without an ask.
pub(crate) fn args(text: &Spec, servers: serde_json::Map<String, Value>, allow: &[String]) -> Vec<String> {
    let mut all = serde_json::Map::new();

    all.insert(
        "codehead".into(),
        json!({
            "type": "stdio",
            "command": text.exe.display().to_string(),
            "args": ["mcp"],
            "env": {"CODEHEAD_THREAD": text.thread, "CODEHEAD_AGENT": text.agent.name},
        }),
    );
    all.extend(servers.into_iter().filter(|(key, _)| key != "codehead"));
    let mcp = json!({ "mcpServers": all });
    let agent_value: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // token chunks (`stream_event`): live text before each full message
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        crate::engine::roster::permission_mode(&text.agent.mode),
        "--name",
        &text.agent.name,
        "--append-system-prompt",
        text.system,
        "--mcp-config",
        &mcp.to_string(),
        // codehead's own tools never wait on the owner
        "--allowedTools",
        "mcp__codehead",
    ]
    .map(String::from)
    .to_vec();

    append_session_arguments(text, allow, agent_value)
}

fn append_session_arguments(text: &Spec<'_>, allow: &[String], mut agent_value: Vec<String>) -> Vec<String> {
    agent_value.extend(allow.iter().cloned());

    // the agent's own model / effort, else the harness tab's
    let own = |value: &Option<String>| {
        value
            .clone()
            .filter(|value_value| !value_value.is_empty() && value_value != "default")
    };

    if let Some(message) = own(&text.agent.model).or_else(|| crate::infra::config::harness_opt("claude", "model")) {
        agent_value.extend(["--model".into(), message]);
    }

    if let Some(error) = own(&text.agent.effort).or_else(|| crate::infra::config::harness_opt("claude", "effort")) {
        agent_value.extend(["--effort".into(), error]);
    }

    if let Some(extra) = crate::infra::config::harness_opt("claude", "args") {
        agent_value.extend(crate::engine::harnesses::split(&extra));
    }

    for directory in text.extra {
        agent_value.extend(["--add-dir".into(), directory.display().to_string()]);
    }

    if let Some(result) = text.resume {
        agent_value.extend(["--resume".into(), result.into()]);

        if text.fork {
            agent_value.push("--fork-session".into());
        }
    }

    agent_value
}
pub(crate) fn stderr_tail(stderr: std::process::ChildStderr) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
    let tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let thread_value = tail.clone();

    std::thread::spawn(move || {
        // bytes: a non-UTF-8 line must not end the reader (a full pipe blocks the harness)
        let mut result = BufReader::new(stderr);
        let mut buf = Vec::new();

        while matches!(result.read_until(b'\n', &mut buf), Ok(count) if count > 0) {
            let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();

            buf.clear();

            append_stderr_line(&thread_value, line);
        }
    });

    tail
}

fn append_stderr_line(thread_value: &std::sync::Arc<std::sync::Mutex<Vec<String>>>, line: String) {
    if let Ok(mut thread_2) = thread_value.lock() {
        thread_2.push(line);
        let count = thread_2.len();

        if count > 20 {
            thread_2.drain(..count - 20);
        }
    }
}
/// Start the session; its events go to TX, tagged with the agent's name.
pub(crate) fn spawn(text: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let height = crate::engine::harnesses::get(&text.agent.harness)
        .ok_or_else(|| format!("unknown harness {:?}", text.agent.harness))?;

    match (
        crate::engine::harnesses::acp_command(height, text.agent.command.as_deref()),
        &height.drive,
    ) {
        (Some(cmd), _) => spawn_acp(text, height, &cmd, tx),
        (None, crate::engine::harnesses::Drive::Codex) => spawn_codex(text, height, tx),
        (None, _) => spawn_claude(text, tx),
    }
}
/// The agent's own value of KEY (model, effort), else the harness tab's.
pub(crate) fn own_or_harness(own: Option<&str>, harness: &str, key: &str) -> Option<String> {
    own.filter(|value| !value.is_empty() && *value != "default")
        .map(str::to_owned)
        .or_else(|| crate::infra::config::harness_opt(harness, key))
}
pub(crate) fn spawn_codex(
    text: &Spec,
    height: &crate::engine::harnesses::Harness,
    tx: Sender<(String, Out)>,
) -> Result<Session, String> {
    let mut child = harness_command("codex", text)
        .arg("app-server")
        .spawn()
        .map_err(|error| {
            format!(
                "{} (codex app-server): {error}. install: {}",
                height.label, height.install
            )
        })?;

    let stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let tail = stderr_tail(child.stderr.take().ok_or("0 stderr")?);

    // the app server runs every MCP transport
    let (servers, errors) =
        crate::engine::servers::acp_config(&crate::engine::servers::for_agent(text.agent), true, true);

    report_mcp_errors(height.name, &errors, &tx, &text.agent.name);

    let env = [
        ("CODEHEAD_THREAD".to_string(), text.thread.to_string()),
        ("CODEHEAD_AGENT".to_string(), text.agent.name.clone()),
    ];
    let setup = crate::engine::codex::Setup {
        cwd: text.dir.display().to_string(),
        resume: text.resume.map(String::from),
        fork: text.fork,
        model: own_or_harness(text.agent.model.as_deref(), height.name, "model"),
        effort: own_or_harness(text.agent.effort.as_deref(), height.name, "effort"),
        instructions: text.system.to_string(),
        config: json!({"mcp_servers": crate::engine::codex::mcp_config(&text.exe.display().to_string(), &env, &servers)}),
        sandbox: if text.agent.mode == "plan" {
            "read-only"
        } else {
            "danger-full-access"
        },
    };

    let conn = crate::engine::codex::start(stdin, stdout, tail, setup, text.agent.name.clone(), tx);

    Ok(Session {
        child,
        driver: Box::new(conn),
    })
}
pub(crate) fn spawn_acp(
    text: &Spec,
    height: &crate::engine::harnesses::Harness,
    cmd: &[String],
    tx: Sender<(String, Out)>,
) -> Result<Session, String> {
    let bin = cmd.first().ok_or("empty ACP command")?;

    let mut child = harness_command(bin, text).args(&cmd[1..]).spawn().map_err(|error| {
        format!(
            "{} ({}): {error}. install: {}",
            height.label,
            cmd.join(" "),
            height.install
        )
    })?;

    let stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let tail = stderr_tail(child.stderr.take().ok_or("0 stderr")?);
    let setup = crate::engine::acp::Setup {
        cwd: text.dir.display().to_string(),
        resume: text.resume.filter(|_| !text.fork).map(String::from),
        model: text
            .agent
            .model
            .clone()
            .filter(|message| !message.is_empty() && message != "default")
            .or_else(|| crate::infra::config::harness_opt(height.name, "model")),
        mcp: (
            text.exe.display().to_string(),
            vec!["mcp".into()],
            vec![
                ("CODEHEAD_THREAD".into(), text.thread.into()),
                ("CODEHEAD_AGENT".into(), text.agent.name.clone()),
            ],
        ),
        preamble: text.system.to_string(),
        harness: height.name.to_string(),
        servers: crate::engine::servers::for_agent(text.agent),
    };
    let conn = crate::engine::acp::start(stdin, stdout, tail, setup, text.agent.name.clone(), tx);

    Ok(Session {
        child,
        driver: Box::new(conn),
    })
}
pub(crate) fn spawn_claude(text: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let servers = crate::engine::servers::for_agent(text.agent);
    let allow: Vec<String> = servers
        .iter()
        .filter(|value| value.auto_allow)
        .map(|value_value| format!("mcp__{}", value_value.name))
        .collect();
    let (entries, env, errors) = crate::engine::servers::claude_config(&servers);

    report_mcp_errors("claude", &errors, &tx, &text.agent.name);
    let mut child = harness_command("claude", text)
        .args(args(text, entries, &allow))
        // secrets of the MCP servers: `${CODEHEAD_MCP_<n>}` in the config, the values here
        .envs(env)
        // Claude Code leaves the Artifact tools out of SDK sessions without this
        .env(
            "CLAUDE_CODE_ARTIFACT",
            std::env::var("CLAUDE_CODE_ARTIFACT").unwrap_or_else(|_| "1".into()),
        )
        .spawn()
        .map_err(|error| format!("claude: {error}"))?;
    let stdin = child.stdin.take().ok_or("claude: 0 stdin")?;
    let stdout = child.stdout.take().ok_or("claude: 0 stdout")?;
    let err_tail = stderr_tail(child.stderr.take().ok_or("claude: 0 stderr")?);
    let name = text.agent.name.clone();

    std::thread::spawn(move || {
        let mut path = Parser::default();
        let mut open = true;

        json_lines(stdout, |value_2| {
            open = path
                .line(&value_2)
                .into_iter()
                .all(|output| tx.send((name.clone(), output)).is_ok());
            open
        });

        if open {
            let err = err_tail
                .lock()
                .map(|thread_value| thread_value.join("\n"))
                .unwrap_or_default();
            let _ = tx.send((name, Out::Exit(err)));
        }
    });

    Ok(Session {
        child,
        driver: Box::new(ClaudeStream(stdin)),
    })
}

pub(crate) fn json_writer(mut stdin: std::process::ChildStdin) -> Sender<String> {
    use std::io::Write as _;
    let (sender, receiver) = std::sync::mpsc::channel::<String>();

    std::thread::spawn(move || {
        let _ = receiver.into_iter().try_for_each(|mut line| {
            line.push('\n');
            stdin.write_all(line.as_bytes()).and_then(|()| stdin.flush())
        });
    });

    sender
}
