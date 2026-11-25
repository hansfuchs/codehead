use super::{
    Duration, HashMap, Host, IDLE, Input, Instant, Kid, Out, Path, SIGNALED, Sender, Value, bind, catch_signals, json,
    mpsc, pin_branch, serve, store,
};

pub(crate) fn run(id: &str) -> ! {
    let Some(thread_value) = store::load(id) else {
        eprintln!("codehead host: no thread {id}");
        std::process::exit(2)
    };

    let (listener, _lock) = match bind(&thread_value.id) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("codehead host {}: {error}", thread_value.id);
            std::process::exit(0)
        }
    };

    // a plan window ends: its next one starts 1 min after (agents work without the screen too)
    crate::engine::warm::watch();
    let (tx, rx) = mpsc::channel::<Input>();
    let (htx, hrx) = mpsc::channel::<(String, Out)>();
    let fwd = tx.clone();

    std::thread::spawn(move || relay_harness(hrx, &fwd));

    let acc = tx.clone();

    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let tx = acc.clone();

            std::thread::spawn(move || serve(conn, &tx));
        }
    });

    let mut host = new_host(&thread_value, htx);

    restore_host(&mut host, &thread_value, &tx);
    receive_inputs(&mut host, &rx, &tx);
    host.stop();

    std::process::exit(0)
}

fn new_host(thread_value: &store::Thread, htx: Sender<(String, Out)>) -> Host {
    Host {
        id: thread_value.id.clone(),
        exe: crate::engine::exe(),
        sessions: HashMap::new(),
        asks: HashMap::new(),
        harness_tx: htx,
        last: Instant::now(),
        stopping: false,
        subs: std::cell::RefCell::default(),
        live: HashMap::new(),
        turn_use: HashMap::new(),
        setup_running: false,
        queue: Vec::new(),
        later: Vec::new(),
        subagents: crate::engine::subagents::Tracker::default(),
        sub_scan: Instant::now(),
        held: Vec::new(),
        kid: Kid::default(),
        kids_at: Instant::now(),
        resume: thread_value.resume.clone().into_iter().collect(),
    }
}

fn restore_host(host: &mut Host, thread_value: &store::Thread, tx: &Sender<Input>) {
    catch_signals();
    host.close_open_turns();

    // subagents the log has open ran in a process that is gone
    let (subs, lost) = crate::engine::subagents::Tracker::from_log(&store::events(&thread_value.id));

    host.subagents = subs;

    for (agent_value, key) in lost {
        host.log(Some(&agent_value), key);
    }

    // ended subagents resumed while 0 host ran (or an older one missed it)
    if host.subagents.resumable() {
        host.scan_subs();
    }

    // the hooks of the repo change: each host start pins the worktree again
    if let (Some(wt), Some(bytes_value), None) = (
        thread_value.worktree.clone(),
        thread_value.branch.clone(),
        &thread_value.worktree_pending,
    ) {
        std::thread::spawn(move || pin_branch(Path::new(&wt), &bytes_value));
    }

    if thread_value.worktree_pending.is_some() {
        host.start_worktree(thread_value, tx.clone());
    } else if thread_value.setup.as_deref() == Some("pending") {
        host.start_setup(thread_value, tx.clone());
    } else if thread_value.pull {
        host.start_pull(thread_value, tx.clone());
    }

    host.kickoff(thread_value);
}

fn receive_inputs(host: &mut Host, rx: &mpsc::Receiver<Input>, tx: &Sender<Input>) {
    loop {
        // 1 s: a signal ends the host within a second
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Input::Cmd(character, reply)) => {
                run_ok_input_cmd_character(host, &character, &reply);
            }
            Ok(Input::Subscribe(text)) => host.subscribe(text),
            Ok(Input::Harness(agent_2, output_value)) => host.harness(agent_2, output_value),
            Ok(Input::Setup(label, ok, out)) => host.setup_done(&label, ok, out),
            Ok(Input::Pull(ok, out)) => host.pull_done(ok, out),
            Ok(Input::Worktree(result_value)) => host.worktree_done(result_value, tx.clone()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if host.stopping || SIGNALED.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        if !poll_host(host) {
            break;
        }
    }
}

fn relay_harness(rx: mpsc::Receiver<(String, Out)>, fwd: &Sender<Input>) {
    for (agent, output) in rx {
        if fwd.send(Input::Harness(agent, output)).is_err() {
            return;
        }
    }
}

fn run_ok_input_cmd_character(host: &mut Host, character: &Value, reply: &Sender<Value>) {
    let result = match host.cmd(character) {
        Ok(mut value) => {
            value["ok"] = json!(true);
            value
        }
        Err(error_value) => json!({"ok": false, "error": error_value}),
    };

    let _ = reply.send(result);

    host.deliver_later();
}

fn poll_host(host: &mut Host) -> bool {
    // running ones: every 2 s; ended ones a message can resume: every 10 s
    let every = Duration::from_secs(if host.subagents.any() { 2 } else { 10 });

    if (host.subagents.any() || host.subagents.resumable()) && host.sub_scan.elapsed() >= every {
        host.scan_subs();
    }

    host.run_resumes();
    host.kid_check();
    host.kids_check();
    let busy = host.sessions.values().any(|line| line.busy);

    // running subagents keep the host (and their harness process) alive; a resume waits too
    if !busy
        && !host.subagents.any()
        && !host.setup_running
        && host.asks.is_empty()
        && host.resume.is_empty()
        && host.last.elapsed() >= IDLE
        && !host.has_kids()
    {
        // a monitor or background command runs: the host (and its harness) stays
        if !crate::domain::monitors::watching(&host.id, &host.sessions.keys().cloned().collect::<Vec<_>>()) {
            return false;
        }

        host.last = Instant::now();
    }

    // the thread was deleted: end with it
    if !store::thread_dir(&host.id).join("thread.json").is_file() {
        return false;
    }

    true
}
