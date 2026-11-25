use super::{Event, Kind, Member, Path, PathBuf, SEEDED, STYLE, Write, io, json, store};

pub(super) fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=codehead",
            "-c",
            "user.email=tutorial@codehead.local",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("git: {error}"))?;

    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The ACP harness a scripted agent runs as: the member's when it speaks ACP, else opencode.
pub(super) fn acp_harness(member: &str) -> String {
    match crate::engine::harnesses::get(member) {
        Some(harness) if matches!(harness.drive, crate::engine::harnesses::Drive::Acp(_)) => member.to_string(),
        _ => "opencode".into(),
    }
}

pub(super) fn turn_end() -> Kind {
    Kind::TurnEnd {
        cost: 0.0,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        ctx: 0,
        window: 0,
        ms: 0,
        error: None,
    }
}

/// The 2nd thread: a finished fix of the readme by the builder, in the log as if it ran.
pub(super) fn seed(repo: &Path, builder: &str) -> Result<String, String> {
    let mut thread = store::Thread::new(SEEDED, "fix the typo in README.md", repo, vec![builder.to_string()]);

    thread.state = "done".into();
    thread.save()?;
    let readme = repo.join("README.md").display().to_string();

    for event in [
        Event::new(
            None,
            Kind::Prompt {
                from: "owner".into(),
                to: vec![builder.to_string()],
                text: "fix the typo in README.md".into(),
            },
        ),
        Event::new(Some(builder), Kind::Text { text: "on it.".into() }),
        Event::new(
            Some(builder),
            Kind::Tool {
                id: "s1".into(),
                name: "Edit".into(),
                input: json!({"file_path": readme}),
            },
        ),
        Event::new(
            Some(builder),
            Kind::ToolResult {
                id: "s1".into(),
                ok: true,
                output: "ok".into(),
            },
        ),
        Event::new(
            Some(builder),
            Kind::Text {
                text: "fixed: \"teh crew\" → \"the crew\" in README.md.".into(),
            },
        ),
        Event::new(Some(builder), turn_end()),
    ] {
        store::append(&thread.id, &event)?;
    }

    Ok(thread.id)
}

/// Make the sandbox at ROOT (its env set already): settings with the owner's look, the crew
/// as scripted agents, 2 shortcuts, a git repo with hello.md + README.md, a home repo (AGENTS.md
/// that lists the repo, notes), the style guide to attach, the seeded thread, the thread.
/// (thread, seeded thread, repo).
pub(super) fn sandbox(
    root: &Path,
    look: &crate::infra::config::File,
    crew: &[Member],
) -> Result<(String, String, PathBuf), String> {
    configure_tutorial(look, crew)?;
    install_crew(crew)?;

    let by = |role: &str| {
        crew.iter()
            .find(|member| member.role == role)
            .map(|member| member.name.clone())
            .unwrap_or_default()
    };

    // git in the sandbox (HOME: the sandbox's): an identity, 0 signing. The changes view's
    // commit runs plain git: 0 identity → "Author identity unknown", 0 commit
    std::fs::write(root.join("home/.gitconfig"), "[user]\n\tname = you\n\temail = you@tutorial.codehead.local\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n").map_err(|error| error.to_string())?;
    crate::domain::shortcuts::save(&[
        crate::domain::shortcuts::Shortcut {
            name: "review".into(),
            text: "review the change in hello.md against the style guide. send each nit to the agent who wrote it."
                .into(),
        },
        crate::domain::shortcuts::Shortcut {
            name: "polish".into(),
            text: "add a 2nd line: welcome aboard.".into(),
        },
    ])?;
    let repo = root.join("hello");

    std::fs::create_dir_all(&repo).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("paste")).map_err(|error| error.to_string())?;
    std::fs::write(style_path(root), STYLE).map_err(|error| error.to_string())?;

    // the home repo: the new tab's repo list and notes come from it
    let home = root.join("home-repo");

    std::fs::create_dir_all(home.join("notes")).map_err(|error| error.to_string())?;
    std::fs::write(home.join("AGENTS.md"), format!("# home\n\nsteering for every agent: read me first.\n\n## Other repos\n\n- hello: the tutorial's repo. `{}`\n", repo.display())).map_err(|error| error.to_string())?;
    std::fs::write(
        home.join("notes/greetings.md"),
        "# greetings\n\nhow the crew greets: lower case, short.\n",
    )
    .map_err(|error| error.to_string())?;
    git(&home, &["init", "-q", "-b", "main"])?;
    git(&home, &["add", "."])?;
    git(&home, &["commit", "-q", "-m", "home"])?;
    std::fs::write(repo.join("hello.md"), "hello\n").map_err(|error| error.to_string())?;
    std::fs::write(repo.join("README.md"), "# hello\n\nteh crew says hi.\n").map_err(|error| error.to_string())?;
    git(&repo, &["init", "-q", "-b", "main"])?;
    git(&repo, &["add", "."])?;
    git(&repo, &["commit", "-q", "-m", "hello"])?;

    // the mock PR's reviewer
    let _ = std::fs::write(repo.join(".git/codehead-tutorial-reviewer"), by("reviewer"));

    // the seeded thread's fix: in the work tree (its changes tab shows it)
    std::fs::write(repo.join("README.md"), "# hello\n\nthe crew says hi.\n").map_err(|error| error.to_string())?;
    let seeded = seed(&repo, &by("builder"))?;
    let names: Vec<String> = crew.iter().map(|member| member.name.clone()).collect();
    let thread = store::Thread::new("tutorial", "", &repo, names);

    thread.save()?;

    Ok((thread.id, seeded, repo))
}

/// The style guide the 1st prompt attaches.
pub(super) fn style_path(root: &Path) -> PathBuf {
    root.join("paste/pasted-text-1.txt")
}

/// The sandbox of the tutorial process PID.
pub(super) fn sandbox_root(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("codehead-tutorial-{pid}"))
}

/// The host sockets of the tutorial process PID: short (a socket path holds max. 104 bytes).
pub(super) fn run_dir(pid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/codehead-tutorial-{pid}"))
}

/// Sandboxes of tutorials whose process is gone (a kill, a crash): their hosts stopped, the
/// dirs deleted.
pub(super) fn sweep() {
    for base in [std::env::temp_dir(), PathBuf::from("/tmp")] {
        sweep_dir(&base);
    }
}

fn sweep_dir(base: &Path) {
    let Ok(rd) = std::fs::read_dir(base) else {
        return;
    };

    for entry in rd.flatten() {
        sweep_entry(&entry);
    }
}

fn sweep_entry(entry: &std::fs::DirEntry) {
    let name = entry.file_name().to_string_lossy().into_owned();

    let Some(pid) = name
        .strip_prefix("codehead-tutorial-")
        .and_then(|suffix| suffix.parse::<i32>().ok())
    else {
        return;
    };

    // signal 0: the process is there (EPERM: there, another user's)
    let alive = unsafe { libc::kill(pid, 0) } == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);

    if alive {
        return;
    }

    // hosts of a gone sandbox: they run on, idle, until stopped
    if let Ok(sockets) = std::fs::read_dir(entry.path()) {
        for socket in sockets.flatten() {
            stop_sandbox_host(socket.path());
        }
    }

    let _ = std::fs::remove_dir_all(entry.path());
}

fn configure_tutorial(look: &crate::infra::config::File, crew: &[Member]) -> Result<(), String> {
    let cfg = crate::infra::config::File {
        scheme: look.scheme.clone(),
        colors: look.colors.clone(),
        messages: look.messages.clone(),
        thoughts: look.thoughts.clone(),
        menu_key: look.menu_key.clone(),
        command_key: look.command_key.clone(),
        agent: crew.first().map(|member| member.name.clone()),
        // the review buttons of the changes view: 0 model calls
        review: Some("off".into()),
        rank: Some("off".into()),
        classify: Some("off".into()),
        ..Default::default()
    };

    crate::infra::config::save_file(&crate::infra::config::path(), "", &cfg)?;
    crate::infra::config::apply(cfg);

    Ok(())
}

fn install_crew(crew: &[Member]) -> Result<(), String> {
    let exe = crate::engine::exe();

    let by = |role: &str| {
        crew.iter()
            .find(|member| member.role == role)
            .map(|member| member.name.clone())
            .unwrap_or_default()
    };

    for member in crew {
        let avatar = crate::domain::avatar::Avatar::parse(&member.avatar).unwrap_or_default();
        let agent = crate::engine::roster::Agent {
            name: member.name.clone(),
            harness: acp_harness(&member.harness),
            mode: "autopilot".into(),
            color: Some(member.color.clone()),
            sprite: Some(crate::engine::roster::SpriteRec {
                id: avatar.id(),
                rows: avatar.rows(crate::domain::avatar::Status::Idle, 0.0).to_vec(),
            }),
            command: Some(format!(
                "'{}' tutor-agent role={} builder={} reviewer={}",
                exe.display(),
                member.role,
                by("builder"),
                by("reviewer")
            )),
            ..Default::default()
        };

        crate::engine::roster::save(&agent)?;
    }

    Ok(())
}

fn stop_sandbox_host(path: PathBuf) {
    if let Ok(stream) = std::os::unix::net::UnixStream::connect(path) {
        let mut writer = stream;
        let _ = writeln!(writer, "{}", json!({"op": "stop"}));
    }
}
