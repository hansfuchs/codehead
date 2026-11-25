use super::{
    BufRead, BufReader, ClaudeStream, Command, Out, Parser, Sender, Session, Spec, Stdio, Value, json,
    report_mcp_errors,
};

/// JSON lines of READER → HANDLE until READER ends or HANDLE says stop (false). Bytes, not
/// text: a line with invalid UTF-8 is skipped, the reader goes on.
pub(crate) fn json_lines(reader: impl std::io::Read, mut handle: impl FnMut(Value) -> bool) {
    let mut buffered = BufReader::new(reader);
    let mut buf = Vec::new();

    loop {
        buf.clear();

        match buffered.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }

        let Ok(value) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };

        if !handle(value) {
            return;
        }
    }
}
/// Spawn options of a harness process: its own process group (a kill reaches its children).
pub(crate) fn harness_command(bin: &str, spec: &Spec) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(bin);

    command
        .current_dir(spec.dir)
        .env("CODEHEAD_THREAD", spec.thread)
        .env("CODEHEAD_AGENT", &spec.agent.name)
        // `git` through codehead's shim: 1 push per checkout at a time (crate::infra::pushlock)
        .env("PATH", crate::infra::pushlock::path_with_shim())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    if let Some(home) = crate::infra::config::home_repo() {
        command.env("CODEHEAD_HOME_REPO", home);
    }

    if let Some(thread) = crate::engine::store::load(spec.thread) {
        if let Some(port) = thread.port {
            command.env("CODEHEAD_PORT", port.to_string());
        }

        // portless: the worktree's label, so its URLs stay fixed
        if crate::infra::portless::active()
            && thread.worktree.is_some()
            && let Some(branch) = &thread.branch
        {
            command.env("PORTLESS_PREFIX", crate::infra::portless::prefix(branch));
        }
    }

    command
}
/// Claude Code's arguments. SERVERS: the agent's codehead MCP servers (`servers::claude_config`),
/// ALLOW: their tool prefixes that run without an ask.
pub(crate) fn args(spec: &Spec, servers: serde_json::Map<String, Value>, allow: &[String]) -> Vec<String> {
    let mut all = serde_json::Map::new();

    all.insert(
        "codehead".into(),
        json!({
            "type": "stdio",
            "command": spec.exe.display().to_string(),
            "args": ["mcp"],
            "env": {"CODEHEAD_THREAD": spec.thread, "CODEHEAD_AGENT": spec.agent.name},
        }),
    );
    all.extend(servers.into_iter().filter(|(key, _)| key != "codehead"));
    let mcp = json!({ "mcpServers": all });
    let arguments: Vec<String> = [
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
        crate::engine::roster::permission_mode(&spec.agent.mode),
        "--name",
        &spec.agent.name,
        "--append-system-prompt",
        spec.system,
        "--mcp-config",
        &mcp.to_string(),
        // codehead's own tools never wait on the owner
        "--allowedTools",
        "mcp__codehead",
    ]
    .map(String::from)
    .to_vec();

    append_session_arguments(spec, allow, arguments)
}

fn append_session_arguments(spec: &Spec<'_>, allow: &[String], mut arguments: Vec<String>) -> Vec<String> {
    arguments.extend(allow.iter().cloned());

    // the agent's own model / effort, else the harness tab's
    let own = |value: &Option<String>| value.clone().filter(|value| !value.is_empty() && value != "default");

    if let Some(model) = own(&spec.agent.model).or_else(|| crate::infra::config::harness_opt("claude", "model")) {
        arguments.extend(["--model".into(), model]);
    }

    if let Some(effort) = own(&spec.agent.effort).or_else(|| crate::infra::config::harness_opt("claude", "effort")) {
        arguments.extend(["--effort".into(), effort]);
    }

    if let Some(extra) = crate::infra::config::harness_opt("claude", "args") {
        arguments.extend(crate::engine::harnesses::split(&extra));
    }

    for dir in spec.extra {
        arguments.extend(["--add-dir".into(), dir.display().to_string()]);
    }

    if let Some(session) = spec.resume {
        arguments.extend(["--resume".into(), session.into()]);

        if spec.fork {
            arguments.push("--fork-session".into());
        }
    }

    arguments
}
pub(crate) fn stderr_tail(stderr: std::process::ChildStderr) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
    let tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let lines = tail.clone();

    std::thread::spawn(move || {
        // bytes: a non-UTF-8 line must not end the reader (a full pipe blocks the harness)
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();

        while matches!(reader.read_until(b'\n', &mut buf), Ok(count) if count > 0) {
            let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();

            buf.clear();

            append_stderr_line(&lines, line);
        }
    });

    tail
}

fn append_stderr_line(lines: &std::sync::Arc<std::sync::Mutex<Vec<String>>>, line: String) {
    if let Ok(mut lines) = lines.lock() {
        lines.push(line);
        let count = lines.len();

        if count > 20 {
            lines.drain(..count - 20);
        }
    }
}
/// Start the session; its events go to TX, tagged with the agent's name.
pub(crate) fn spawn(spec: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let harness = crate::engine::harnesses::get(&spec.agent.harness)
        .ok_or_else(|| format!("unknown harness {:?}", spec.agent.harness))?;

    match (
        crate::engine::harnesses::acp_command(harness, spec.agent.command.as_deref()),
        &harness.drive,
    ) {
        (Some(cmd), _) => spawn_acp(spec, harness, &cmd, tx),
        (None, crate::engine::harnesses::Drive::Codex) => spawn_codex(spec, harness, tx),
        (None, _) => spawn_claude(spec, tx),
    }
}
/// The agent's own value of KEY (model, effort), else the harness tab's.
pub(crate) fn own_or_harness(own: Option<&str>, harness: &str, key: &str) -> Option<String> {
    own.filter(|value| !value.is_empty() && *value != "default")
        .map(str::to_owned)
        .or_else(|| crate::infra::config::harness_opt(harness, key))
}
pub(crate) fn spawn_codex(
    spec: &Spec,
    harness: &crate::engine::harnesses::Harness,
    tx: Sender<(String, Out)>,
) -> Result<Session, String> {
    let mut child = harness_command("codex", spec)
        .arg("app-server")
        .spawn()
        .map_err(|error| {
            format!(
                "{} (codex app-server): {error}. install: {}",
                harness.label, harness.install
            )
        })?;

    let stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let tail = stderr_tail(child.stderr.take().ok_or("0 stderr")?);

    // the app server runs every MCP transport
    let (servers, errors) =
        crate::engine::servers::acp_config(&crate::engine::servers::for_agent(spec.agent), true, true);

    report_mcp_errors(harness.name, &errors, &tx, &spec.agent.name);

    let env = [
        ("CODEHEAD_THREAD".to_string(), spec.thread.to_string()),
        ("CODEHEAD_AGENT".to_string(), spec.agent.name.clone()),
    ];
    let setup = crate::engine::codex::Setup {
        cwd: spec.dir.display().to_string(),
        resume: spec.resume.map(String::from),
        fork: spec.fork,
        model: own_or_harness(spec.agent.model.as_deref(), harness.name, "model"),
        effort: own_or_harness(spec.agent.effort.as_deref(), harness.name, "effort"),
        instructions: spec.system.to_string(),
        config: json!({"mcp_servers": crate::engine::codex::mcp_config(&spec.exe.display().to_string(), &env, &servers)}),
        sandbox: if spec.agent.mode == "plan" {
            "read-only"
        } else {
            "danger-full-access"
        },
    };

    let conn = crate::engine::codex::start(stdin, stdout, tail, setup, spec.agent.name.clone(), tx);

    Ok(Session {
        child,
        driver: Box::new(conn),
    })
}
pub(crate) fn spawn_acp(
    spec: &Spec,
    harness: &crate::engine::harnesses::Harness,
    cmd: &[String],
    tx: Sender<(String, Out)>,
) -> Result<Session, String> {
    let bin = cmd.first().ok_or("empty ACP command")?;

    let mut child = harness_command(bin, spec).args(&cmd[1..]).spawn().map_err(|error| {
        format!(
            "{} ({}): {error}. install: {}",
            harness.label,
            cmd.join(" "),
            harness.install
        )
    })?;

    let stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let tail = stderr_tail(child.stderr.take().ok_or("0 stderr")?);
    let setup = crate::engine::acp::Setup {
        cwd: spec.dir.display().to_string(),
        resume: spec.resume.filter(|_| !spec.fork).map(String::from),
        model: spec
            .agent
            .model
            .clone()
            .filter(|model| !model.is_empty() && model != "default")
            .or_else(|| crate::infra::config::harness_opt(harness.name, "model")),
        mcp: (
            spec.exe.display().to_string(),
            vec!["mcp".into()],
            vec![
                ("CODEHEAD_THREAD".into(), spec.thread.into()),
                ("CODEHEAD_AGENT".into(), spec.agent.name.clone()),
            ],
        ),
        preamble: spec.system.to_string(),
        harness: harness.name.to_string(),
        servers: crate::engine::servers::for_agent(spec.agent),
    };
    let conn = crate::engine::acp::start(stdin, stdout, tail, setup, spec.agent.name.clone(), tx);

    Ok(Session {
        child,
        driver: Box::new(conn),
    })
}
pub(crate) fn spawn_claude(spec: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let servers = crate::engine::servers::for_agent(spec.agent);
    let allow: Vec<String> = servers
        .iter()
        .filter(|server| server.auto_allow)
        .map(|server| format!("mcp__{}", server.name))
        .collect();
    let (entries, env, errors) = crate::engine::servers::claude_config(&servers);

    report_mcp_errors("claude", &errors, &tx, &spec.agent.name);
    let mut child = harness_command("claude", spec)
        .args(args(spec, entries, &allow))
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
    let name = spec.agent.name.clone();

    std::thread::spawn(move || {
        let mut parser = Parser::default();
        let mut open = true;

        json_lines(stdout, |value| {
            open = parser
                .line(&value)
                .into_iter()
                .all(|output| tx.send((name.clone(), output)).is_ok());
            open
        });

        if open {
            let stderr = err_tail.lock().map(|lines| lines.join("\n")).unwrap_or_default();
            let _ = tx.send((name, Out::Exit(stderr)));
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
