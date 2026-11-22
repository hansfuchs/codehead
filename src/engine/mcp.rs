//! `codehead mcp`: the MCP server (stdio, JSON-RPC 2.0) every agent session gets. Its tools
//! message the thread's host. $CODEHEAD_THREAD, $CODEHEAD_AGENT: the thread, the agent.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use super::{client, roster};

fn tools() -> Value {
    json!([
        {
            "name": "send",
            "description": "Message other agents on this thread. They see only what you send. An agent not on the thread joins it. The owner sees every message.",
            "inputSchema": {"type": "object", "properties": {
                "to": {"description": "Agent name, or names", "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]},
                "message": {"type": "string", "description": "The message: what you need, with file paths and exact actions"}
            }, "required": ["to", "message"]}
        },
        {
            "name": "set_status",
            "description": "Report to the owner: blocked (say on what in note) or done. Never for working: codehead shows each turn as working.",
            "inputSchema": {"type": "object", "properties": {
                "state": {"type": "string", "enum": ["blocked", "done"]},
                "note": {"type": "string"}
            }, "required": ["state"]}
        },
        {
            "name": "stash_name",
            "description": "Name an entry of the owner's stash (lines the owner took out of the worktree in the changes view): 2-6 words on what the lines do. Call it when codehead asks you to.",
            "inputSchema": {"type": "object", "properties": {
                "id": {"type": "string", "description": "The stash entry id"},
                "name": {"type": "string", "description": "2-6 words"}
            }, "required": ["id", "name"]}
        },
        {
            "name": "stash_restored",
            "description": "Report that a stash entry the owner asked you to restore is back in the worktree: the entry leaves the stash. Only after the patch applied (or the owner settled each conflict).",
            "inputSchema": {"type": "object", "properties": {
                "id": {"type": "string", "description": "The stash entry id"}
            }, "required": ["id"]}
        },
        {
            "name": "comment",
            "description": "Write review comments on lines of the changed files, in the owner's changes view (as the owner's own comments). Only when the owner asks for it (e.g. a review of the changes: 1 comment per finding). Never on your own: 0 unasked comments.",
            "inputSchema": {"type": "object", "properties": {
                "comments": {"type": "array", "items": {"type": "object", "properties": {
                    "file": {"type": "string", "description": "Path relative to the repo root of the worktree (an absolute path in it: fine)"},
                    "line": {"type": "integer", "description": "Line number in the new version of the file"},
                    "lines": {"type": "string", "description": "Optional: the lines it is on, e.g. `11-13, 15`"},
                    "severity": {"type": "string", "enum": ["critical", "high", "medium", "low"]},
                    "text": {"type": "string", "description": "The comment: what is wrong, how to fix it"}
                }, "required": ["file", "line", "severity", "text"]}}
            }, "required": ["comments"]}
        },
        {
            "name": "comments",
            "description": "List the review comments of the changes view (the owner's and the agents'): id, file, line, severity, author, state (open / resolved), text, replies, last_by (who wrote last). Read them when you address review comments, or when the owner asks you to.",
            "inputSchema": {"type": "object", "properties": {
                "state": {"type": "string", "enum": ["open", "resolved", "all"], "description": "Default: open"}
            }}
        },
        {
            "name": "comment_status",
            "description": "Resolve review comments once addressed (fixed, or answered with a reason not to fix), as on GitHub: they leave the owner's file view. A reviewer re-opens a comment whose fix does not hold. 1 note per call: what fixed it, or why not.",
            "inputSchema": {"type": "object", "properties": {
                "ids": {"type": "array", "items": {"type": "string"}, "description": "Comment ids, e.g. [\"c3\", \"c4\"]"},
                "state": {"type": "string", "enum": ["resolved", "open"]},
                "note": {"type": "string", "description": "1 line: the fix (file:line), or why not"}
            }, "required": ["ids", "state", "note"]}
        },
        {
            "name": "comment_reply",
            "description": "Reply on a review comment of the changes view (its thread): answer a question on it, argue a finding, say what you did. The owner gets a notification. `@name` in the text: that agent gets the comment's thread at once.",
            "inputSchema": {"type": "object", "properties": {
                "id": {"type": "string", "description": "The comment id, e.g. \"c3\""},
                "text": {"type": "string", "description": "The reply"}
            }, "required": ["id", "text"]}
        },
        {
            "name": "flow_step",
            "description": "Report the current step of your flow done (a step comes as a message that starts with `[codehead flow: ...]`). codehead returns your next step, or holds the flow at a gate for the owner. Call it once per step, only when the step's `Done when` holds.",
            "inputSchema": {"type": "object", "properties": {
                "again": {"type": "boolean", "description": "Loop step: its loop condition is not met yet, go back to the loop's start"},
                "pass_gate": {"type": "boolean", "description": "Gate step, agents on autopilot only: go on without the owner's approval (the situation makes the wait pointless). Say why in note"},
                "note": {"type": "string", "description": "1 line: the result of the step"}
            }}
        },
        {
            "name": "flow_suggest",
            "description": "Propose another flow to the owner when the task changed kind (e.g. a bug investigation turns into a refactor). The owner decides; go on with the current step until then.",
            "inputSchema": {"type": "object", "properties": {
                "flow": {"type": "string", "description": "Flow name or slug, e.g. `refactor`, `bug-fix`"},
                "reason": {"type": "string", "description": "1 line: why"}
            }, "required": ["flow", "reason"]}
        },
        {
            "name": "list_agents",
            "description": "Every agent: name, harness/model, mode, whether it is on this thread.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

fn call(thread: &str, agent: &str, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "send" => {
            let r = client::send(thread, &json!({"op": "send", "from": agent, "to": args["to"], "text": args["message"]}))?;
            Ok(format!("sent to {}", r["to"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")))
        }
        "set_status" => {
            client::send(thread, &json!({"op": "status", "agent": agent, "state": args["state"], "note": args["note"]}))?;
            Ok("status set".into())
        }
        "stash_name" => crate::changes::stash::set_name(thread, args["id"].as_str().unwrap_or_default(), args["name"].as_str().unwrap_or_default()),
        "stash_restored" => crate::changes::stash::restored(thread, args["id"].as_str().unwrap_or_default()),
        "comment" => {
            let root = super::store::load(thread).map(|t| t.work_dir());
            let ids = crate::changes::agent_comments(thread, agent, root.as_deref(), &args["comments"])?;
            Ok(format!("{} comments added to the changes view (open): {}", ids.len(), ids.join(", ")))
        }
        "comments" => {
            // comments on files with 0 uncommitted changes: resolved first
            if let Some(t) = super::store::load(thread) {
                crate::changes::settle_comments(thread, &t.work_dir());
            }
            Ok(crate::changes::list_comments(thread, args["state"].as_str().unwrap_or("open")))
        }
        "comment_status" => {
            let ids: Vec<String> = args["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            if ids.is_empty() {
                return Err("ids: 1 comment id or more (the `comments` tool lists them)".into());
            }
            crate::changes::set_comment_state(thread, agent, &ids, args["state"].as_str().unwrap_or("resolved"), args["note"].as_str().unwrap_or_default())
        }
        "comment_reply" => {
            let id = args["id"].as_str().unwrap_or_default();
            let text = args["text"].as_str().unwrap_or_default();
            let c = crate::changes::threads::reply(thread, agent, id, text, false)?;
            // `@name`: that agent gets the thread at once (as a message of this agent)
            let names: Vec<String> = roster::active().into_iter().map(|a| a.name).collect();
            let to = crate::changes::threads::mentioned(text, &names, agent);
            if to.is_empty() {
                return Ok(format!("reply added to {id}"));
            }

            let msg = format!("on comment {id} `{}`: {}\n\n{}", c.path, text.trim(), crate::changes::threads::context(&c));
            client::send(thread, &json!({"op": "send", "from": agent, "to": to, "text": msg}))?;
            Ok(format!("reply added to {id}, sent to {}", to.join(", ")))
        }
        "flow_step" => {
            let r = client::send(thread, &json!({"op": "flow_step", "agent": agent, "again": args["again"], "pass_gate": args["pass_gate"], "note": args["note"]}))?;
            Ok(flow_reply(&r))
        }
        "flow_suggest" => {
            client::send(thread, &json!({"op": "flow_suggest", "agent": agent, "flow": args["flow"], "reason": args["reason"]}))?;
            Ok("suggestion shown to the owner. Go on with the current step until the owner switches.".into())
        }
        "list_agents" => {
            let on = super::store::load(thread).map(|t| t.agents).unwrap_or_default();
            let rows: Vec<Value> = roster::active()
                .iter()
                .map(|a| json!({"name": a.name, "harness": super::harness::harness_label(a), "mode": a.mode, "on_thread": on.contains(&a.name), "you": a.name == agent}))
                .collect();
            Ok(serde_json::to_string_pretty(&rows).unwrap_or_default())
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

/// The `flow_step` reply for the agent: the next step's text, a handoff, a wait, the end.
fn flow_reply(r: &Value) -> String {
    if let Some(t) = r["text"].as_str() {
        return format!("Step done. Your next step:\n\n{t}");
    }
    if let Some(a) = r["handed"].as_str() {
        return format!("Step done. The next step went to @{a}. End your turn; codehead brings the flow back to you.");
    }
    match r["wait"].as_str() {
        Some("owner") => "Step done. The flow waits for the owner's decision. End your turn and wait.".into(),
        Some("role") => "Step done. The next step's role has 0 agent: the owner picks one. End your turn.".into(),
        _ if r["finished"].as_bool() == Some(true) => "Step done. The flow finished.".into(),
        _ => "Step done.".into(),
    }
}

fn handle(thread: &str, agent: &str, req: &Value) -> Option<Value> {
    let id = req.get("id")?.clone();
    let result = match req["method"].as_str().unwrap_or_default() {
        "initialize" => json!({
            "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "codehead", "version": crate::update::VERSION},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let p = &req["params"];
            match call(thread, agent, p["name"].as_str().unwrap_or_default(), &p["arguments"]) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            }
        }
        m => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method {m} not found")}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn run() -> ! {
    let thread = std::env::var("CODEHEAD_THREAD").unwrap_or_default();
    let agent = std::env::var("CODEHEAD_AGENT").unwrap_or_default();
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        // a notification (0 id) gets 0 reply
        if let Some(r) = handle(&thread, &agent, &req)
            && writeln!(out, "{r}").and_then(|_| out.flush()).is_err()
        {
            break;
        }
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_list() {
        let r = handle("t", "a", &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}})).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        let r = handle("t", "a", &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 11);
        assert!(handle("t", "a", &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
    }

    #[test]
    fn comment_reply_adds_a_reply_and_lists_it() {
        let thread = format!("mcp-reply-{}", std::process::id());
        let _ = std::fs::remove_dir_all(super::super::store::thread_dir(&thread));
        call(&thread, "REV", "comment", &json!({"comments": [{"file": "a.rs", "line": 3, "severity": "low", "text": "nit"}]})).unwrap();

        assert_eq!(call(&thread, "REV", "comment_reply", &json!({"id": "c1", "text": "fixed in a.rs:4"})).unwrap(), "reply added to c1");
        assert!(call(&thread, "REV", "comment_reply", &json!({"id": "c9", "text": "x"})).unwrap_err().contains("c9"));

        let rows: Value = serde_json::from_str(&call(&thread, "LEAD", "comments", &json!({"state": "open"})).unwrap()).unwrap();
        assert_eq!((rows[0]["replies"][0]["author"].as_str(), rows[0]["replies"][0]["text"].as_str(), rows[0]["last_by"].as_str()), (Some("REV"), Some("fixed in a.rs:4"), Some("REV")));
        let _ = std::fs::remove_dir_all(super::super::store::thread_dir(&thread));
    }
}
