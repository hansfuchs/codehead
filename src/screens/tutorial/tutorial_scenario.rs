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
fn print(error: &Event) {
    let who = error.agent.clone().unwrap_or_else(|| "-".into());

    match &error.kind {
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
        && coach.current().is_some_and(|changes| !matches!(changes, Task::Changes))
    {
        let busy = !crate::engine::client::state(id).1.is_empty();
        let cur = coach.current();

        if cur == Some(Task::Attach) {
            coach.reach(Task::Attach);
        }

        prompt_initial(&context, &mut sent, cur, busy)?;

        std::thread::sleep(Duration::from_millis(300));

        for error_2 in tail.read() {
            print(&error_2);
            coach.see(std::slice::from_ref(&error_2));
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
fn prompt_initial(context: &PromptContext<'_>, sent: &mut Vec<Task>, cur: Option<Task>, busy: bool) -> Result<(), ()> {
    let id = context.id;
    let root = context.root;
    let lead = context.lead;
    let reviewer = context.reviewer;
    let shortcut = |name: &str| crate::domain::shortcuts::expand(&format!("#{name}"));

    // the owner's message TEXT to TO, once the thread is idle
    let prompt = |to: &str, text: String| send_prompt(id, to, &text);

    if let Some(thread_value @ (Task::Send | Task::Mention)) = cur
        && !busy
        && !sent.contains(&thread_value)
    {
        let (to, text) = match (thread_value, reviewer) {
            (Task::Send, _) => (
                lead.clone(),
                format!(
                    "{TASK}\n\n{}\n- {MARKER}: {}",
                    crate::domain::chat::ATTACHMENTS,
                    style_path(root).display()
                ),
            ),
            (_, Some(result)) => (result.clone(), format!("@{result} {}", shortcut("review"))),
            (_, None) => (lead.clone(), format!("@{lead} {}", shortcut("polish"))),
        };

        if let Err(error_value) = prompt(&to, text) {
            println!("prompt: {error_value}");
            return Err(());
        }

        sent.push(thread_value);
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

    if let Err(error_4) =
        plain(&["add", "hello.md"]).and_then(|()| plain(&["commit", "-q", "-m", "hello.md says hi to the crew"]))
    {
        println!("commit: {error_4}");
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

    for error_6 in tail.read() {
        if let Kind::SubThread { state, text, .. } = &error_6.kind {
            println!("sub-thread {state}: {text}");
        }

        print(&error_6);
        coach.see(std::slice::from_ref(&error_6));
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

    if let Err(error_5) = kid.save() {
        println!("sub-thread: {error_5}");
        return Err(());
    }

    let _ = crate::engine::client::call(
        id,
        &json!({"op": "sub_start", "id": kid.id, "title": task, "text": task}),
    );
    let context = format!(
        "[codehead: you run in a sub-thread of the thread “{}”: the same checkout ({}), 0 history of it. The message below is your whole task. Do it, then end with your result as your last message: the thread waits for you; it gets that message.]",
        main.map(|thread_2| thread_2.title).unwrap_or_default(),
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
    if let Err(error_7) = prompt(lead, format!("@{lead} open a PR")) {
        println!("prompt: {error_7}");
        return Err(());
    }

    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(60) && coach.current() == Some(Task::AskPr) {
        std::thread::sleep(Duration::from_millis(300));

        for error_8 in tail.read() {
            print(&error_8);
            coach.see(std::slice::from_ref(&error_8));
        }
    }

    let Some(result_value) = crate::infra::gh::pr_urls(PR_URL).into_iter().next() else {
        return Err(());
    };

    match crate::infra::gh::fetch(&result_value) {
        Ok(path) => println!(
            "mock PR: {} · {:?} · {} commits · +{} -{} · {:?}",
            path.title,
            path.state,
            path.commits.len(),
            path.additions,
            path.deletions,
            path.ready()
        ),
        Err(error_9) => {
            println!("mock PR: {error_9}");
            return Err(());
        }
    }

    coach.reach(Task::MergeView);
    let _ = crate::infra::gh::run(&result_value, &crate::infra::gh::Op::Merge("SQUASH".into(), false));

    coach.repo_at = None;
    coach.see_repo(repo);
    println!(
        "mock PR after merge: {:?}",
        crate::infra::gh::fetch(&result_value).map(|path_value| path_value.state)
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

        for error_10 in ftail.read() {
            print(&error_10);
            coach.see(std::slice::from_ref(&error_10));
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
        .map_err(|error_3| error_3.to_string())
        .and_then(|output_value| {
            if output_value.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output_value.stderr).trim().to_string())
            }
        })
}
