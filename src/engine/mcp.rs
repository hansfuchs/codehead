//! `codehead mcp`: the MCP server (stdio, JSON-RPC 2.0) every agent session gets. Its tools
//! message the thread's host. $`CODEHEAD_THREAD`, $`CODEHEAD_AGENT`: the thread, the agent.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use super::{client, roster};

fn tools() -> Value {
    serde_json::from_str(include_str!("mcp_tools.json")).expect("valid MCP tool definitions")
}

fn call(thread: &str, agent: &str, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "send" => send_message(thread, agent, args),
        "set_status" => set_agent_status(thread, agent, args),
        "stash_name" => crate::domain::changes::stash::set_name(
            thread,
            args["id"].as_str().unwrap_or_default(),
            args["name"].as_str().unwrap_or_default(),
        ),
        "stash_restored" => crate::domain::changes::stash::restored(thread, args["id"].as_str().unwrap_or_default()),
        "comment" => add_review_comments(thread, agent, args),
        "comments" => Ok(list_review_comments(thread, args)),
        "comment_status" => set_review_comment_status(thread, agent, args),
        "comment_reply" => reply_to_review_comment(thread, agent, args),
        "flow_step" => advance_flow(thread, agent, args),
        "flow_suggest" => suggest_flow(thread, agent, args),
        "set_title" => crate::domain::projects::set_title(
            thread,
            args["title"].as_str().unwrap_or_default(),
            args["branch"].as_str(),
        ),
        "set_projects" => set_thread_projects(thread, args),
        "list_agents" => Ok(list_thread_agents(thread, agent)),
        _ => Err(format!("unknown tool {name}")),
    }
}

/// The `flow_step` reply for the agent: the next step's text, a handoff, a wait, the end.
fn flow_reply(result: &Value) -> String {
    if let Some(next) = result["text"].as_str() {
        return format!("Step done. Your next step:\n\n{next}");
    }

    if let Some(handed) = result["handed"].as_str() {
        return format!(
            "Step done. The next step went to @{handed}. End your turn; codehead brings the flow back to you."
        );
    }

    match result["wait"].as_str() {
        Some("owner") => "Step done. The flow waits for the owner's decision. End your turn and wait.".into(),
        Some("role") => "Step done. The next step's role has 0 agent: the owner picks one. End your turn.".into(),
        _ if result["finished"].as_bool() == Some(true) => "Step done. The flow finished.".into(),
        _ => "Step done.".into(),
    }
}

fn handle(thread: &str, agent: &str, req: &Value) -> Option<Value> {
    let id = req.get("id")?.clone();

    let result = match req["method"].as_str().unwrap_or_default() {
        "initialize" => json!({
            "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "codehead", "version": crate::infra::update::VERSION},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let params = &req["params"];

            match call(
                thread,
                agent,
                params["name"].as_str().unwrap_or_default(),
                &params["arguments"],
            ) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                Err(error) => json!({"content": [{"type": "text", "text": error}], "isError": true}),
            }
        }
        method => {
            return Some(
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method {method} not found")}}),
            );
        }
    };

    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub(crate) fn run() -> ! {
    let thread = std::env::var("CODEHEAD_THREAD").unwrap_or_default();
    let agent = std::env::var("CODEHEAD_AGENT").unwrap_or_default();
    let mut out = std::io::stdout().lock();

    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // a notification (0 id) gets 0 reply
        if let Some(result) = handle(&thread, &agent, &req)
            && writeln!(out, "{result}").and_then(|()| out.flush()).is_err()
        {
            break;
        }
    }

    std::process::exit(0)
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;

fn send_message(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    let reply = client::send(
        thread,
        &json!({"op": "send", "from": agent, "to": args["to"], "text": args["message"]}),
    )?;

    Ok(format!(
        "sent to {}",
        reply["to"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
fn set_agent_status(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    client::send(
        thread,
        &json!({"op": "status", "agent": agent, "state": args["state"], "note": args["note"]}),
    )?;
    Ok("status set".into())
}
fn add_review_comments(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    let root = super::store::load(thread).map(|loaded| loaded.work_dir());
    let ids = crate::domain::changes::agent_comments(thread, agent, root.as_deref(), &args["comments"])?;

    Ok(format!(
        "{} comments added to the changes view (open): {}",
        ids.len(),
        ids.join(", ")
    ))
}
fn list_review_comments(thread: &str, args: &Value) -> std::string::String {
    // comments on files with 0 uncommitted changes: resolved first
    if let Some(loaded) = super::store::load(thread) {
        crate::domain::changes::settle_comments(thread, &loaded.work_dir());
    }

    crate::domain::changes::list_comments(thread, args["state"].as_str().unwrap_or("open"))
}
fn set_review_comment_status(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    let ids: Vec<String> = args["ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();

    if ids.is_empty() {
        return Err("ids: 1 comment id or more (the `comments` tool lists them)".into());
    }

    crate::domain::changes::set_comment_state(
        thread,
        agent,
        &ids,
        args["state"].as_str().unwrap_or("resolved"),
        args["note"].as_str().unwrap_or_default(),
    )
}
fn reply_to_review_comment(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    let id = args["id"].as_str().unwrap_or_default();
    let text = args["text"].as_str().unwrap_or_default();
    let comment = crate::domain::changes::threads::reply(thread, agent, id, text, false)?;

    // `@name`: that agent gets the thread at once (as a message of this agent)
    let names: Vec<String> = roster::active().into_iter().map(|member| member.name).collect();
    let to = crate::domain::changes::threads::mentioned(text, &names, agent);

    if to.is_empty() {
        return Ok(format!("reply added to {id}"));
    }

    let message = format!(
        "on comment {id} `{}`: {}\n\n{}",
        comment.path,
        text.trim(),
        crate::domain::changes::threads::context(&comment)
    );

    client::send(thread, &json!({"op": "send", "from": agent, "to": to, "text": message}))?;

    Ok(format!("reply added to {id}, sent to {}", to.join(", ")))
}
fn advance_flow(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    let reply = client::send(
        thread,
        &json!({"op": "flow_step", "agent": agent, "again": args["again"], "pass_gate": args["pass_gate"], "note": args["note"]}),
    )?;

    Ok(flow_reply(&reply))
}
fn suggest_flow(thread: &str, agent: &str, args: &Value) -> Result<String, String> {
    client::send(
        thread,
        &json!({"op": "flow_suggest", "agent": agent, "flow": args["flow"], "reason": args["reason"]}),
    )?;
    Ok("suggestion shown to the owner. Go on with the current step until the owner switches.".into())
}
fn set_thread_projects(thread: &str, args: &Value) -> Result<String, String> {
    let projects: Vec<String> = args["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();

    crate::domain::projects::set_projects(thread, &projects)
}
fn list_thread_agents(thread: &str, agent: &str) -> std::string::String {
    let on = super::store::load(thread)
        .map(|loaded| loaded.agents)
        .unwrap_or_default();
    let rows: Vec<Value> = roster::active()
                .iter()
                .map(|member| json!({"name": member.name, "harness": super::harness::harness_label(member), "mode": member.mode, "on_thread": on.contains(&member.name), "you": member.name == agent}))
                .collect();

    serde_json::to_string_pretty(&rows).unwrap_or_default()
}
