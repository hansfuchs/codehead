use super::{
    CLOCK, DefaultTerminal, Duration, Event, Instant, KeyCode, KeyEventKind, KeyModifiers, MORPH_TIME, draw, fetch,
    names, now,
};

pub(super) fn run_loop(term: &mut DefaultTerminal, id: &str) -> std::io::Result<()> {
    let names = names();

    // the log's changes come from the watcher (0 stat poll); the windows' resets move with the
    // clock: 1 read per CLOCK anyway
    crate::infra::watch::start();
    let topics = crate::infra::bus::subscribe();
    let mut snap = fetch(id, &names);
    let mut at = Instant::now();
    let mut morph = Instant::now();

    loop {
        term.draw(|file| draw(file, &snap, morph.elapsed(), now()))?;

        // the morph animation: frames until it ends; else sleep until a key, a change, the clock
        let wait = if morph.elapsed() < MORPH_TIME {
            Duration::from_millis(33)
        } else {
            CLOCK.saturating_sub(at.elapsed()).max(Duration::from_millis(1))
        };

        let mut read = at.elapsed() >= CLOCK;

        let Some(requested) = read_keys(wait)? else {
            return Ok(());
        };

        read |= requested;

        read |= topics.try_iter().any(
            |thread| matches!(thread, crate::infra::bus::Topic::Thread(tid, crate::infra::bus::Part::Log) if tid == id),
        );

        if read {
            snap = fetch(id, &names);
            at = Instant::now();
            morph = Instant::now();
        }
    }
}

pub(crate) fn run(id: &str) -> ! {
    let mut term = ratatui::init();
    let result = run_loop(&mut term, id);

    ratatui::restore();

    if let Err(error) = result {
        crate::infra::terminal::fail(&format!("cost: {error}"), 1);
    }

    std::process::exit(0)
}

fn read_keys(wait: Duration) -> std::io::Result<Option<bool>> {
    let mut refresh = false;

    for (event, _) in crate::infra::terminal::read_ready(wait).map_err(std::io::Error::other)? {
        match key_action(&event) {
            KeyAction::Quit => return Ok(None),
            KeyAction::Refresh => refresh = true,
            KeyAction::Continue => {}
        }
    }

    Ok(Some(refresh))
}

enum KeyAction {
    Quit,
    Refresh,
    Continue,
}

fn key_action(event: &Event) -> KeyAction {
    let Event::Key(key) = event else {
        return KeyAction::Continue;
    };

    if key.kind == KeyEventKind::Release {
        return KeyAction::Continue;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => KeyAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
        KeyCode::Char('r') => KeyAction::Refresh,
        _ => KeyAction::Continue,
    }
}
