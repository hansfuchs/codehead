//! Notifications: what needs the owner on a thread they do not look at.
//!
//! A background poll reads the tail of each open thread's log (only logs that changed) and the
//! remembered PRs of each thread (every 3 min), and keeps the notes that hold now: a resolved
//! state (an answered ask, a blocked agent at work again) drops its note. A note shows while its
//! time is after the thread's seen mark; the thread view moves the mark when the owner has the
//! event on screen for 1 s. The header shows the counts, ctrl+n the list.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::engine::store::{self, Event, Kind as Ev};
use crate::theme;

/// Kinds, most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A question / permission request waits on the owner.
    Ask,
    Blocked,
    /// The flow waits on the owner (a gate, a loop at its max, a role with 0 agent).
    Gate,
    Error,
    /// A usage limit stopped an agent.
    Limit,
    Conflict,
    /// A check of the thread's PR failed.
    Ci,
    /// A comment / review on the thread's PR, not the owner's.
    Review,
    /// An agent wrote `@owner`.
    Mention,
    /// An agent replied on a review comment (changes view), unseen.
    Reply,
    /// An agent's context window ≥ 90 %.
    Context,
    /// Every agent on the thread reported done.
    Done,
}

impl Kind {
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Ask => "?",
            Kind::Blocked => "⏸",
            Kind::Gate => "◆",
            Kind::Error => "✗",
            Kind::Limit => "⧗",
            Kind::Conflict => "⇄",
            Kind::Ci => "✗",
            Kind::Review => "✎",
            Kind::Mention => "@",
            Kind::Reply => "↳",
            Kind::Context => "◔",
            Kind::Done => "✓",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Ask => "question",
            Kind::Blocked => "blocked",
            Kind::Gate => "gate",
            Kind::Error => "error",
            Kind::Limit => "usage limit",
            Kind::Conflict => "conflict",
            Kind::Ci => "ci failed",
            Kind::Review => "review",
            Kind::Mention => "mention",
            Kind::Reply => "reply",
            Kind::Context => "context",
            Kind::Done => "done",
        }
    }

    pub fn color(self) -> Color {
        let p = theme::pal();
        match self {
            Kind::Error | Kind::Conflict | Kind::Ci => p.error(),
            Kind::Ask | Kind::Blocked | Kind::Gate => p.attention(),
            Kind::Limit | Kind::Context => p.caution(),
            Kind::Review | Kind::Mention | Kind::Reply => p.info(),
            Kind::Done => p.success(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub thread: String,
    pub title: String,
    pub dir: PathBuf,
    pub kind: Kind,
    /// The agents it is about (count in the header); empty: the thread (PR notes).
    pub agents: Vec<String>,
    /// Unix ms of the event: the jump target, the seen check.
    pub ts: u64,
    pub text: String,
    /// A reply note: the comment (the jump opens the changes view on it).
    pub comment: String,
}

impl Note {
    /// Agents it counts for (a PR note: 1).
    pub fn count(&self) -> usize {
        self.agents.len().max(1)
    }

    pub fn card(&self) -> crate::nav::Card {
        crate::nav::Card { id: self.thread.clone(), title: self.title.clone(), dir: Some(self.dir.clone()), ..Default::default() }
    }
}

// ---------- derive ----------

/// The notes that hold at the end of EVENTS (the tail of a thread's log): (kind, agents, ts, text).
pub fn derive(events: &[Event], members: &[String]) -> Vec<(Kind, Vec<String>, u64, String)> {
    #[derive(Default)]
    struct Agent {
        /// last `set_status`: (state, note, ts); a turn start after it clears it
        status: Option<(String, String, u64)>,
        asks: Vec<(String, u64, String)>,
        error: Option<(u64, String)>,
        limit: Option<(u64, u64)>,
        ctx: Option<(u64, u64, u64)>,
        mention: Option<(u64, String)>,
    }
    let mut by: HashMap<String, Agent> = HashMap::new();
    let mut gate: Option<(u64, String)> = None;
    for e in events {
        let name = e.agent.clone().unwrap_or_default();
        match &e.kind {
            Ev::Prompt { from, to, text } if from == "owner" => {
                // the owner wrote: errors and mentions before it are answered
                for a in by.values_mut() {
                    a.error = None;
                    a.mention = None;
                }
                let _ = (to, text);
            }
            Ev::Prompt { from, to, text } if to.iter().any(|t| t == "owner") || mentions_owner(text) => {
                by.entry(from.clone()).or_default().mention = Some((e.ts, first_line(text)));
            }
            Ev::Text { text } if mentions_owner(text) => by.entry(name).or_default().mention = Some((e.ts, first_line(text))),
            Ev::Ask { id, input, .. } => {
                let q = input["questions"][0]["question"].as_str().or(input["question"].as_str()).map(first_line).unwrap_or_else(|| "permission request".into());
                by.entry(name).or_default().asks.push((id.clone(), e.ts, q));
            }
            Ev::Answer { id, .. } => {
                for a in by.values_mut() {
                    a.asks.retain(|x| x.0 != *id);
                }
            }
            Ev::TurnStart => {
                let a = by.entry(name).or_default();
                a.status = None;
                a.limit = None;
            }
            Ev::TurnEnd { error, ctx, window, .. } => {
                let a = by.entry(name).or_default();
                a.asks.clear();
                if *window > 0 {
                    a.ctx = Some((e.ts, *ctx, *window));
                }
                match error.as_deref() {
                    Some(m) if crate::stream::usage_limit(m) => {}
                    Some(m) if m != crate::engine::host::INTERRUPTED && m != "session ended" => a.error = Some((e.ts, first_line(m))),
                    _ => a.error = None,
                }
            }
            Ev::Error { message } if !crate::stream::usage_limit(message) => by.entry(name).or_default().error = Some((e.ts, first_line(message))),
            Ev::Resume { at } => by.entry(name).or_default().limit = (*at > 0).then_some((e.ts, *at)),
            Ev::Status { state, note } => by.entry(name).or_default().status = Some((state.clone(), note.clone(), e.ts)),
            Ev::Leave => {
                by.remove(&name);
            }
            Ev::Flow { op, text } => match op.as_str() {
                "wait" => gate = Some((e.ts, text.clone())),
                "pending" | "queued" | "role" => {}
                _ => gate = None,
            },
            _ => {}
        }
    }
    let mut out: Vec<(Kind, Vec<String>, u64, String)> = Vec::new();
    let mut names: Vec<&String> = by.keys().filter(|n| !n.is_empty()).collect();
    names.sort();
    let mut push = |k: Kind, a: &str, ts: u64, text: String| match out.iter_mut().find(|x| x.0 == k) {
        // 1 note per kind per thread: the agents add up, the latest event wins the jump
        Some(n) => {
            n.1.push(a.to_string());
            if ts > n.2 {
                n.2 = ts;
                n.3 = format!("{a}: {text}");
            }
        }
        None => out.push((k, vec![a.to_string()], ts, format!("{a}: {text}"))),
    };
    for n in &names {
        let a = &by[*n];
        if let Some((_, ts, q)) = a.asks.last() {
            push(Kind::Ask, n, *ts, q.clone());
        }
        if let Some((s, note, ts)) = &a.status
            && s == "blocked"
        {
            push(Kind::Blocked, n, *ts, if note.is_empty() { "blocked".into() } else { first_line(note) });
        }
        if let Some((ts, m)) = &a.error {
            push(Kind::Error, n, *ts, m.clone());
        }
        if let Some((ts, at)) = a.limit {
            push(Kind::Limit, n, ts, format!("resumes {}", crate::stream::clock(at)));
        }
        if let Some((ts, m)) = &a.mention {
            push(Kind::Mention, n, *ts, m.clone());
        }
        if let Some((ts, ctx, win)) = a.ctx
            && ctx * 10 >= win * 9
        {
            push(Kind::Context, n, ts, format!("context {} %", ctx * 100 / win));
        }
    }
    if let Some((ts, text)) = gate {
        out.push((Kind::Gate, Vec::new(), ts, text));
    }
    // thread done: every agent on the thread reported done (0 turn since)
    let on: Vec<&String> = members.iter().collect();
    if !on.is_empty() && on.iter().all(|m| by.get(*m).and_then(|a| a.status.as_ref()).is_some_and(|s| s.0 == "done")) {
        let ts = on.iter().filter_map(|m| by[*m].status.as_ref().map(|s| s.2)).max().unwrap_or(0);
        let note = on.iter().filter_map(|m| by[*m].status.as_ref().filter(|s| !s.1.is_empty()).map(|s| first_line(&s.1))).next_back().unwrap_or_else(|| "all agents done".into());
        out.push((Kind::Done, on.iter().map(|s| s.to_string()).collect(), ts, note));
    }
    out.sort_by_key(|x| x.0);
    out
}

fn mentions_owner(text: &str) -> bool {
    text.match_indices("@owner").any(|(i, _)| !text[i + 6..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-'))
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().chars().take(160).collect()
}

// ---------- state ----------

struct Cache {
    /// per thread: (log len, log mtime) → its derived notes, its last event ts
    logs: HashMap<String, ((u64, SystemTime), Vec<Note>, u64)>,
    /// PR notes per thread (the last PR pass)
    prs: HashMap<String, Vec<Note>>,
    /// PR condition key → when codehead saw it first (stable ts across passes and restarts)
    first: HashMap<String, u64>,
    /// per thread: its comments file's time → its reply notes (unseen agent replies)
    replies: HashMap<String, (Option<SystemTime>, Vec<Note>)>,
}

/// The reply notes of thread T: 1 per comment with unseen agent replies (its latest).
fn reply_notes(t: &store::Thread) -> Vec<Note> {
    crate::changes::threads::unseen_replies(&t.id)
        .into_iter()
        .map(|(comment, ts, by, text)| Note { thread: t.id.clone(), title: t.title.clone(), dir: t.work_dir(), kind: Kind::Reply, agents: vec![by], ts, text, comment })
        .collect()
}

static NOTES: Mutex<Vec<Note>> = Mutex::new(Vec::new());
static LAST: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
static SEEN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
/// Up with each change of the notes or the seen marks: the views redraw.
static VERSION: AtomicU64 = AtomicU64::new(0);

fn dir() -> PathBuf {
    crate::engine::data_dir().join("notify")
}

fn load_map(name: &str) -> HashMap<String, u64> {
    std::fs::read(dir().join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_map(name: &str, m: &HashMap<String, u64>) {
    if cfg!(test) {
        return;
    }
    let _ = std::fs::create_dir_all(dir());
    if let Ok(b) = serde_json::to_vec(m) {
        let _ = crate::engine::write_atomic(&dir().join(name), &b);
    }
}

fn seen() -> &'static Mutex<HashMap<String, u64>> {
    SEEN.get_or_init(|| Mutex::new(if cfg!(test) { HashMap::new() } else { load_map("seen.json") }))
}

pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// The owner saw thread THREAD up to TS (unix ms): its notes up to TS go.
pub fn mark_seen(thread: &str, ts: u64) {
    let Ok(mut s) = seen().lock() else { return };
    if s.get(thread).is_some_and(|&x| x >= ts) {
        return;
    }
    s.insert(thread.to_string(), ts);
    save_map("seen.json", &s);
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// The owner saw all of thread THREAD (its stream at the bottom).
pub fn mark_all_seen(thread: &str) {
    let last = LAST.get().and_then(|m| m.lock().ok().and_then(|m| m.get(thread).copied()));
    if let Some(ts) = last {
        mark_seen(thread, ts);
    }
}

/// Unseen notes, most urgent first, then the newest; EXCEPT: the thread in view (0 notes of it).
pub fn unseen(except: Option<&str>) -> Vec<Note> {
    let s = seen().lock().map(|s| s.clone()).unwrap_or_default();
    let mut v: Vec<Note> = NOTES.lock().map(|n| n.clone()).unwrap_or_default();
    // a reply note: unseen until the owner sees the reply in the changes view (0 seen mark)
    v.retain(|n| Some(n.thread.as_str()) != except && (n.kind == Kind::Reply || s.get(&n.thread).is_none_or(|&at| n.ts > at)));
    v.sort_by(|a, b| a.kind.cmp(&b.kind).then(b.ts.cmp(&a.ts)));
    v
}

/// Unseen notes of thread THREAD (the thread view: the marker while scrolled back).
pub fn unseen_of(thread: &str) -> Vec<Note> {
    let s = seen().lock().ok().and_then(|s| s.get(thread).copied()).unwrap_or(0);
    NOTES.lock().map(|n| n.iter().filter(|n| n.thread == thread && n.kind != Kind::Reply && n.ts > s).cloned().collect()).unwrap_or_default()
}

/// Start the background poll (once per process).
pub fn start() {
    static ON: OnceLock<()> = OnceLock::new();
    if cfg!(test) || ON.set(()).is_err() {
        return;
    }
    std::thread::spawn(|| {
        let mut c = Cache { logs: HashMap::new(), prs: HashMap::new(), first: load_map("prs.json"), replies: HashMap::new() };
        let mut pr_at: Option<Instant> = None;
        loop {
            let threads: Vec<store::Thread> = store::all().into_iter().filter(|t| t.state != "archived" && t.state != "deleted").collect();
            let do_prs = pr_at.is_none_or(|t| t.elapsed() > Duration::from_secs(180));
            if do_prs {
                pr_at = Some(Instant::now());
                pr_pass(&threads, &mut c);
            }
            let mut all = Vec::new();
            let mut last = HashMap::new();
            for t in &threads {
                let p = store::events_path(&t.id);
                let Ok(m) = std::fs::metadata(&p) else { continue };
                let key = (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH));
                let fresh = c.logs.get(&t.id).is_none_or(|x| x.0 != key);
                if fresh {
                    let ev = store::events_tail(&t.id, 512 << 10);
                    let end = ev.last().map_or(0, |e| e.ts);
                    let dir = t.work_dir();
                    let notes = derive(&ev, &t.agents).into_iter().map(|(kind, agents, ts, text)| Note { thread: t.id.clone(), title: t.title.clone(), dir: dir.clone(), kind, agents, ts, text, comment: String::new() }).collect();
                    c.logs.insert(t.id.clone(), (key, notes, end));
                }
                // unseen agent replies on comments: read when the comments file changed
                let at = crate::changes::comments_stamp(&t.id);
                if c.replies.get(&t.id).is_none_or(|r| r.0 != at) {
                    c.replies.insert(t.id.clone(), (at, reply_notes(t)));
                }
                all.extend(c.replies[&t.id].1.iter().cloned());
                let (_, notes, end) = &c.logs[&t.id];
                all.extend(notes.iter().cloned());
                all.extend(c.prs.get(&t.id).into_iter().flatten().cloned());
                last.insert(t.id.clone(), *end);
            }
            if let Ok(mut l) = LAST.get_or_init(|| Mutex::new(HashMap::new())).lock() {
                *l = last;
            }
            if let Ok(mut n) = NOTES.lock()
                && *n != all
            {
                *n = all;
                VERSION.fetch_add(1, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// The remembered PRs of each thread (the merge tab found them): conflicts, failed checks, the
/// latest comment of someone else.
fn pr_pass(threads: &[store::Thread], c: &mut Cache) {
    use crate::gh;
    let now = crate::engine::now_ms();
    let mut live: HashSet<String> = HashSet::new();
    c.prs.clear();
    for t in threads {
        let mut notes = Vec::new();
        for r in crate::prlist::remembered(&t.id) {
            let Ok(p) = gh::fetch(&r) else { continue };
            if !matches!(p.state, gh::State::Open | gh::State::Draft) {
                continue;
            }
            let mut add = |kind: Kind, key: String, ts: Option<u64>, text: String| {
                let ts = ts.unwrap_or_else(|| *c.first.entry(key.clone()).or_insert(now));
                live.insert(key);
                notes.push(Note { thread: t.id.clone(), title: t.title.clone(), dir: t.work_dir(), kind, agents: Vec::new(), ts, text, comment: String::new() });
            };
            if p.conflicts() {
                add(Kind::Conflict, format!("conflict:{}", r.key()), None, format!("{}: merge conflict with {}", r.label(), p.base));
            }
            let failed: Vec<&str> = p.checks.iter().filter(|x| x.verdict == gh::Verdict::Fail).map(|x| x.name.as_str()).collect();
            if !failed.is_empty() {
                add(Kind::Ci, format!("ci:{}:{}", r.key(), p.head_oid), None, format!("{}: {} failed", r.label(), failed.join(", ")));
            }
            let latest = p.threads.iter().flat_map(|t| std::iter::once(&t.root).chain(&t.replies)).filter(|m| !m.mine).max_by_key(|m| m.created);
            if let Some(m) = latest {
                add(Kind::Review, String::new(), Some(m.created * 1000), format!("{}: {}: {}", r.label(), m.author, first_line(&m.body)));
            }
        }
        if !notes.is_empty() {
            c.prs.insert(t.id.clone(), notes);
        }
    }
    let before = c.first.len();
    c.first.retain(|k, _| live.contains(k));
    if c.first.len() != before || !live.is_empty() {
        save_map("prs.json", &c.first);
    }
}

// ---------- header ----------

/// Header counts of NOTES: `✓ done 2  ⏸ blocked 1  ctrl+n`; empty: 0 notes.
pub fn badge(notes: &[Note]) -> Vec<Span<'static>> {
    if notes.is_empty() {
        return Vec::new();
    }
    let mut by: Vec<(Kind, usize)> = Vec::new();
    for n in notes {
        match by.iter_mut().find(|x| x.0 == n.kind) {
            Some(x) => x.1 += n.count(),
            None => by.push((n.kind, n.count())),
        }
    }
    by.sort_by_key(|x| x.0);
    let mut s = Vec::new();
    for (k, n) in by {
        s.push(Span::styled(format!("{} {} {n}", k.icon(), k.label()), Style::default().fg(k.color()).add_modifier(Modifier::BOLD)));
        s.push(Span::raw("  "));
    }
    s.push(Span::styled("ctrl+n ", theme::muted()));
    s
}

pub fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()) as u16).sum()
}

/// The note of the highest kind of thread ID, on the top border of its card R (the threads tab).
pub fn card_badge(buf: &mut ratatui::buffer::Buffer, r: Rect, notes: &[Note], id: &str) {
    use ratatui::widgets::Widget;
    let Some(n) = notes.iter().filter(|n| n.thread == id).min_by_key(|n| n.kind) else { return };
    let more = notes.iter().filter(|x| x.thread == id).count() - 1;
    let text = if more > 0 { format!(" {} {} +{more} ", n.kind.icon(), n.kind.label()) } else { format!(" {} {} ", n.kind.icon(), n.kind.label()) };
    let w = unicode_width::UnicodeWidthStr::width(text.as_str()) as u16;
    if r.width < w + 8 {
        return;
    }
    let at = Rect { x: r.right() - w - 5, y: r.y, width: w, height: 1 };
    Paragraph::new(Span::styled(text, Style::default().fg(n.kind.color()).add_modifier(Modifier::BOLD))).render(at, buf);
}

// ---------- the dropdown (ctrl+n) ----------

pub fn is_key(k: &KeyEvent) -> bool {
    k.code == KeyCode::Char('n') && k.modifiers == KeyModifiers::CONTROL
}

#[derive(Default)]
pub struct Panel {
    pub notes: Vec<Note>,
    cursor: usize,
    rows: Vec<(Rect, usize)>,
}

pub enum Pick {
    /// Keep the panel open.
    Stay,
    Close,
    /// Enter: jump to the note's event in its thread.
    Jump(Note),
}

impl Panel {
    pub fn open(except: Option<&str>) -> Panel {
        Panel { notes: unseen(except), ..Default::default() }
    }

    pub fn key(&mut self, k: KeyEvent) -> Pick {
        let n = self.notes.len();
        match k.code {
            KeyCode::Esc => Pick::Close,
            _ if is_key(&k) => Pick::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                Pick::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(n.saturating_sub(1));
                Pick::Stay
            }
            KeyCode::Enter => self.notes.get(self.cursor).cloned().map_or(Pick::Close, Pick::Jump),
            // x: mark seen without a jump
            KeyCode::Char('x') if n > 0 => {
                let note = self.notes.remove(self.cursor.min(n - 1));
                mark_seen(&note.thread, note.ts);
                self.cursor = self.cursor.min(self.notes.len().saturating_sub(1));
                if self.notes.is_empty() { Pick::Close } else { Pick::Stay }
            }
            _ => Pick::Stay,
        }
    }

    /// A click at AT: the note's jump.
    pub fn click(&mut self, at: ratatui::layout::Position) -> Pick {
        match self.rows.iter().find(|(r, _)| r.contains(at)) {
            Some(&(_, i)) => self.notes.get(i).cloned().map_or(Pick::Close, Pick::Jump),
            None => Pick::Close,
        }
    }

    /// The panel at the top right of AREA, below its 1st row.
    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(4).min(76);
        let rows = (self.notes.len().max(1) as u16 * 2).min(area.height.saturating_sub(5));
        let r = Rect { x: area.right().saturating_sub(w + 1), y: area.y + 1, width: w, height: rows + 3 };
        f.render_widget(Clear, r);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(format!(" notifications {} ", self.notes.len()), theme::accent_bold()))
            .title_bottom(Span::styled(" ↑↓ move · enter jump · x seen · esc close ", theme::muted()))
            .style(theme::text());
        let inner = block.inner(r);
        f.render_widget(block, r);
        self.rows.clear();
        if self.notes.is_empty() {
            f.render_widget(Paragraph::new(Span::styled(" 0 notifications", theme::muted())), Rect { height: 1, ..inner });
            return;
        }
        let fits = (inner.height / 2).max(1) as usize;
        self.cursor = self.cursor.min(self.notes.len() - 1);
        let top = self.cursor.saturating_sub(fits - 1);
        let tw = inner.width.saturating_sub(2) as usize;
        for (slot, (i, n)) in self.notes.iter().enumerate().skip(top).take(fits).enumerate() {
            let y = inner.y + slot as u16 * 2;
            let row = Rect { x: inner.x, y, width: inner.width, height: 2 };
            let cur = i == self.cursor;
            if cur {
                f.render_widget(Paragraph::new("").style(Style::default().bg(theme::pal().surface0)), row);
            }
            let when = crate::nav::ago(n.ts);
            let head = format!("{} {} ", n.kind.icon(), n.kind.label());
            let room = tw.saturating_sub(head.chars().count() + when.chars().count() + 3);
            let l1 = Line::from(vec![
                Span::styled(format!(" {head}"), Style::default().fg(n.kind.color()).add_modifier(Modifier::BOLD)),
                Span::styled(crate::nav::fit(&n.title, room), if cur { theme::bold() } else { theme::text() }),
                Span::styled(format!(" · {when}"), theme::muted()),
            ]);
            let l2 = Line::from(Span::styled(format!("   {}", crate::nav::fit(&n.text, tw.saturating_sub(2))), theme::muted()));
            f.render_widget(Paragraph::new(vec![l1, l2]), row);
            self.rows.push((row, i));
        }
    }
}

/// The thread view's seen tracker: the newest event ts on screen counts once it stayed 1 s.
#[derive(Default)]
pub struct Watch {
    pending: Option<(u64, Instant)>,
}

impl Watch {
    /// THREAD shows events up to TS now (None: the bottom, every event). True: a mark moved.
    pub fn on_screen(&mut self, thread: &str, ts: Option<u64>) {
        let ts = ts.unwrap_or(u64::MAX);
        match self.pending {
            Some((p, at)) if p == ts => {
                if at.elapsed() >= Duration::from_secs(1) {
                    if ts == u64::MAX { mark_all_seen(thread) } else { mark_seen(thread, ts) }
                    self.pending = Some((p, at));
                }
            }
            _ => self.pending = Some((ts, Instant::now())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(agent: &str, ts: u64, kind: Ev) -> Event {
        Event { ts, agent: (!agent.is_empty()).then(|| agent.to_string()), kind }
    }

    fn status(a: &str, ts: u64, s: &str) -> Event {
        ev(a, ts, Ev::Status { state: s.into(), note: String::new() })
    }

    fn kinds(v: &[(Kind, Vec<String>, u64, String)]) -> Vec<Kind> {
        v.iter().map(|x| x.0).collect()
    }

    #[test]
    fn done_only_when_every_agent_is_done() {
        let m = vec!["ada".to_string(), "bob".to_string()];
        let mut e = vec![ev("ada", 1, Ev::Join), ev("bob", 1, Ev::Join), status("ada", 2, "done")];
        assert!(!kinds(&derive(&e, &m)).contains(&Kind::Done));
        e.push(status("bob", 3, "done"));
        let d = derive(&e, &m);
        assert_eq!(kinds(&d), vec![Kind::Done]);
        assert_eq!((d[0].1.len(), d[0].2), (2, 3));
        e.push(ev("ada", 4, Ev::TurnStart));
        assert!(derive(&e, &m).is_empty(), "a turn after done: working again");
    }

    #[test]
    fn blocked_and_asks_drop_once_resolved() {
        let m = vec!["ada".to_string()];
        let mut e = vec![status("ada", 1, "blocked"), ev("ada", 2, Ev::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"question": "which db?"}) })];
        assert_eq!(kinds(&derive(&e, &m)), vec![Kind::Ask, Kind::Blocked]);
        e.push(ev("", 3, Ev::Answer { id: "q".into(), allow: true, label: "pg".into() }));
        e.push(ev("ada", 4, Ev::TurnStart));
        assert!(derive(&e, &m).is_empty());
    }

    #[test]
    fn limit_gate_context_mention() {
        let m = vec!["ada".to_string()];
        let end = |ctx| Ev::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx, window: 100, ms: 0, error: None };
        let e = vec![
            ev("ada", 1, end(95)),
            ev("ada", 2, Ev::Resume { at: 9_000 }),
            ev("", 3, Ev::Flow { op: "wait".into(), text: "⏸ gate plan approval".into() }),
            ev("ada", 4, Ev::Text { text: "@owner please check the plan".into() }),
        ];
        assert_eq!(kinds(&derive(&e, &m)), vec![Kind::Gate, Kind::Limit, Kind::Mention, Kind::Context]);
        let mut e2 = e.clone();
        e2.push(ev("", 5, Ev::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "ok".into() }));
        e2.push(ev("", 6, Ev::Flow { op: "answer".into(), text: String::new() }));
        e2.push(ev("ada", 7, Ev::TurnStart));
        e2.push(ev("ada", 8, end(10)));
        assert!(derive(&e2, &m).is_empty());
        assert!(!mentions_owner("@owners"));
    }

    #[test]
    fn errors_skip_usage_limits_and_clear_on_owner_message() {
        let m = vec!["ada".to_string()];
        let mut e = vec![ev("ada", 1, Ev::Error { message: "boom".into() })];
        assert_eq!(kinds(&derive(&e, &m)), vec![Kind::Error]);
        e.push(ev("", 2, Ev::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "retry".into() }));
        assert!(derive(&e, &m).is_empty());
    }

    #[test]
    fn panel_moves_and_jumps() {
        let n = |t: &str| Note { thread: t.into(), title: t.into(), dir: PathBuf::new(), kind: Kind::Done, agents: vec![], ts: 1, text: String::new(), comment: String::new() };
        let mut p = Panel { notes: vec![n("a"), n("b")], ..Default::default() };
        assert!(matches!(p.key(KeyEvent::from(KeyCode::Down)), Pick::Stay));
        assert!(matches!(p.key(KeyEvent::from(KeyCode::Enter)), Pick::Jump(x) if x.thread == "b"));
        assert!(matches!(p.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)), Pick::Close));
    }

    #[test]
    fn reply_notes_hold_past_the_seen_mark_until_the_reply_is_seen() {
        let mut t = store::Thread::new("replies", "x", std::path::Path::new("/tmp"), vec!["REV".into()]);
        t.id = format!("notify-replies-{}", std::process::id());
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        crate::changes::agent_comments(&t.id, "REV", None, &serde_json::json!([{"file": "a.rs", "line": 3, "severity": "low", "text": "nit"}])).unwrap();
        assert!(reply_notes(&t).is_empty(), "a new comment: 0 reply note");
        crate::changes::threads::reply(&t.id, "REV", "c1", "done in a.rs:4", false).unwrap();

        let notes = reply_notes(&t);
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].kind, notes[0].comment.as_str(), notes[0].agents.clone(), notes[0].text.as_str()), (Kind::Reply, "c1", vec!["REV".to_string()], "c1 a.rs: done in a.rs:4"));

        // the chat's seen mark past it: the reply note stays (0 chat row), out of the chat's ↓ count
        let done = Note { kind: Kind::Done, comment: String::new(), ..notes[0].clone() };
        *NOTES.lock().unwrap() = vec![notes[0].clone(), done];
        mark_seen(&t.id, notes[0].ts + 1);
        let kinds: Vec<Kind> = unseen(None).into_iter().filter(|n| n.thread == t.id).map(|n| n.kind).collect();
        assert_eq!(kinds, vec![Kind::Reply]);
        assert!(unseen_of(&t.id).is_empty());

        // seen in the changes view: 0 note
        assert!(crate::changes::threads::mark_seen(&t.id, "c1"));
        assert!(reply_notes(&t).is_empty());
        NOTES.lock().unwrap().clear();
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }
}
