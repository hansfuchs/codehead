use super::{
    Duration, HashMap, Host, IDLE, Input, Instant, Kid, Out, Path, SIGNALED, Sender, Value, bind, catch_signals, json,
    mpsc, pin_branch, serve, store,
};

/// Run the host of thread ID until idle or `stop`.
pub(crate) fn run(id: &str) -> ! {
    let Some(thread) = store::load(id) else {
        eprintln!("codehead host: no thread {id}");
        std::process::exit(2)
    };

    let (listener, lock) = match bind(&thread.id) {
        Ok(bound) => bound,
        Err(error) => {
            eprintln!("codehead host {}: {error}", thread.id);
            std::process::exit(0)
        }
    };

    // a plan window ends: its next one starts 1 min after (agents work without the screen too)
    crate::engine::warm::watch();
    let (tx, rx) = mpsc::channel::<Input>();
    let (htx, hrx) = mpsc::channel::<(String, Out)>();
    let relay_tx = tx.clone();

    std::thread::spawn(move || relay_harness(hrx, &relay_tx));

    let accept_tx = tx.clone();

    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let tx = accept_tx.clone();

            std::thread::spawn(move || serve(conn, &tx));
        }
    });

    let mut host = new_host(&thread, htx);

    restore_host(&mut host, &thread, &tx);
    receive_inputs(&mut host, &rx, &tx);
    host.stop();
    drop(lock);

    std::process::exit(0)
}

fn new_host(thread: &store::Thread, htx: Sender<(String, Out)>) -> Host {
    Host {
        id: thread.id.clone(),
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
        resume: thread.resume.clone().into_iter().collect(),
    }
}

fn restore_host(host: &mut Host, thread: &store::Thread, tx: &Sender<Input>) {
    catch_signals();
    host.close_open_turns();

    // subagents the log has open ran in a process that is gone
    let (subs, lost) = crate::engine::subagents::Tracker::from_log(&store::events(&thread.id));

    host.subagents = subs;

    for (agent, kind) in lost {
        host.log(Some(&agent), kind);
    }

    // ended subagents resumed while 0 host ran (or an older one missed it)
    if host.subagents.resumable() {
        host.scan_subs();
    }

    // the hooks of the repo change: each host start pins the worktree again
    if let (Some(wt), Some(branch), None) = (thread.worktree.clone(), thread.branch.clone(), &thread.worktree_pending) {
        std::thread::spawn(move || pin_branch(Path::new(&wt), &branch));
    }

    if thread.worktree_pending.is_some() {
        host.start_worktree(thread, tx.clone());
    } else if thread.setup.as_deref() == Some("pending") {
        host.start_setup(thread, tx.clone());
    } else if thread.pull {
        host.start_pull(thread, tx.clone());
    }

    host.kickoff(thread);
}

fn receive_inputs(host: &mut Host, rx: &mpsc::Receiver<Input>, tx: &Sender<Input>) {
    loop {
        // 1 s: a signal ends the host within a second
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Input::Cmd(request, reply)) => {
                reply_to_command(host, &request, &reply);
            }
            Ok(Input::Subscribe(subscriber)) => host.subscribe(subscriber),
            Ok(Input::Harness(agent, output)) => host.harness(agent, output),
            Ok(Input::Setup(label, ok, out)) => host.setup_done(&label, ok, out),
            Ok(Input::Pull(ok, out)) => host.pull_done(ok, out),
            Ok(Input::Worktree(result)) => host.worktree_done(result, tx.clone()),
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

fn relay_harness(rx: mpsc::Receiver<(String, Out)>, input_tx: &Sender<Input>) {
    for (agent, output) in rx {
        if input_tx.send(Input::Harness(agent, output)).is_err() {
            return;
        }
    }
}

fn reply_to_command(host: &mut Host, request: &Value, reply: &Sender<Value>) {
    let response = match host.cmd(request) {
        Ok(mut reply) => {
            reply["ok"] = json!(true);
            reply
        }
        Err(error) => json!({"ok": false, "error": error}),
    };

    let _ = reply.send(response);

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
    let busy = host.sessions.values().any(|live| live.busy);

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
