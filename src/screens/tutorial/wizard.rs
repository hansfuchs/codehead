//! The setup wizard: full screen, 1 decision per screen, the concepts shown, not explained.
//!
//! 0 harnesses (only when 0 is ready, the dry run always) · 1 home · 2 crew · 3 try it (the
//! tutorial) · 4 next steps.
//!
//! `codehead setup --dry`: every screen, 0 writes; on exit the list of what a real run does. The
//! tutorial plays a scripted thread (the real one runs on ephemeral sub-threads, not built yet).
//! A run without `--dry`: the same screens, then the same list (the writes come with the home
//! sync and sub-threads).
//!
//! keys: enter next · esc back, always (1st screen: quit) · ctrl+c quit · each screen's own in
//! its footer. 0 modes: text (path, name, prompt) takes typing and paste at once, so esc never
//! only closes an input.

use std::io;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::domain::avatar::{self, Avatar, Cat, Spawn, Status};
use crate::engine::harnesses;
use crate::ui::theme;

/// Width of the wizard's column.
const COL: u16 = 68;
/// Names a new agent can get.
const NAMES: [&str; 24] = [
    "ADA", "BYTE", "CRUX", "DELTA", "ECHO", "FLUX", "HEX", "IRIS", "JUNO", "KILO", "LUX", "MOTH", "NOVA", "ORBI",
    "PIXL", "QUILL", "RUNE", "SABLE", "TALON", "VOLT", "WREN", "XENO", "YUKI", "ZED",
];
/// Harnesses the harness screen lists when not installed.
const OFFER: [&str; 5] = ["claude", "codex", "opencode", "cursor", "amp"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Screen {
    Harness,
    Home,
    Crew,
    Try,
    Next,
}

const SCREENS: [Screen; 5] = [Screen::Harness, Screen::Home, Screen::Crew, Screen::Try, Screen::Next];

#[derive(Clone, Copy, PartialEq, Debug)]
enum HomeChoice {
    New,
    Clone,
    Existing,
    Skip,
}

const HOME_CHOICES: [(HomeChoice, &str, &str); 4] = [
    (HomeChoice::New, "new", "fresh repo + private GitHub repo"),
    (HomeChoice::Clone, "clone", "2nd device: your crew comes along"),
    (HomeChoice::Existing, "existing", "a git repo you have"),
    (HomeChoice::Skip, "skip", "local only, 0 sync"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Builder,
    Reviewer,
    Planner,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Builder => "builder",
            Role::Reviewer => "reviewer",
            Role::Planner => "planner",
        }
    }
}

const PRESETS: [(&str, &[Role]); 3] = [
    ("solo", &[Role::Builder]),
    ("pair", &[Role::Builder, Role::Reviewer]),
    ("crew", &[Role::Planner, Role::Builder, Role::Reviewer]),
];

/// An agent of the crew screen.
struct Member {
    name: String,
    role: Role,
    harness: &'static str,
    avatar: Avatar,
    color: &'static str,
    spawn: Spawn,
}

/// Key K on line TEXT (the cursor at the end): a char, backspace, alt+backspace (the last word),
/// ctrl+u (all). true: taken.
fn edit(text: &mut String, key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Char('u') if ctrl => text.clear(),
        KeyCode::Backspace if alt => {
            let end = text.trim_end().len();
            let cut = text[..end].rfind([' ', '/']).map_or(0, |i| i + 1);

            text.truncate(cut);
        }
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char(typed) if !ctrl && !alt => text.push(typed),
        _ => return false,
    }

    true
}

pub(crate) struct Wizard {
    dry: bool,
    screen: usize,
    seed: u64,
    /// The tutorial runs next (its own process, the loop in `run` starts it).
    launch: bool,
    /// Screens before this one: esc goes back to the last.
    history: Vec<usize>,
    quit: bool,
    /// Screen entered at: the title decode.
    entered: Instant,

    // harness screen
    harness_state: HarnessState,
    // home screen
    home_state: HomeState,
    // crew screen
    crew_state: CrewState,
}

impl Wizard {
    pub(crate) fn new(dry: bool) -> Wizard {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(7, |elapsed| elapsed.as_nanos() as u64);
        let mut wizard = Wizard {
            dry,
            screen: 0,
            seed,

            launch: false,
            history: Vec::new(),
            quit: false,
            entered: Instant::now(),

            harness_state: HarnessState {
                installed: Vec::new(),
                hcursor: 0,
                note: String::new(),
            },
            home_state: HomeState {
                home: 0,
                paths: ["~/codehead-home".into(), String::new(), String::new(), String::new()],
            },
            crew_state: CrewState {
                preset: 1,
                crew: Vec::new(),
                focus: 0,
            },
        };

        wizard.detect();

        // a real run skips the harness screen when 1 harness is ready
        if !dry && !wizard.harness_state.installed.is_empty() {
            wizard.screen = 1;
        }

        wizard.build_crew();

        wizard
    }

    fn rand(&mut self) -> u64 {
        // xorshift: 0 crate for a few picks
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn detect(&mut self) {
        self.harness_state.installed = harnesses::installed().iter().map(|harness| harness.name).collect();
    }

    /// Harness rows: installed ones, then the offered ones not installed.
    fn harness_rows(&self) -> Vec<&'static harnesses::Harness> {
        let mut rows: Vec<&'static harnesses::Harness> = self
            .harness_state
            .installed
            .iter()
            .filter_map(|name| harnesses::get(name))
            .collect();

        rows.extend(
            OFFER
                .iter()
                .filter(|name| !self.harness_state.installed.contains(name))
                .filter_map(|name| harnesses::get(name)),
        );
        rows
    }

    fn random_avatar(&mut self) -> Avatar {
        let cat = if self.rand().is_multiple_of(3) {
            Cat::Geo
        } else {
            Cat::Regi
        };

        let idx = (self.rand() % cat.count() as u64) as usize;

        Avatar {
            cat,
            idx,
            ..Avatar::default()
        }
    }

    fn spawn(&self) -> Spawn {
        let taken: Vec<usize> = self.crew_state.crew.iter().map(|member| member.spawn.anim).collect();

        Spawn::random_except(&taken)
    }

    /// The crew of the preset: names, avatars and colors new, harnesses from the installed ones
    /// (the reviewer on a 2nd harness when there is one).
    fn build_crew(&mut self) {
        rebuild_crew(self);
    }

    /// Forward to screen TO (esc comes back here).
    fn go(&mut self, to: usize) {
        let to = to.min(SCREENS.len() - 1);

        if to != self.screen {
            self.history.push(self.screen);
        }

        // a fresh tutorial each time: the crew may be new
        self.launch = SCREENS[to] == Screen::Try;
        self.show(to);
    }

    /// Back to the screen before (0 before: quit).
    fn back(&mut self) {
        match self.history.pop() {
            Some(previous) => self.show(previous),
            None => self.quit = true,
        }
    }

    fn show(&mut self, to: usize) {
        select_setup_screen(self, to);
    }

    // ---------- tutorial ----------

    fn lead(&self) -> String {
        self.crew_state.crew[0].name.clone()
    }

    /// The tutorial's spec: the crew, as `codehead tutorial` reads it.
    fn spec(&self) -> String {
        let crew = self
            .crew_state
            .crew
            .iter()
            .map(|member| crate::screens::tutorial::Member {
                name: member.name.clone(),
                role: member.role.name().into(),
                harness: member.harness.into(),
                avatar: member.avatar.id(),
                color: member.color.into(),
            })
            .collect();

        serde_json::to_string(&crate::screens::tutorial::Spec { dry: self.dry, crew }).unwrap_or_default()
    }

    /// The tutorial ended with exit CODE: next screen, back, or quit.
    fn tutorial_ended(&mut self, code: Option<i32>) {
        match code {
            Some(crate::screens::tutorial::DONE) => self.go(self.screen + 1),
            Some(crate::screens::tutorial::QUIT) => self.quit = true,
            Some(crate::screens::tutorial::BACK) => self.back(),
            other => self.harness_state.note = format!("tutorial: exit {other:?}"),
        }
    }

    // ---------- keys ----------

    /// Pasted TEXT (1st line) into the text of the screen: the home path, the agent's name, the
    /// tutorial prompt.
    fn paste(&mut self, text: &str) {
        let line = text.lines().next().unwrap_or_default().trim();

        match SCREENS[self.screen] {
            Screen::Home if HOME_CHOICES[self.home_state.home].0 != HomeChoice::Skip => {
                self.home_state.paths[self.home_state.home].push_str(line);
            }
            Screen::Crew => {
                let typed = format!("{}{line}", self.crew_state.crew[self.crew_state.focus].name);

                self.crew_state.crew[self.crew_state.focus].name = clean_name(&typed);
            }
            _ => {}
        }
    }

    /// Why the crew can't go on: an empty or a doubled name. None: fine.
    fn crew_problem(&self) -> Option<String> {
        validate_crew(self)
    }
}

/// S as an agent name while it is typed: upper case, letters, digits, `-`, `_`, a letter 1st,
/// max. 32 (`roster::valid_name` once not empty).
fn clean_name(text: &str) -> String {
    let mut out: String = text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
        .map(|character| character.to_ascii_uppercase())
        .collect();

    while out.chars().next().is_some_and(|first| !first.is_ascii_alphabetic()) {
        out.remove(0);
    }

    out.truncate(32);
    out
}

/// `codehead setup [--dry]`.
pub(crate) fn run(dry: bool) -> ! {
    crate::infra::terminal::restore_on_panic();
    let mut out = io::stdout();
    let mut tty = crate::ui::terminal::Tty { enhanced: false };

    if let Err(error) = tty.enter(&mut out) {
        crate::infra::terminal::fail(&format!("terminal: {error}"), 1);
    }

    let mut term = match ratatui::Terminal::new(crate::ui::frames::backend()) {
        Ok(terminal) => terminal,
        Err(error) => {
            tty.leave(&mut out);
            crate::infra::terminal::fail(&format!("terminal: {error}"), 1)
        }
    };

    let mut wizard = Wizard::new(dry);
    let result = run_setup(&mut wizard, &mut tty, &mut out, &mut term);

    finish_setup(dry, out, tty, &(wizard), result)
}

fn finish_setup(
    dry: bool,
    mut out: io::Stdout,
    mut tty: crate::ui::terminal::Tty,
    wizard: &Wizard,
    result: Result<(), io::Error>,
) -> ! {
    tty.leave(&mut out);

    if let Err(error) = result {
        crate::infra::terminal::fail(&format!("terminal: {error}"), 1);
    }

    println!(
        "{}",
        if dry {
            "dry run: 0 changes. a real run would:"
        } else {
            "setup: the writes are not built yet. a real run would:"
        }
    );

    for line in wizard.plan() {
        println!("- {line}");
    }

    std::process::exit(0)
}

#[cfg(test)]
#[path = "tutorial_wizard_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tutorial_wizard_look_tests.rs"]
mod look;

#[path = "wizard_draw.rs"]
mod draw;
#[path = "wizard_draw_helpers.rs"]
mod draw_helpers;
#[path = "wizard_key.rs"]
mod key;

#[path = "wizard_details.rs"]
mod details;

struct HarnessState {
    installed: Vec<&'static str>,
    hcursor: usize,
    note: String,
}

struct HomeState {
    home: usize,
    /// The path / URL of each home choice (skip: unused).
    paths: [String; 4],
}

struct CrewState {
    preset: usize,
    crew: Vec<Member>,
    focus: usize,
}

fn rebuild_crew(wizard: &mut Wizard) {
    let roles = PRESETS[wizard.crew_state.preset].1;
    let first = wizard.harness_state.installed.first().copied().unwrap_or("claude");
    let second = wizard.harness_state.installed.get(1).copied().unwrap_or(first);
    let taken: Vec<String> = crate::engine::roster::all()
        .into_iter()
        .map(|agent| agent.name)
        .collect();
    let mut names: Vec<&str> = NAMES
        .iter()
        .copied()
        .filter(|name| !taken.iter().any(|agent| agent == name))
        .collect();

    wizard.crew_state.crew.clear();

    for (i, role) in roles.iter().enumerate() {
        let pick = (wizard.rand() % names.len().max(1) as u64) as usize;

        let name = if names.is_empty() {
            format!("AGENT{i}")
        } else {
            names.remove(pick).to_string()
        };

        let avatar = wizard.random_avatar();
        let color = crate::domain::agents::COLORS[(i + wizard.crew_state.preset) % 5];
        let harness = if *role == Role::Reviewer { second } else { first };
        let spawn = wizard.spawn();

        wizard.crew_state.crew.push(Member {
            name,
            role: *role,
            harness,
            avatar,
            color,
            spawn,
        });
    }

    wizard.crew_state.focus = wizard.crew_state.focus.min(wizard.crew_state.crew.len() - 1);
}

fn select_setup_screen(wizard: &mut Wizard, to: usize) {
    wizard.screen = to;
    wizard.entered = Instant::now();
    wizard.harness_state.note.clear();

    if SCREENS[wizard.screen] == Screen::Crew {
        for i in 0..wizard.crew_state.crew.len() {
            wizard.crew_state.crew[i].spawn = Spawn {
                anim: wizard.crew_state.crew[i].spawn.anim,
                start: avatar::now() + i as f32 * 0.25,
            };
        }
    }
}

fn validate_crew(wizard: &Wizard) -> Option<String> {
    for (i, member) in wizard.crew_state.crew.iter().enumerate() {
        if member.name.is_empty() {
            return Some(format!("the {} needs a name", member.role.name()));
        }

        if wizard.crew_state.crew[..i]
            .iter()
            .any(|earlier| earlier.name == member.name)
        {
            return Some(format!("2 agents named {}", member.name));
        }
    }

    None
}

fn run_setup(
    wizard: &mut Wizard,
    tty: &mut crate::ui::terminal::Tty,
    out: &mut io::Stdout,
    term: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<crate::ui::frames::Frames>>,
) -> io::Result<()> {
    while !wizard.quit {
        // the tutorial: its own process takes the terminal (a sandbox env of its own)
        if wizard.launch {
            launch_tutorial(wizard, tty, out, term)?;
            continue;
        }

        term.draw(|frame| wizard.draw(frame))?;

        read_setup_events(wizard)?;
    }

    Ok(())
}

fn launch_tutorial(
    wizard: &mut Wizard,
    tty: &mut crate::ui::terminal::Tty,
    out: &mut io::Stdout,
    term: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<crate::ui::frames::Frames>>,
) -> io::Result<()> {
    wizard.launch = false;
    tty.release(out);
    let child = std::process::Command::new(crate::engine::exe())
        .arg("tutorial")
        .arg(wizard.spec())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let (code, err) = match child {
        Ok(process) => match process.wait_with_output() {
            Ok(output) => (
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ),
            Err(error) => (None, error.to_string()),
        },
        Err(error) => (None, error.to_string()),
    };

    tty.enter(out)?;
    term.clear()?;

    if err.is_empty() {
        wizard.tutorial_ended(code);
    } else {
        wizard.harness_state.note = err.lines().last().unwrap_or_default().to_string();
    }

    Ok(())
}

fn read_setup_events(wizard: &mut Wizard) -> io::Result<()> {
    // read_ready, not crossterm: a hung-up terminal ends the wizard
    for (ev, _) in crate::infra::terminal::read_ready(Duration::from_millis(33)).map_err(io::Error::other)? {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => wizard.key(key),
            Event::Paste(text) => wizard.paste(&text),
            _ => {}
        }

        if wizard.quit {
            break;
        }
    }

    Ok(())
}
