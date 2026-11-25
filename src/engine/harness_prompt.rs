use super::Agent;
use std::fmt::Write as _;

/// What the agent is told about codehead, appended to its system prompt.
pub(crate) fn system_prompt(agent: &Agent, thread: &crate::engine::store::Thread, roster: &[Agent]) -> String {
    let others: Vec<String> = thread
        .agents
        .iter()
        .filter(|count| **count != agent.name)
        .map(
            |count_value| match roster.iter().find(|agent_value| &agent_value.name == count_value) {
                Some(agent_2) => format!("{count_value} ({})", harness_label(agent_2)),
                None => count_value.clone(),
            },
        )
        .collect();
    let text = format!(
        "You are {name}, an agent in codehead. codehead runs named agents on threads: 1 thread = 1 task, \
shared with its owner (the person). The owner reads every line of the thread.\n\
- Answer the owner in your reply text: the owner reads it there. Write `@owner` when the owner must act or read something now: codehead notifies them. \
Use the `send` tool of the `codehead` MCP server (`to`: agent names) only to hand work to another agent or to ask it something: \
other agents see only what you send them. Sending to an agent not on the thread brings it in.\n\
- `list_agents` shows every agent, its harness and model. `set_status` reports blocked / review / done to the owner (never working: codehead shows each turn as working): \
review when a step is finished (a plan, a draft) and the next step waits for the owner; done only when the whole task is finished.\n\
- `comment` writes review comments on lines of the changes (the owner's changes view). Only when the owner asks for comments (e.g. a review): 1 comment per finding, never unasked. \
`comments` lists them (id, author, open / resolved, replies). Addressed a comment (fixed, or answered why not): `comment_status` resolved, with a 1-line note. \
A question or a point on a comment: answer with `comment_reply` (id, text).\n\
- Messages from other agents reach you as prompts that start with `[from @name]`.\n\
- A message that starts with `[codehead flow: ...]` is a step of the thread's flow: do that step only, then call `flow_step`. \
The task changed kind: propose another flow with `flow_suggest`.\n\
- Ask the owner, not other agents, for anything only the owner knows.\n\
- Code blocks: a command to run, a file's content, a config or code snippet, any text the owner may copy: \
a fenced code block with a language tag (```sh), on its own lines, never inline code. Inline code only \
for names inside a sentence (a file, a function, a flag, a branch). codehead draws fenced blocks as boxes \
with a copy button.\n\
Thread: {title}\nAgents on the thread: {list}",
        name = agent.name,
        title = thread.title,
        list = if others.is_empty() {
            "only you".to_string()
        } else {
            others.join(", ")
        },
    );

    append_project_context(agent, thread, text)
}

fn append_project_context(agent: &Agent, thread: &crate::engine::store::Thread, mut text: String) -> String {
    if let Some(height) = crate::infra::config::home_repo() {
        text.push_str(&home_block(&height));
    }

    // a sub-thread runs for another thread: 0 own name, 0 own projects
    if thread.of.is_none() {
        text.push_str(&crate::domain::projects::prompt(thread));
    }

    if crate::infra::portless::active() {
        text.push_str(&crate::infra::portless::prompt(
            thread.worktree.as_ref().and(thread.branch.as_deref()),
        ));
    }

    if let Some(path) = thread.port {
        let what = if crate::infra::portless::active() {
            "databases, queues and other non-HTTP listeners"
        } else {
            "dev servers and other listeners"
        };

        let _ = write!(
            text,
            "\nPorts of this thread: {path}-{} ($CODEHEAD_PORT = {path}). Start {what} on these, never on the defaults: other threads run next to you.",
            path + crate::infra::worktree_setup::PORTS - 1
        );
    }

    let profile = crate::domain::agents::session_prompt(agent);

    if !profile.is_empty() {
        text.push_str("\n\n");
        text.push_str(&profile);
    }

    text
}
/// The home repo block of the system prompt: every harness gets the same steering.
pub(crate) fn home_block(home: &std::path::Path) -> String {
    let height = home.display();

    format!(
        "\n\n# Home repo\n\
Home repo: `{height}` ($CODEHEAD_HOME_REPO). Read `{height}/AGENTS.md` before work: its rules apply to you \
wherever you work (notes, plans, reports, other repos)."
    )
}
/// `claude/opus`, `claude`.
pub(crate) fn harness_label(agent: &Agent) -> String {
    match agent
        .model
        .as_deref()
        .filter(|message| !message.is_empty() && *message != "default")
    {
        Some(message_value) => format!("{}/{message_value}", agent.harness),
        None => agent.harness.clone(),
    }
}
