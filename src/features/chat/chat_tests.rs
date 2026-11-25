use super::*;
use serde_json::json;

/// The drawn rows on a test screen: (text per row, fg per cell).
fn cells(directory: &Drawn, width: u16) -> Vec<(String, Vec<Option<Color>>)> {
    use ratatui::widgets::Paragraph;
    let height = directory.rows.len() as u16;
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height.max(1))).unwrap();

    term.draw(|file| {
        file.render_widget(
            Paragraph::new(
                directory
                    .rows
                    .iter()
                    .map(|result| result.line.clone())
                    .collect::<Vec<_>>(),
            ),
            file.area(),
        );
    })
    .unwrap();

    let bytes = term.backend().buffer().clone();

    (0..height)
        .map(|y| {
            (
                (0..width).map(|x| bytes[(x, y)].symbol().to_string()).collect(),
                (0..width)
                    .map(|x| Some(bytes[(x, y)].fg).filter(|color| *color != Color::Reset))
                    .collect(),
            )
        })
        .collect()
}

/// The fg of each cell of the 1st `@NAME` after row AFTER (0 match: panic).
fn mention_fg(rows: &[(String, Vec<Option<Color>>)], name: &str, after: usize) -> Vec<Option<Color>> {
    let at = format!("@{name}");

    for (text, fg) in &rows[after..] {
        if let Some(byte) = text.find(&at) {
            let x = text[..byte].chars().count();

            return fg[x..x + at.chars().count()].to_vec();
        }
    }

    panic!("0 {at}: {:?}", rows.iter().map(|result| &result.0).collect::<Vec<_>>());
}

fn ev(agent: Option<&str>, kind: Kind) -> Event {
    Event {
        ts: 1_000_000,
        agent: agent.map(String::from),
        kind,
    }
}

/// An existing file NAME in a temp dir of test TAG (attachments link only existing files).
fn attached_file(tag: &str, name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("codehead-attached-{tag}-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    let path_value = dir.join(name);

    std::fs::write(&path_value, b"x").unwrap();
    path_value
}

fn text(directory: &Drawn) -> String {
    directory
        .rows
        .iter()
        .map(|result| {
            result
                .line
                .spans
                .iter()
                .map(|text_value| text_value.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
                + "\n"
        })
        .collect()
}

#[path = "chat_attachments_tests.rs"]
mod attachments;
#[path = "chat_boxes_tests.rs"]
mod boxes;
#[path = "chat_bubbles_tests.rs"]
mod bubbles;
#[path = "chat_colors_tests.rs"]
mod colors;
#[path = "chat_events_tests.rs"]
mod events;
#[path = "chat_folds_tests.rs"]
mod folds;
#[path = "chat_memo_tests.rs"]
mod memo;
#[path = "chat_owners_tests.rs"]
mod owners;
#[path = "chat_subthreads_tests.rs"]
mod subthreads;
#[path = "chat_thoughts_tests.rs"]
mod thoughts;
