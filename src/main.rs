#![recursion_limit = "256"]
//! codehead: pick notes, write the task, then work the thread with its agents in this terminal.

mod domain;
mod engine;
mod features;
mod infra;
#[cfg(test)]
mod perf;
mod screens;
mod ui;

use std::path::Path;

use crate::infra::terminal::fail;

const HELP: &str = "codehead: write the task (+ notes, + Claude Code context), then work the thread with its agents.

usage: codehead                    the thread updated last (0 threads: the start screen)
       codehead [-a agent[,agent…]] [-b branch] [-c] [-n] [title...]
       codehead --view <thread>   thread view of a thread (id or id prefix)
       codehead --cost <thread>   cost box of a thread, full screen
       codehead --version         print the version
       codehead home [init]       what codehead keeps in the home repo; init: move agents, flows, servers, settings there
       codehead setup [--dry]     the setup wizard; --dry: every screen, 0 writes, then the list of what a real run does
       codehead --update          install the latest release (curl -fsSL https://codehead.sh/install | sh)
       codehead --kill-hosts      stop every thread host: its agents and their subagents end; the next
                                  message to a thread starts a new host (with the installed binary)
  -a  agents preselected for the new thread (default: the default agent)
  -b  branch for the new worktree (default: <prefix>/<title slug>, prefix \"task\")
  -c  copy the picked note paths to the clipboard, start nothing
  -n  dry run: print what would start, start nothing
  title text prefills the title input.

config:  ~/.config/codehead.toml ($CODEHEAD_CONFIG overrides). keys, each overridden by its env var:
           notes  = \"~/notes\"   notes root ($CODEHEAD_NOTES; default: notes/ of the codehead app dir)
           styles = \"~/styles\"  output styles ($CODEHEAD_STYLES; default: output-styles/ of the app dir)
           repo   = \"~/app\"     repo of the new tab's where rows ($CODEHEAD_REPO; default: the start dir's)
           agent  = \"amon\"      default agent ($CODEHEAD_AGENT)
           prefix = \"task\"      branch prefix ($CODEHEAD_PREFIX)
           home   = \"~/home\"    home repo ($CODEHEAD_HOME_REPO). with home/codehead/ (codehead home init): settings
                                   there too, shared: codehead/settings.toml, this device: codehead/devices/<device>.toml
           device = \"mbp\"       this device's name (default: the host name)
           models = \"fable,opus,sonnet,haiku\"  Claude models ($CODEHEAD_MODELS)
           messages = \"bubbles\"  thread view messages: bubbles or lines ($CODEHEAD_MESSAGES)
           thoughts = \"hidden\"  thread view thoughts: hidden, folded or open ($CODEHEAD_THOUGHTS)
           [keys]   menu = \"ctrl+p\" ($CODEHEAD_MENU_KEY), command = \">\" ($CODEHEAD_COMMAND_KEY)
           [colors] scheme = \"lavender\" | \"coral\" | \"amber\" ($CODEHEAD_SCHEME); accent, text, … = \"#RRGGBB\"
           [aliases]
           \"project:platform\" = \"~/projects/exampleorg/project/notes/platform/\"
           [prices.\"<model>\"] input = 1.25, cached = 0.125, output = 10.0   $ per 1M tokens, for
                                   harnesses that report 0 $ (codex, ACP); 0 entry: 0 $
settings: 7th tab (F7). edit fields and path aliases; ctrl+s saves, ctrl+r discards edits / reloads.
          enter on + add (menu: add path alias) adds an alias; d d removes the current alias, on a
          field ctrl+d: back to its default (↺ too).
          path fields use the prompt's path suggestions: tab completes, esc closes suggestions.
          esc on a field: its edit undone (0 apply). ↑↓: next / previous field; tab: the sub-tabs, the tabs.
          environment variables override file values. edits stay until saved or reloaded.
default agent: config agent, else amon, else the first agent.
         preselected for new threads, takes over threads of deleted agents.
project: git repo of the current directory. the task's where rows: repo (enter: a 2-pane picker,
         left the parent's dirs, right the current dir's, ←→ out / in, type: filter, enter: pick;
         home repos without a clone: cloned first), then branch (enter: dropdown): the current
         branch (default, works there), main, the other branches (newest commit first), a new branch
         (main checkout, git switch -c, 0 worktrees), or a new branch + worktree (-b sets this).
         below it: checkout the picked branch (else the main checkout works as is), stash local
         changes that block the checkout (git stash). a start on main / master in an existing
         checkout: git pull --ff-only in the thread before the 1st turn (a failed pull: a note
         to the agents, the thread goes on). setting repo: the where rows start on that repo.
         0 git repos: the current directory itself (thread works in place)
prompt:  @: agent names. >: commands, in dropdowns above the prompt box (never over it; a submenu
         beside its parent, 0 room there: above them). max. 10 rows, ▲ ▼ on the right border: more rows.
         mention → files (of the repo / worktree), notes, threads, artifacts (urls in the Claude
           Code transcripts) → a list, oldest on top, the latest current; typed text: fuzzy filter.
           enter: the relative path / note path / thread name / url goes into the text.
         skills → the skills of the agents' harnesses (Claude Code, Codex: SKILL.md files of the
           user, the plugins, the project) with their description. enter: /name ($name: Codex).
         esc: 1 level back.
         a path (src/, ./a/b, ~/x/, /usr/lo): the entries of that directory in a dropdown, as the
         path is typed (dirs first; hidden ones after a `.`). relative: from the repo / worktree
         top, else the dir. tab inserts the row (a dir: its entries next); enter too, after ↑↓.
         aliases work as paths: project:platform/ lists the target directory. prompts retain
         short aliases; sent messages expand aliases to full paths.
         ctrl+v: clipboard image → [Image #n], a dropped image path too; text over
         1024 chars (code: never) → [Pasted text #n +L lines]. the files go into the message as a list.
drafts:  an unsent task goes to ~/.cache/codehead/drafts ($CODEHEAD_DRAFTS) after every key. the new tab
         shows \"view drafts (n)\" on top: enter lists them; enter loads 1, d d deletes it, esc: back.
         a started thread drops its draft. drafts older than 30 days go. \"save as draft\" (right of
         it) or ctrl+s: the task as a kept draft (★ saved, 0 cleanup by age), the form starts over.
         n in the menu: an empty form, the task on screen stays a draft.
artifacts: codehead sets CLAUDE_CODE_ARTIFACT=1 for the sessions it starts: Claude Code agents can
         publish web artifacts (off by default in SDK sessions). a value set before codehead stays.
esc:     1 level back: never saves, never drops edits unasked (edited text: esc again within 2 s drops
         it; the agent form and reset ask).
keys:    1 key, 1 meaning: a letter key never acts with ctrl / alt / ⌘ held (alt+P never pushes).
         outward or destructive keys ask first: a 2nd press (d d, S S, ctrl+d ctrl+d, enter enter).
         the same keys on every screen: tab / shift+tab: the next / previous region (focus); arrows:
         inside it. ctrl+. (shift+F10 in any terminal): the context menu, the actions of the thing in
         focus, beside it. ctrl+p: the menu. e: the IDE / editor at the thing. d d: delete. r: read again.
tabs:    start screen: F1-F7 (alt+1-7, ctrl+1-7) switch tabs (word jumps: ctrl / alt+←→). tab /
         shift+tab: the tabs row ↔ the tab's content (settings: its sub-tabs too). ←→ on a row of tabs:
         the next / previous tab; ↑↓: the fields.
quit:    esc esc on the start screen; ctrl+c 2× within 1 s (a 1st ctrl+c on text: clears it, ctrl+z back);
         menu → quit codehead. q never quits.
updates: 1 check per day (github.com releases, in the background). a newer release: 1 line after
         codehead quits. codehead --update installs it. $CODEHEAD_NO_UPDATE_CHECK=1: 0 checks.
debug:   $CODEHEAD_KEYLOG=<file>: every key of the text inputs, with its time (a report on keys the
         terminal sends in a way codehead does not expect, e.g. the macOS double-space period).
inputs:  all text fields (title, prompt, agent form, filters, thread prompt): ctrl+z / ⌘z undo,
         ctrl+shift+z / ⌘⇧z redo (ctrl+y too). ⌘ keys need a terminal with the kitty keyboard protocol.
         Markdown lists (`- `, `* `, `1. `, 2 spaces per layer): enter starts the next item at the same
         layer; enter / backspace on an empty item: 1 layer out (1st layer: the marker goes), space: 1 layer in.
start:   the new tab: the prompt first (its 1st line: the thread title, unless the title field has one),
         then title, repo, where, opts, agents (ctrl+. on a card: its flow), notes. type, ctrl+enter. notes: ↓ to the notes field, enter.
threads: worktrees, then recent Claude Code sessions. enter on a session:
         new thread in its dir, its last response as context.
thread view: the thread's agents on top (sprite, model, working / idle / waits on you; ★ the lead),
         the stream (every message, tool run and open question, from the thread's event log), the
         prompt below, the cost box beside it (screens ≥ 100 columns).
         tab / shift+tab: the next / previous region: the prompt, the view (changes, merge, files), the
         source control panel, the repo tabs, an open question, the agents box (←→ an agent, its menu
         above it: flow, remove: 2 enters, replace with: an agent off the thread takes over, flow too).
         ctrl+.: the context menu of the region (shift+F10, the menu key: any terminal).
         ctrl+m (kitty protocol): the prompt again, from every pane, box, filter and the menu.
         below the prompt: who gets the message, where the thread works (repo ⎇ branch, (worktree);
         1 per checkout, read again every 3 s).
         a message without @name goes to the lead; @name: to that agent (an agent not on the thread
         joins it). agents message each other with the `send` tool of codehead's MCP server.
         wheel, pgup / pgdn, shift+↑↓: scroll · esc: back to live, else interrupt (after 0.5 s) · esc esc: back.
         send: ctrl+enter (⌘enter, alt+enter); ctrl+s never sends. a question: ↑↓ space enter 1-9,
         letters: Other, tab: the next region (question → agents → prompt). a new question takes the keys only on an empty prompt
         after 1 s of 0 keys; else it waits for tab (0 typed key answers it by surprise).
         click a link: open it; ctrl+click: any URL on screen. ctrl+c: copies the prompt's selection,
         else clears the prompt (ctrl+z: back), 2× on an empty one closes codehead.
         #: shortcuts (settings → shortcuts): #name stands for its text; tab / enter puts the text in,
         a #name left in the prompt: its text on send. ↑↓ in any dropdown: the row into the text at once.
         >: commands, as in the task prompt (files: of the thread's worktree), and
         branch: a copy of the thread (events, members, dir); each agent's next session forks its
         Claude Code session (claude --resume ID --fork-session). the old thread stays.
         @: agent names. ←→ on a name: + add, or ⇄ replace an agent of the thread: on send, the old
         agent leaves and the new one gets the message with the old one's last messages.
         menu → IDE on the worktree (every pane): the IDE on the repo root, several repos: a picker (setting editor: auto, code, cursor, zed, idea …).
         views (row 0): F1-F4 (ctrl+1-4, alt+1-4): chat, changes, merge, files. tab: the next region,
         esc: back to the view. yazi: files view ctrl+., menu. changes: 1 box per file in review
         order (schema, logic, config, UI, docs, tests, generated), j k box (j: down), n N hunk,
         v reviewed (a comment: resolved), enter fold (a comment: edit), e IDE at the line, w side by
         side / unified, r read git again, ctrl+f filter, c comment (a finding: into the batch), d d
         delete a comment / dismiss a finding. ←: the panel; ctrl+. → show / hide the panel.
         the panel's ctrl+. (right of the panel): its row's actions, then the ship steps under
         \"ship\": ship (agent: commit, push, PR, review, CI green), push, fetch + rebase, create PR /
         PR draft (agent, sub-thread, commits only), open the PR (gh), open PR on top: update its
         description (agent), merge it, CI log → agents (a stopped rebase: the agents resolve,
         continue, abort); outward steps: enter enter.
         ↑ on the 1st row of a PR's diff: its views row.
         flow waits (gate, loop max, role with 0 agent): a question in the picker under the stream,
         after the agents' ones; text in the prompt goes along as the note. > flow: the flow's steps.
         ctrl+f: the search bar above the prompt (the chat's messages, notes, shell, tool calls);
         enter / ↑ the next older match, ↓ the next newer one, esc closes.
         tool runs (▸): click opens / closes the calls (input + output); ctrl+e (empty prompt): all.
         thoughts (setting thoughts = hidden | folded | open): ∴ rows, a click opens 1, ctrl+t: all.
         ctrl+v pastes a clipboard image as [Image #n], a dropped image path too. pasted text over
         1024 chars (code: never): [Pasted text #n +L lines]. both go in as attachments.
         a click on a marker or a 📎 row: the files view on the file (section attachments), ctrl+.: yazi.
         files view: images + pdf page 1 drawn in Ghostty, kitty, Rio, iTerm2, WezTerm, sixel
         terminals (setting previews = auto | kitty | iterm2 | sixel | off; 0 in tmux, zellij, screen).
         ⌘/ctrl+Enter (alt+Enter) sends, Enter: new line. ↑ on the 1st row of an empty
         prompt / ↓ on the last: the thread's sent messages.
         agent questions and permission requests: ↑↓ / 1-9 / space / enter on the empty prompt, or a
         click on a choice.
engine:  codehead runs the agents itself. 1 host process per thread (codehead host <id>, detached:
         agents keep working after codehead closes; it exits 15 min. after the last turn). it runs 1
         harness session per agent and resumes it on the agent's next message. harnesses: claude
         (claude -p, stream-json), codex, opencode, goose, cursor, pi, omp, devin, grok, kimi, amp,
         hermes, openclaw (Agent Client Protocol; $CODEHEAD_ACP_<NAME> or `command` in the agent
         file: another command). ACP autopilot: guarded commands (force push, sudo, …) still ask. records: ~/.local/share/codehead/threads/<id>/ (thread.json,
         events.jsonl; $CODEHEAD_HOME). agents: ~/.config/codehead/agents/<name>.toml.
         sockets: $XDG_RUNTIME_DIR/codehead. logs: ~/.local/share/codehead/logs.
         codehead ctl <thread> '<json>': 1 command to a thread's host (scripts).
menu:    ctrl+p on every screen (setting keys.menu: 1 key codehead binds to nothing else), esc / ctrl+p
         closes. ←→: tabs (outside threads / merge); tab / shift+tab, ↓ / ↑ at the top of a list: in / out of a
         tab's sub-tabs (threads: repos, merge: states) and its list. quick: new, settings + the commands of
         the screen (keys: the keys of the screen). threads: filter (type) + thread cards, enter opens.
         agents: the agents of the thread in view (enter, enter: remove = it leaves the thread), add,
         edit → the agents tab of the start screen.";

#[derive(Default)]
pub(crate) struct Args {
    pub(crate) agent: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) copy: bool,
    pub(crate) dry: bool,
    pub(crate) title: String,
    pub(crate) view: Option<String>,
}

fn parse() -> Args {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    let mut rest = Vec::new();

    while let Some(arg) = it.next() {
        parse_option(&mut args, &mut it, &mut rest, arg);
    }

    args.title = rest.join(" ");

    args
}

/// `--kill-hosts`: stop every thread host, 1 line per host. Exit 1: a host did not end.
fn kill_hosts() -> ! {
    let out = engine::client::stop_all(std::time::Duration::from_secs(10));

    if out.is_empty() {
        println!("0 hosts run");
        std::process::exit(0)
    }

    for (id, error) in &out {
        let title = engine::store::load(id).map(|thread| thread.title).unwrap_or_default();

        match error {
            None => println!("stopped  {id}  {title}"),
            Some(error) => println!("failed   {id}  {title}: {error}"),
        }
    }

    let failed = out.iter().filter(|(_, error)| error.is_some()).count();

    println!("{} of {} hosts stopped", out.len() - failed, out.len());

    std::process::exit(i32::from(failed != 0))
}

// ---------- terminal ----------

// ---------- threads ----------

/// Claude Code leaves the Artifact tools (web artifacts on claude.ai) out of SDK sessions, and
/// codehead runs its agents as SDK sessions: $`CLAUDE_CODE_ARTIFACT=1` puts them back. Set here, for
/// every session codehead starts (hosts and agents inherit it). A value set by the person stays.
fn enable_artifacts() {
    if std::env::var_os("CLAUDE_CODE_ARTIFACT").is_none() {
        // SAFETY: the 1st statement of main, 0 other threads yet.
        unsafe { std::env::set_var("CLAUDE_CODE_ARTIFACT", "1") };
    }
}

fn main() {
    // called as `git` (the shim on the agents' PATH): the push lock, then the real git
    if std::env::args_os()
        .next()
        .is_some_and(|arg0| Path::new(&arg0).file_name().is_some_and(|name| name == "git"))
    {
        crate::infra::pushlock::shim_main();
    }

    engine::trace("main");
    crate::infra::pushlock::install();
    enable_artifacts();
    sub();

    // the palette before any screen (`--cost` draws in parse); a broken file fails in start
    let _ = crate::infra::config::load();
    let args = parse();

    crate::infra::update::spawn_check();
    crate::screens::application::start(args);
}

/// Subcommands without a screen: the thread host, the agents' MCP server, scripting.
fn sub() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let arg = |i: usize| {
        argv.get(i)
            .cloned()
            .unwrap_or_else(|| fail(&format!("{}: argument {i} missing", argv[0]), 2))
    };

    match argv.first().map(String::as_str) {
        Some("host") => engine::host::run(&arg(1)),
        Some("mcp") => engine::mcp::run(),
        Some("home") => crate::infra::home_init::run(argv.get(1).map(String::as_str)),
        Some("setup") => crate::screens::tutorial::wizard::run(argv.iter().any(|arg| arg == "--dry")),
        Some("tutorial") => crate::screens::tutorial::run(&argv[1..]),
        Some("tutor-agent") => crate::screens::tutorial::agent(&argv[1..]),
        Some("ctl") => {
            // codehead ctl <thread> '<json command>': 1 command to the thread's host
            let command: serde_json::Value =
                serde_json::from_str(&arg(2)).unwrap_or_else(|error| fail(&format!("ctl: {error}"), 2));

            match engine::client::call(&arg(1), &command) {
                Ok(reply) => println!("{reply}"),
                Err(error) => fail(&error, 1),
            }

            std::process::exit(0)
        }
        _ => {}
    }
}

fn parse_option(args: &mut Args, it: &mut impl Iterator<Item = String>, rest: &mut Vec<String>, arg: String) {
    match arg.as_str() {
        "-h" | "--help" => {
            println!("{HELP}");
            std::process::exit(0)
        }
        "-V" | "--version" => {
            println!("codehead {}", crate::infra::update::VERSION);
            std::process::exit(0)
        }
        "--update" => crate::infra::update::run(),
        "--kill-hosts" => kill_hosts(),
        "-a" => args.agent = Some(it.next().unwrap_or_else(|| fail("-a: value required", 2))),
        "-b" => args.branch = Some(it.next().unwrap_or_else(|| fail("-b: value required", 2))),
        "-c" => args.copy = true,
        "--cost" => crate::features::cost::run(&it.next().unwrap_or_else(|| fail("--cost: thread id required", 2))),
        "--view" => args.view = Some(it.next().unwrap_or_else(|| fail("--view: thread id required", 2))),
        "-n" => args.dry = true,
        "--" => {
            rest.extend(it.by_ref());
        }
        other if other.starts_with('-') && other.len() > 1 => fail(&format!("unknown option {other}\n\n{HELP}"), 2),
        _ => rest.push(arg),
    }
}
