use super::{Agent, BufRead, HashMap, PR_URL, Path, PathBuf, STEP, TASK, THINK, TOOL, Value, WORD, Write, io, json};

/// The step name of a flow message: a line `[codehead flow: chore · step 1/3: implement]` →
/// implement. Only at a line's start: the system prompt names the form in its text.
pub(super) fn step_of(text: &str) -> Option<String> {
    let head = text.lines().find(|line| line.starts_with("[codehead flow:"))?;
    let head = &head[..head.find(']')?];

    Some(head.rsplit(": ").next()?.trim().to_string())
}

/// The steps of a flow, 1 after the other, while they are this agent's.
pub(super) fn flow_turn(agent: &mut Agent, text: &str) {
    let mut text = text.to_string();

    while let Some(step) = step_of(&text) {
        match step.as_str() {
            "implement" => {
                agent.edit("hi crew.\nwelcome aboard.\nflows: 1 step at a time.\n");
                agent.say("implemented: 1 line on flows.");
            }
            "verify" => {
                agent.read();
                agent.say("verified: hello.md reads right.");
            }
            "report" => agent.say("report: hello.md got 1 line on flows. the flow is done."),
            other => agent.say(&format!("{other}: done.")),
        }

        match agent.step_done() {
            Some(next) => {
                std::thread::sleep(STEP);
                text = next;
            }
            None => break,
        }
    }
}

/// An owner message that asks for a pull request.
pub(super) fn asks_pr(text: &str) -> bool {
    let lower = text.to_lowercase();

    lower.contains("pull request")
        || lower
            .split(|character: char| !character.is_alphanumeric())
            .any(|word| word == "pr")
}

/// The turns of the script: ROLE, the prompt TEXT, turns done before.
pub(super) fn turn(agent: &mut Agent, role: &str, kv: &HashMap<String, String>, text: &str, count: u32) {
    let from_agent = text.lines().any(|line| line.starts_with("[from @"));
    let (builder, reviewer) = (
        kv.get("builder").cloned().unwrap_or_default(),
        kv.get("reviewer").cloned().unwrap_or_default(),
    );

    std::thread::sleep(THINK);

    if step_of(text).is_some() {
        return flow_turn(agent, text);
    }

    // a sub-thread's task (any role): the check, the result as the last message
    if text.contains("you run in a sub-thread") {
        agent.say("checking README.md against hello.md.");
        agent.read_file("README.md");
        agent.say("result: README.md names 0 greeting. 0 change needed for the PR.");
        return;
    }

    if !from_agent && asks_pr(text) {
        announce_pull_request(agent);
        return;
    }

    match role {
        "planner" => {
            agent.say(&format!(
                "plan: 1 change. hello.md greets the crew, in the style of the guide. {builder} does it."
            ));
            agent.hand(&builder, TASK);
        }
        "builder" if from_agent && !reviewer.is_empty() && text.contains(&format!("[from @{reviewer}]")) => {
            agent.edit("hi crew.\nwelcome aboard.\n");
            agent.say("fixed: a newline at the end, plus a welcome line.");
            agent.hand(&reviewer, "fixed. 1 more look?");
        }
        "builder" if count == 0 => {
            agent.say("reading the style guide first.");
            agent.read_file("../paste/pasted-text-1.txt");
            agent.edit("hi crew.");
            agent.say("done: hello.md says hi to the crew. lower case, 1 line.");
        }
        "builder" => {
            agent.edit("hi crew.\nwelcome aboard.\n");
            agent.say("added a 2nd line: welcome aboard.");
        }
        "reviewer" if from_agent => {
            agent.read();
            agent.say("approved. 0 more nits.");
        }
        "reviewer" => {
            agent.read();
            agent.say(&format!(
                "1 nit: the style guide wants a newline at the end. hello.md has 0. sent to {builder}."
            ));
            agent.hand(&builder, "hello.md: add a newline at the end, please (style guide).");
        }
        _ => agent.say("ok."),
    }
}

/// `codehead tutor-agent role=… builder=… reviewer=…`.
pub(crate) fn agent(args: &[String]) -> ! {
    let kv: HashMap<String, String> = args
        .iter()
        .filter_map(|arg| arg.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let role = kv.get("role").cloned().unwrap_or_default();
    let me = std::env::var("CODEHEAD_AGENT").unwrap_or_default();
    let tutor = Agent {
        out: io::stdout(),
        session: format!("tutor-{me}"),
        cwd: PathBuf::from("."),
        me: me.clone(),
        thread: std::env::var("CODEHEAD_THREAD").unwrap_or_default(),
        next: 0,
    };
    let turns = 0u32;

    serve_tutorial_agent(&(kv), &(role), tutor, turns)
}

fn serve_tutorial_agent(kv: &HashMap<String, String>, role: &str, mut agent: Agent, mut turns: u32) -> ! {
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };

        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        handle_agent_method(&mut agent, role, kv, &mut turns, &message);
    }

    std::process::exit(0)
}

impl Agent {
    fn send(&mut self, value: &Value) {
        let _ = writeln!(self.out, "{value}");
        let _ = self.out.flush();
    }

    fn update(&mut self, update: &Value) {
        let message = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": self.session, "update": update}});

        self.send(&message);
    }

    /// TEXT word by word, as a model streams.
    fn say(&mut self, text: &str) {
        stream_text(self, text);
    }

    /// A tool call: shown, WORK done, its output shown. The output.
    pub(super) fn tool(
        &mut self,
        title: &str,
        kind: &str,
        input: &Value,
        work: impl FnOnce(&Path) -> String,
    ) -> String {
        self.next += 1;
        let id = format!("t{}", self.next);

        self.update(&(json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": kind, "status": "in_progress", "rawInput": input})));
        std::thread::sleep(TOOL);
        let output = work(&self.cwd);

        self.update(
            &(json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed", "rawOutput": output})),
        );

        output
    }

    fn edit(&mut self, text: &str) {
        let path = self.cwd.join("hello.md");
        let text = text.to_string();

        self.tool(
            "Edit hello.md",
            "edit",
            &(json!({"file_path": path.display().to_string()})),
            move |cwd| {
                let _ = std::fs::write(cwd.join("hello.md"), text);

                "ok".into()
            },
        );
    }

    fn read(&mut self) {
        self.read_file("hello.md");
    }

    /// A shell command shown, 0 run (the mock PR: 0 GitHub call).
    fn shell(&mut self, cmd: &str, output: &str) {
        let output = output.to_string();

        self.tool(cmd, "execute", &(json!({"command": cmd})), move |_| output);
    }

    /// A message to agent TO, through the host (as the MCP `send` tool does).
    fn hand(&mut self, to: &str, text: &str) {
        let (thread, me) = (self.thread.clone(), self.me.clone());
        let (to, text) = (to.to_string(), text.to_string());

        self.tool(
            "send",
            "other",
            &(json!({"to": [to], "message": text})),
            move |_| match crate::engine::client::send(
                &thread,
                &json!({"op": "send", "from": me, "to": [to], "text": text}),
            ) {
                Ok(_) => "sent".into(),
                Err(error) => error,
            },
        );
    }

    /// The flow step done (as the MCP `flow_step` tool does). The next step's text, if it is
    /// this agent's.
    fn step_done(&mut self) -> Option<String> {
        let (thread, me) = (self.thread.clone(), self.me.clone());
        let mut next = None;

        let out = self.tool(
            "flow_step",
            "other",
            &(json!({})),
            |_| match crate::engine::client::send(&thread, &json!({"op": "flow_step", "agent": me})) {
                Ok(result) => result.to_string(),
                Err(error) => error,
            },
        );

        if let Ok(value) = serde_json::from_str::<Value>(&out) {
            next = value["text"].as_str().map(String::from);
        }

        next
    }
}

fn stream_text(agent: &mut Agent, text: &str) {
    for (i, word) in text.split(' ').enumerate() {
        let chunk = if i == 0 { word.to_string() } else { format!(" {word}") };

        agent.update(&(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": chunk}})));
        std::thread::sleep(WORD);
    }
}

fn announce_pull_request(agent: &mut Agent) {
    agent.say("pushing the branch, opening the pull request.");
    agent.shell(
        "git push -u origin task/hello",
        "branch 'task/hello' set up to track 'origin/task/hello'.",
    );
    agent.shell("gh pr create --fill", PR_URL);
    agent.say(&format!("PR opened: {PR_URL}"));
}

fn handle_agent_method(agent: &mut Agent, role: &str, kv: &HashMap<String, String>, turns: &mut u32, message: &Value) {
    let id = message.get("id").cloned();

    let reply = |agent: &mut Agent, result: Value| {
        if let Some(id) = &id {
            agent.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
        }
    };

    match message["method"].as_str() {
        Some("initialize") => reply(
            agent,
            json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": false}}),
        ),
        Some("session/new") => {
            if let Some(cwd) = message["params"]["cwd"].as_str() {
                agent.cwd = PathBuf::from(cwd);
            }

            let sid = agent.session.clone();

            reply(agent, json!({"sessionId": sid}));
        }
        Some("session/prompt") => {
            let text: String = message["params"]["prompt"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();

            turn(agent, role, kv, &text, *turns);
            *turns += 1;
            reply(agent, json!({"stopReason": "end_turn"}));
        }
        Some(_) if id.is_some() => reply(agent, json!({})),
        _ => {}
    }
}
