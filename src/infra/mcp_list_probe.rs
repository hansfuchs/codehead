use super::{
    BufRead, BufReader, Command, Duration, INIT, INITIALIZED, Instant, LIST, Server, Status, Stdio, Value, Write,
    servers,
};
use std::fmt::Write as FormatWrite;

/// Server S, asked for its tools: (state, tool count, error).
pub(crate) fn probe(text: &Server) -> Status {
    let at = crate::engine::now_ms();

    let result = match text.kind.as_str() {
        "stdio" => probe_stdio(text),
        "http" => probe_http(text),
        _ => Err("probe: SSE servers are probed in a session only".into()),
    };

    match result {
        Ok(count) => Status {
            state: "connected".into(),
            tools: Some(count),
            source: "codehead".into(),
            at,
            ..Default::default()
        },
        Err(error_value) if error_value.contains("401") || error_value.to_lowercase().contains("unauthorized") => {
            Status {
                state: "needs-auth".into(),
                error: "needs OAuth sign-in: codehead cannot sign in for a harness".into(),
                source: "codehead".into(),
                at,
                ..Default::default()
            }
        }
        Err(error_2) => Status {
            state: "failed".into(),
            error: error_2,
            source: "codehead".into(),
            at,
            ..Default::default()
        },
    }
}

pub(super) fn probe_stdio(text: &Server) -> Result<usize, String> {
    let cmd = text.command.clone().unwrap_or_default();
    let mut character = Command::new(&cmd);

    character
        .args(&text.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    for (key, value) in &text.env {
        character.env(key, servers::resolve(value)?);
    }

    let mut child = character.spawn().map_err(|error| format!("{cmd}: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let (tx, rx) = std::sync::mpsc::channel::<Value>();

    std::thread::spawn(move || {
        read_responses(stdout, &tx);
    });

    let deadline = Instant::now() + Duration::from_secs(20);

    let wait = |id| wait_reply(&rx, deadline, id);

    let result_value = (|| {
        writeln!(stdin, "{INIT}").map_err(|error_2| error_2.to_string())?;
        wait(1)?;
        writeln!(stdin, "{INITIALIZED}\n{LIST}").map_err(|error_3| error_3.to_string())?;
        let result = wait(2)?;

        Ok(result["tools"].as_array().map_or(0, Vec::len))
    })();
    let _ = child.kill();
    let _ = child.wait();

    result_value
}

/// 1 POST to the server (curl; its config, headers included, on stdin: 0 secret in argv).
pub(super) fn post(text_value: &Server, body: &str, session: Option<&str>) -> Result<(String, String), String> {
    let cfg = curl_config(text_value, body, session)?;
    let mut child = Command::new("curl")
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("curl: {error}"))?;

    child
        .stdin
        .take()
        .ok_or("0 stdin")?
        .write_all(cfg.as_bytes())
        .map_err(|error_value| error_value.to_string())?;
    let out = child.wait_with_output().map_err(|error_2| error_2.to_string())?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or(("", &text));
    let status = head.lines().next().unwrap_or_default();

    if let Some(code) = status.split_whitespace().nth(1)
        && !code.starts_with('2')
    {
        return Err(format!("HTTP {code}"));
    }

    let sid = head
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key_value, _)| key_value.trim().eq_ignore_ascii_case("mcp-session-id"))
                .map(|(_, value_value)| value_value.trim().to_string())
        })
        .unwrap_or_default();

    Ok((sid, body.to_string()))
}

/// The JSON-RPC reply in BODY: plain JSON, or the `data:` lines of an event stream.
pub(super) fn reply_of(body: &str) -> Option<Value> {
    serde_json::from_str::<Value>(body.trim()).ok().or_else(|| {
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|directory| serde_json::from_str::<Value>(directory.trim()).ok())
            .find(|value| value.get("id").is_some())
    })
}

pub(super) fn probe_http(text: &Server) -> Result<usize, String> {
    let (sid, body) = post(text, INIT, None)?;
    let init = reply_of(&body).ok_or("initialize: 0 JSON-RPC reply")?;

    if let Some(error) = init.get("error") {
        return Err(format!("initialize: {}", error["message"].as_str().unwrap_or("error")));
    }

    let sid = (!sid.is_empty()).then_some(sid);
    let _ = post(text, INITIALIZED, sid.as_deref());
    let (_, body) = post(text, LIST, sid.as_deref())?;
    let result = reply_of(&body).ok_or("tools/list: 0 JSON-RPC reply")?;

    if let Some(error_value) = result.get("error") {
        return Err(format!(
            "tools/list: {}",
            error_value["message"].as_str().unwrap_or("error")
        ));
    }

    Ok(result["result"]["tools"].as_array().map_or(0, Vec::len))
}

/// Every codehead server probed (in parallel), the results recorded as harness `codehead`.
pub(crate) fn probe_all(list: &[Server]) -> Vec<(String, Status)> {
    let jobs: Vec<_> = list
        .iter()
        .cloned()
        .map(|text| std::thread::spawn(move || (text.name.clone(), probe(&text))))
        .collect();
    let rows: Vec<(String, Status)> = jobs.into_iter().filter_map(|j| j.join().ok()).collect();

    servers::record("codehead", rows.clone());
    rows
}

fn curl_config(text_value: &Server, body: &str, session: Option<&str>) -> Result<String, String> {
    let mut cfg = format!(
        "url = \"{}\"\nrequest = \"POST\"\nsilent\nshow-error\ninclude\nmax-time = 20\n",
        text_value.url.clone().unwrap_or_default().replace('"', "\\\"")
    );
    let mut headers = vec![
        "Content-Type: application/json".to_string(),
        "Accept: application/json, text/event-stream".to_string(),
    ];

    for (key, value) in &text_value.headers {
        headers.push(format!("{key}: {}", servers::resolve(value)?));
    }

    if let Some(id) = session {
        headers.push(format!("Mcp-Session-Id: {id}"));
    }

    for height in headers {
        let _ = writeln!(
            cfg,
            "header = \"{}\"",
            height.replace('\\', "\\\\").replace('"', "\\\"")
        );
    }

    let _ = writeln!(cfg, "data = \"{}\"", body.replace('\\', "\\\\").replace('"', "\\\""));

    Ok(cfg)
}

fn read_responses(stdout: std::process::ChildStdout, tx: &std::sync::mpsc::Sender<Value>) {
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Ok(value_value) = serde_json::from_str::<Value>(&line)
            && tx.send(value_value).is_err()
        {
            return;
        }
    }
}

fn wait_reply(rx: &std::sync::mpsc::Receiver<Value>, deadline: Instant, id: u64) -> Result<Value, String> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let value_2 = rx.recv_timeout(left).map_err(|_| "0 answer in 20s".to_string())?;

        if value_2["id"] == id {
            return value_2.get("error").map_or_else(
                || Ok(value_2["result"].clone()),
                |error_value| Err(error_value["message"].as_str().unwrap_or("error").to_string()),
            );
        }
    }
}
