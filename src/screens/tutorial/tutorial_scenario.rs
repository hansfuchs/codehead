use super::{
    Coach, DONE, Duration, Event, FLOW_TASK, Kind, MARKER, PR_URL, Path, Spec, TASK, Task, git_out, json, start_thread,
    store, style_path,
};

/// The quests without a screen: the owner's messages sent for the owner (shortcuts expanded,
/// the attachment listed), the view tasks counted as done, the commit made with git, the mock
/// PR merged through the gh mock, the flow thread run. Prints the threads' events and hello.md.
/// 0: the script ran through.
pub(super) fn headless(id: &str, repo: &Path, spec: &Spec, root: &Path) -> i32 {
    let mut scenario = Scenario {
        id,
        repo,
        root,
        coach: Coach::new(&spec.crew),
        tail: store::Tail::new(id),
        lead: spec.crew[0].name.clone(),
        reviewer: spec
            .crew
            .iter()
            .find(|member| member.role == "reviewer")
            .map(|member| member.name.clone()),
    };

    if run_intro(&mut scenario).is_err() {
        return 1;
    }

    if commit_changes(&mut scenario).is_err() {
        return 1;
    }

    if run_subthread(&mut scenario).is_err() {
        return 1;
    }

    if merge_pull_request(&mut scenario).is_err() {
        return 1;
    }

    run_flow(&mut scenario)
}
struct Scenario<'context> {
    id: &'context str,
    repo: &'context Path,
    root: &'context Path,
    coach: Coach,
    tail: store::Tail,
    lead: String,
    reviewer: Option<String>,
}
struct PromptContext<'context> {
    id: &'context str,
    root: &'context Path,
    lead: &'context String,
    reviewer: &'context Option<String>,
}
fn send_prompt(id: &str, to: &str, text: &str) -> Result<(), String> {
    crate::engine::client::call(id, &json!({"op":"prompt","to":[to],"text":text})).map(|_| ())
}
fn print(event: &Event) {
    let who = event.agent.clone().unwrap_or_else(|| "-".into());

    match &event.kind {
        Kind::Prompt { from, to, text } => println!(
            "prompt {from} → {}: {}",
            to.join(","),
            text.lines().next().unwrap_or_default()
        ),
        Kind::Text { text } => println!("text {who}: {text}"),
        Kind::Tool { name, .. } => println!("tool {who}: {name}"),
        Kind::Error { message } => println!("error {who}: {message}"),
        Kind::TurnEnd { .. } => println!("turn end {who}"),
        Kind::Flow { op, text } => println!("flow {op}: {text}"),
        _ => {}
    }
}

fn run_intro(scenario: &mut Scenario<'_>) -> Result<(), ()> {
    let id = scenario.id;
    let repo = scenario.repo;
    let context = PromptContext {
        id,
        root: scenario.root,
        lead: &scenario.lead,
        reviewer: &scenario.reviewer,
    };
    let coach = &mut scenario.coach;
    let tail = &mut scenario.tail;
    let mut sent: Vec<Task> = Vec::new();
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(120)
        && coach.current().is_some_and(|task| !matches!(task, Task::Changes))
    {
        let busy = !crate::engine::client::state(id).1.is_empty();
        let cur = coach.current();

        if cur == Some(Task::Attach) {
            coach.reach(Task::Attach);
        }

        send_task_prompt(&context, &mut sent, cur, busy)?;

        std::thread::sleep(Duration::from_millis(300));

        for event in tail.read() {
            print(&event);
            coach.see(std::slice::from_ref(&event));
        }
    }

    println!(
        "hello.md: {:?}",
        std::fs::read_to_string(repo.join("hello.md")).unwrap_or_default()
    );

    if coach.current() != Some(Task::Changes) {
        println!("next task: {:?}", coach.current());
        return Err(());
    }

    Ok(())
}
fn send_task_prompt(
    context: &PromptContext<'_>,
    sent: &mut Vec<Task>,
    cur: Option<Task>,
    busy: bool,
) -> Result<(), ()> {
    let id = context.id;
    let root = context.root;
    let lead = context.lead;
    let reviewer = context.reviewer;
    let shortcut = |name: &str| crate::domain::shortcuts::expand(&format!("#{name}"));

    // the owner's message TEXT to TO, once the thread is idle
    let prompt = |to: &str, text: String| send_prompt(id, to, &text);

    if let Some(task @ (Task::Send | Task::Mention)) = cur
        && !busy
        && !sent.contains(&task)
    {
        let (to, text) = match (task, reviewer) {
            (Task::Send, _) => (
                lead.clone(),
                format!(
                    "{TASK}\n\n{}\n- {MARKER}: {}",
                    crate::domain::chat::ATTACHMENTS,
                    style_path(root).display()
                ),
            ),
            (_, Some(reviewer)) => (reviewer.clone(), format!("@{reviewer} {}", shortcut("review"))),
            (_, None) => (lead.clone(), format!("@{lead} {}", shortcut("polish"))),
        };

        if let Err(error) = prompt(&to, text) {
            println!("prompt: {error}");
            return Err(());
        }

        sent.push(task);
    }

    Ok(())
}
fn commit_changes(scenario: &mut Scenario<'_>) -> Result<(), ()> {
    let repo = scenario.repo;
    let coach = &mut scenario.coach;

    // quest 3: the IDE (mocked in the view), stage + commit hello.md only
    coach.reach(Task::Ide);

    // plain git, as the changes view runs it: the sandbox's own git config only
    let plain = |args: &[&str]| run_git(repo, args);

    if let Err(error) =
        plain(&["add", "hello.md"]).and_then(|()| plain(&["commit", "-q", "-m", "hello.md says hi to the crew"]))
    {
        println!("commit: {error}");
        return Err(());
    }

    coach.repo_at = None;
    coach.see_repo(repo);
    println!(
        "README.md left unstaged: {}",
        git_out(repo, &["status", "--short"]).trim()
    );

    if coach.current() != Some(Task::Sub) {
        println!("next task: {:?}", coach.current());
        return Err(());
    }

    Ok(())
}
fn run_subthread(scenario: &mut Scenario<'_>) -> Result<(), ()> {
    start_subthread(scenario)?;
    let coach = &mut scenario.coach;
    let tail = &mut scenario.tail;
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(60) && matches!(coach.current(), Some(Task::Sub | Task::SubDone)) {
        observe_subthread(coach, tail);
    }

    if coach.current() != Some(Task::AskPr) {
        println!("next task: {:?}", coach.current());
        return Err(());
    }

    Ok(())
}

fn observe_subthread(coach: &mut Coach, tail: &mut store::Tail) {
    std::thread::sleep(Duration::from_millis(300));

    for event in tail.read() {
        if let Kind::SubThread { state, text, .. } = &event.kind {
            println!("sub-thread {state}: {text}");
        }

        print(&event);
        coach.see(std::slice::from_ref(&event));
    }
}
fn start_subthread(scenario: &Scenario<'_>) -> Result<(), ()> {
    let id = scenario.id;
    let repo = scenario.repo;
    let lead = &scenario.lead;
    let reviewer = &scenario.reviewer;

    // quest 4: a sub-thread, started as the view's `>sub` starts it
    let side = reviewer.clone().unwrap_or_else(|| lead.clone());
    let task = format!("@{side} does README.md need a change?");
    let main = store::load(id);
    let mut kid = store::Thread::new(&task, &task, repo, vec![side.clone()]);

    kid.of = Some(id.to_string());

    if let Err(error) = kid.save() {
        println!("sub-thread: {error}");
        return Err(());
    }

    let _ = crate::engine::client::call(
        id,
        &json!({"op": "sub_start", "id": kid.id, "title": task, "text": task}),
    );
    let context = format!(
        "[codehead: you run in a sub-thread of the thread “{}”: the same checkout ({}), 0 history of it. The message below is your whole task. Do it, then end with your result as your last message: the thread waits for you; it gets that message.]",
        main.map(|thread| thread.title).unwrap_or_default(),
        repo.display()
    );
    let _ = crate::engine::client::call(
        &kid.id,
        &json!({"op": "prompt", "to": [side], "text": task, "context": context}),
    );

    Ok(())
}
fn merge_pull_request(scenario: &mut Scenario<'_>) -> Result<(), ()> {
    let id = scenario.id;
    let repo = scenario.repo;
    let lead = &scenario.lead;
    let coach = &mut scenario.coach;
    let tail = &mut scenario.tail;
    let prompt = |to: &str, text: String| send_prompt(id, to, &text);

    // quest 5: the PR link, the mock PR, its merge
    if let Err(error) = prompt(lead, format!("@{lead} open a PR")) {
        println!("prompt: {error}");
        return Err(());
    }

    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(60) && coach.current() == Some(Task::AskPr) {
        std::thread::sleep(Duration::from_millis(300));

        for event in tail.read() {
            print(&event);
            coach.see(std::slice::from_ref(&event));
        }
    }

    let Some(url) = crate::infra::gh::pr_urls(PR_URL).into_iter().next() else {
        return Err(());
    };

    match crate::infra::gh::fetch(&url) {
        Ok(pull_request) => println!(
            "mock PR: {} · {:?} · {} commits · +{} -{} · {:?}",
            pull_request.title,
            pull_request.state,
            pull_request.commits.len(),
            pull_request.additions,
            pull_request.deletions,
            pull_request.ready()
        ),
        Err(error) => {
            println!("mock PR: {error}");
            return Err(());
        }
    }

    coach.reach(Task::MergeView);
    let _ = crate::infra::gh::run(&url, &crate::infra::gh::Op::Merge("SQUASH".into(), false));

    coach.repo_at = None;
    coach.see_repo(repo);
    println!(
        "mock PR after merge: {:?}",
        crate::infra::gh::fetch(&url).map(|pull_request| pull_request.state)
    );

    if coach.current() != Some(Task::Menu) {
        println!("next task: {:?}", coach.current());
        return Err(());
    }

    Ok(())
}
fn run_flow(scenario: &mut Scenario<'_>) -> i32 {
    let repo = scenario.repo;
    let lead = &scenario.lead;
    let coach = &mut scenario.coach;

    // quest 7's thread: chore, run by the lead's steps
    coach.reach(Task::Flow);

    let Some(fid) = start_thread(coach, repo, FLOW_TASK, Vec::new(), Vec::new(), lead) else {
        println!("flow thread: 0 start");
        return 1;
    };

    let mut ftail = store::Tail::new(&fid);
    let start_at = std::time::Instant::now();

    while start_at.elapsed() < Duration::from_secs(90) && coach.current() == Some(Task::FlowRun) {
        std::thread::sleep(Duration::from_millis(300));

        for event in ftail.read() {
            print(&event);
            coach.see(std::slice::from_ref(&event));
        }
    }

    println!("next task: {:?}", coach.current());

    if coach.current() == Some(Task::Leave) { DONE } else { 1 }
}

fn run_git(repo: &Path, args: &[&str]) -> Result<(), String> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| error.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        })
}
