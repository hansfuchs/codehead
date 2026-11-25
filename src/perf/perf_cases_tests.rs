use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::Paragraph;

use super::{bench, bench_within, fixture, ms};
use crate::engine::store::Event;
use crate::features::chat::ask::Picker;
use crate::features::chat::{self, Colors, Fold, Look, Thoughts, Thread};

fn thread(turns: usize) -> Thread {
    let mut thread = Thread::default();

    for event in &fixture::events(turns) {
        thread.push(event);
    }

    thread
}

#[test]
#[ignore = "perf: just perf"]
fn log_parse_2k_events() {
    let lines: String = fixture::events(250)
        .iter()
        .map(|event| serde_json::to_string(event).unwrap() + "\n")
        .collect();

    bench("log parse 2k events", || {
        lines
            .lines()
            .filter_map(|line| serde_json::from_str::<Event>(line).ok())
            .count()
    });
}

#[test]
#[ignore = "perf: just perf"]
fn thread_fold_2k_events() {
    let events = fixture::events(250);

    bench("thread fold 2k events", || {
        let mut thread = Thread::default();

        for event in &events {
            thread.push(event);
        }

        thread
    });
}

#[test]
#[ignore = "perf: just perf"]
fn thread_draw_250_turns() {
    let thread = thread(250);
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };
    let colors = Colors::from([("ada".to_string(), ratatui::style::Color::Cyan)]);

    for look in [Look::Bubbles, Look::Lines] {
        bench(&format!("thread draw 250 turns {look:?}"), || {
            chat::draw(&thread, look, 120, &colors, &[], &fold, &Picker::default())
                .rows
                .len()
        });
    }
}

/// ctrl+f in a long chat: 1 key of the query runs the match over every message (cold: the
/// texts of the items built once).
#[test]
#[ignore = "perf: just perf"]
fn chat_search_1k_turns() {
    let thread = thread(1000);
    let mut search = crate::features::chat::chat_search::ChatSearch::default();

    search.open();
    let mut count = 0u32;

    bench(&format!("chat search {} items (1 key)", thread.items.len()), || {
        count += 1;
        search.ta.set_text(if count.is_multiple_of(2) {
            "deploy fix"
        } else {
            "deploy fi"
        });
        search.refresh(&thread)
    });
}

#[test]
#[ignore = "perf: just perf"]
fn frame_paint_200x60() {
    let lines: Vec<_> = crate::ui::markdown::markdown(&fixture::markdown(20), 200, &[])
        .into_iter()
        .map(|rendered| rendered.line)
        .collect();
    let mut term = Terminal::new(TestBackend::new(200, 60)).unwrap();

    bench("frame paint 200x60", || {
        term.draw(|frame| frame.render_widget(Paragraph::new(lines.clone()), frame.area()))
            .map(|_| ())
    });
}

#[test]
#[ignore = "perf: just perf"]
fn markdown_100_blocks() {
    let md = fixture::markdown(100);

    bench("markdown 100 blocks", || {
        crate::ui::markdown::markdown(&md, 100, &[]).len()
    });
}

#[test]
#[ignore = "perf: just perf"]
fn highlight_rust_200_lines() {
    // 1st call: the syntax set loads; a new text each run: 0 cache hits
    let block: String = fixture::RUST.lines().take(200).collect::<Vec<_>>().join("\n");

    assert_eq!(block.lines().count(), 200);

    let mut count = 0;

    bench("highlight rust 200 lines", || {
        count += 1;
        crate::ui::code::highlight(&format!("{block}\n// {count}"), "rust")
    });
}

/// The 1st code block of a session: the syntax set loads. Valid only as the 1st case that
/// highlights: libtest runs the cases by name, `a_` sorts it 1st.
#[test]
#[ignore = "perf: just perf"]
fn a_syntax_set_load() {
    bench("syntax set load (1st block)", || {
        crate::ui::code::highlight("fn main() {}", "rust")
    });
}

#[test]
#[ignore = "perf: just perf"]
fn diff_parse_50_files() {
    let diff = fixture::diff(50);

    bench("diff parse 50 files", || crate::features::changes::parse(&diff).len());
}

#[test]
#[ignore = "perf: just perf"]
fn word_marks_long_line() {
    let old: Vec<char> = "let value = compute(alpha, beta, gamma); ".repeat(40).chars().collect();
    let new: Vec<char> = "let value = compute(alpha, delta, gamma); "
        .repeat(40)
        .chars()
        .collect();

    bench("word marks 1.6k chars", || {
        crate::features::changes::word_marks(&old, &new)
    });
}

/// The new-thread form's submit → the thread view's 1st frame: the plan, thread.json, the host
/// start, the view (as `main.rs` runs it). 1 case per target kind. The host: the release
/// binary (`just perf` builds it), 0 roster agent (its delivery fails, 0 harness runs).
/// Between runs (untimed): the last host logged its task, made its worktree, pulled main.
#[test]
#[ignore = "perf: just perf"]
fn thread_start_submit_to_1st_frame() {
    super::start::run();
}

/// A copy key (selection, path, code block): the UI thread's part of `clip::copy` with 64 KB.
/// The tool: `cat` (the same spawn, pipe and wait as pbcopy / wl-copy, 0 write to the
/// owner's clipboard). The tool's own run (off the UI thread) prints for reference.
#[test]
#[ignore = "perf: just perf"]
fn clipboard_copy_64k() {
    let text = fixture::RUST.chars().cycle().take(64 << 10).collect::<String>();
    let cat: [(&str, &[&str]); 1] = [("cat", &[])];

    bench("clipboard copy 64 KB", || {
        assert!(crate::infra::clip::copy_with(&cat, &text, false));
    });

    super::measure("clipboard tool run (off UI)", || {
        crate::infra::clip::copy_with(&cat, &text, true)
    });
}

/// The MCP tab's edit form (budget 50 ms per step): its open (`a` + a frame) and its save
/// (ctrl+s + a frame: the file, the agents picker). The secret: a `${VAR}` reference (a
/// keychain value: its write runs off the UI thread, 0 write to the owner's keychain here).
#[test]
#[ignore = "perf: just perf"]
fn mcp_edit_form_open_and_save() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use std::time::Duration;

    const FORM: Duration = Duration::from_millis(50);

    let mut term = Terminal::new(TestBackend::new(160, 50)).unwrap();

    let mut draw_tab = |tab: &mut crate::features::settings::mcp_tab::McpTab| {
        term.draw(|frame| tab.draw(frame, frame.area(), true))
            .map(|_| ())
            .unwrap();
    };

    bench_within("mcp form open", FORM, || {
        let mut tab = crate::features::settings::mcp_tab::McpTab::new();

        mcp_key(&mut tab, KeyCode::Char('a'), KeyModifiers::NONE);
        draw_tab(&mut tab);
    });

    let mut count = 0;

    let worst = super::measure_budget(
        "mcp form save",
        FORM,
        || {
            count += 1;
            mcp_form(count)
        },
        |mut tab| {
            mcp_key(&mut tab, KeyCode::Char('s'), KeyModifiers::CONTROL);
            draw_tab(&mut tab);
            tab
        },
    );

    assert!(worst < FORM, "mcp form save: {:.2} ms ≥ budget 50 ms", ms(worst));

    let saved = crate::engine::servers::load().unwrap();

    assert!(
        saved
            .iter()
            .any(|server| server.name == "perf-1"
                && server.env.get("TOKEN").map(String::as_str) == Some("${PERF_TOKEN}")),
        "{saved:?}"
    );
}

/// Archive from the ctrl+p menu: the yes (enter) + 1 frame, the card gone from the list in
/// that step. The thread: a worktree with 5k ignored files (a `node_modules`); the job (host
/// stop, stash, worktree out) runs on its own thread, its time prints for reference.
#[test]
#[ignore = "perf: just perf"]
fn archive_yes_to_card_gone() {
    super::archive::run();
}

/// The owner's case: an agent commits in its worktree → the card's label. 80 threads, 30
/// worktrees of 1 repo, the real watcher + landed worker. Budget: change → read start and
/// read end → card on screen, each < 20 ms; the git read itself prints for reference.
#[test]
#[ignore = "perf: just perf"]
fn landed_refresh_after_commit() {
    super::landed::run();
}

#[test]
#[ignore = "perf: just perf"]
fn fuzzy_rank_10k_paths() {
    let paths = fixture::paths(10_000);

    bench("fuzzy rank 10k paths", || {
        crate::ui::fuzzy::rank(&paths, "eng mod fil", std::clone::Clone::clone, |_| String::new()).len()
    });
}

fn mcp_key(
    tab: &mut crate::features::settings::mcp_tab::McpTab,
    code: ratatui::crossterm::event::KeyCode,
    modifiers: ratatui::crossterm::event::KeyModifiers,
) {
    tab.key(ratatui::crossterm::event::KeyEvent::new(code, modifiers));
}

fn mcp_text(tab: &mut crate::features::settings::mcp_tab::McpTab, text: &str) {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    for character in text.chars() {
        mcp_key(tab, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

fn mcp_form(count: usize) -> crate::features::settings::mcp_tab::McpTab {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    let mut tab = crate::features::settings::mcp_tab::McpTab::new();

    mcp_key(&mut tab, KeyCode::Char('a'), KeyModifiers::NONE);
    mcp_text(&mut tab, &format!("perf-{count}"));
    mcp_key(&mut tab, KeyCode::Down, KeyModifiers::NONE);
    mcp_key(&mut tab, KeyCode::Down, KeyModifiers::NONE);
    mcp_text(&mut tab, "npx -y perf-server --port 4000");
    mcp_key(&mut tab, KeyCode::Down, KeyModifiers::NONE);
    mcp_text(&mut tab, "TOKEN=${PERF_TOKEN}");

    tab
}
