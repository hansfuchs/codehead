use super::*;

pub(crate) fn pr() -> Pr {
    Pr {
        pr: PrRef::new("acme/platform", 12),
        title: "Add X".into(),
        body: "the body".into(),
        url: "https://github.com/acme/platform/pull/12".into(),
        author: "me".into(),
        viewer: "me".into(),
        base: "main".into(),
        head: "feat/x".into(),
        mergeable: "MERGEABLE".into(),
        merge_state: "BEHIND".into(),
        review: "APPROVED".into(),
        can_edit: true,
        method: "SQUASH".into(),
        auto_allowed: true,
        labels: vec![gh::Label {
            name: "bug".into(),
            color: "d73a4a".into(),
        }],
        checks: fixture_checks(),
        threads: fixture_threads(),
        ..Default::default()
    }
}

pub(crate) fn with(path: Pr) -> Merge {
    let mut message = Merge::one(path.pr.clone());
    let key = path.pr.clone();

    message.tabs.entry(key).or_default().pr = Some(path);
    message
}

pub(super) fn key(message: &mut Merge, character: KeyCode) -> Act {
    message.key(KeyEvent::new(character, KeyModifiers::NONE))
}

pub(super) fn focus(message: &Merge) -> Option<Stop> {
    message.tab_ref().and_then(|thread| thread.focus)
}

pub(super) fn draw(message: &mut Merge, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut term = ratatui::Terminal::new(backend).unwrap();

    term.draw(|file| message.draw(file, file.area(), true)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..height)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn buffer(message: &mut Merge, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    term.draw(|file| message.draw(file, file.area(), true)).unwrap();
    term.backend().buffer().clone()
}

/// The bg of the 1st cell of TEXT on screen.
pub(super) fn bg_of(buf: &ratatui::buffer::Buffer, text: &str) -> Color {
    let agent = buf.area;

    for y in 0..agent.height {
        let row: Vec<String> = (0..agent.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        let line = row.concat();

        if let Some(i) = line.find(text) {
            let x = line[..i].chars().count() as u16;

            return buf[(x, y)].bg;
        }
    }

    panic!("{text} not on screen");
}

/// A gh result as the background thread sends it.
pub(super) fn done(message: &mut Merge, op: Op) {
    message.poll(false);
    let _ = message.tx.send(Res::Op(pr().pr, op, Ok(())));

    message.poll(false);
}

fn fixture_checks() -> Vec<gh::Check> {
    vec![
        gh::Check {
            name: "lint".into(),
            group: "CI".into(),
            verdict: Verdict::Fail,
            url: "u1".into(),
            required: true,
            secs: 12,
            description: String::new(),
        },
        gh::Check {
            name: "test".into(),
            group: "CI".into(),
            verdict: Verdict::Pass,
            url: "u2".into(),
            required: false,
            secs: 27,
            description: String::new(),
        },
        gh::Check {
            name: "deploy".into(),
            group: "CD".into(),
            verdict: Verdict::Skip,
            url: "u3".into(),
            required: false,
            secs: 0,
            description: String::new(),
        },
    ]
}

fn fixture_threads() -> Vec<gh::Thread> {
    vec![
        gh::Thread {
            root: gh::Comment {
                kind: Kind::Issue,
                id: 1,
                author: "rev".into(),
                body: "looks off".into(),
                mine: false,
                ..Default::default()
            },
            ..Default::default()
        },
        gh::Thread {
            root: gh::Comment {
                kind: Kind::Line,
                id: 2,
                author: "rev".into(),
                body: "why?".into(),
                ..Default::default()
            },
            replies: vec![gh::Comment {
                kind: Kind::Line,
                id: 3,
                author: "me".into(),
                body: "because".into(),
                mine: true,
                ..Default::default()
            }],
            path: Some("src/x.rs".into()),
            line: Some(4),
            ..Default::default()
        },
    ]
}
