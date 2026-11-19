//! `codehead --cost <thread>`: the cost pane of the thread view (bottom right).
//! Scoped to 1 thread: per agent tokens, $, ctx, last-hour sparkline, then the plan limits.
//! Sources: the thread's event log (turn ends), the host's live agents, `limits.json`.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};

use crate::{agents, theme};

const POLL: Duration = Duration::from_secs(2);
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Default, Clone)]
struct AgentRow {
    name: String,
    color: Option<String>,
    tokens: u64,
    usd: f64,
    /// Context fill of the agent's latest session, percent.
    ctx: Option<u64>,
    /// Last hour, (unix time, tokens).
    samples: Vec<(u64, u64)>,
    live: bool,
    /// Saved in the agent prompt, else the stable pool pick (agents::sprite).
    sprite: Option<crate::sprites::Sprite>,
    /// Context tokens of the latest session: the least a next message sends.
    ctx_tokens: u64,
    /// $ per token of the last turn (every kind, cache incl.).
    usd_per_token: f64,
}

type Limit = crate::engine::limits::Window;

#[derive(Default)]
pub struct Snap {
    found: bool,
    status: String,
    cost: f64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    turns: u64,
    agents: Vec<AgentRow>,
    limits: Vec<Limit>,
}



fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Context window of MODEL in tokens: a guess for events without the harness' window.
fn window_of(model: &str) -> u64 {
    if model.contains("[1m]") { 1_000_000 } else { 200_000 }
}

/// Plan limits the hosts saw last: (harness, window, used %, resets at).
pub fn limits() -> Vec<(String, String, f64, Option<u64>)> {
    crate::engine::limits::read().into_iter().map(|w| (w.harness, w.window, w.pct, w.resets)).collect()
}

/// Per agent of thread ID, from its turn ends; live agents from its host.
pub fn uses(id: &str) -> Vec<AgentUse> {
    let (live, _) = if crate::engine::client::alive(id) { crate::engine::client::state(id) } else { Default::default() };
    let mut by: Vec<AgentUse> = Vec::new();
    let mut model: HashMap<String, String> = HashMap::new();
    for e in crate::engine::store::events(id) {
        let Some(name) = e.agent.clone() else { continue };
        let i = match by.iter().position(|a| a.agent == name) {
            Some(i) => i,
            None => {
                // the roster's harness; a removed agent: unknown
                let harness = crate::engine::roster::get(&name).map_or_else(|| "?".into(), |r| r.harness);
                by.push(AgentUse { agent: name.clone(), harness, live: live.contains(&name), ..Default::default() });
                by.len() - 1
            }
        };
        let a = &mut by[i];
        match e.kind {
            crate::engine::store::Kind::Session { model: m, .. } => {
                a.sessions += 1;
                model.insert(name, m);
            }
            crate::engine::store::Kind::TurnEnd { cost, input, output, cache_read, cache_write, ctx, window, .. } => {
                a.input += input;
                a.output += output;
                a.cache_read += cache_read;
                a.cache_write += cache_write;
                a.turns += 1;
                a.usd += cost;
                let all = input + output + cache_read + cache_write;
                if all > 0 {
                    a.usd_per_token = cost / all as f64;
                }
                if ctx > 0 {
                    // the harness' window; older events: a guess from the model name
                    let size = if window > 0 { window } else { window_of(model.get(&name).map_or("", String::as_str)) };
                    a.ctx = Some((ctx, size));
                }
                a.samples.push((e.ts / 1000, input + output));
            }
            _ => {}
        }
    }
    by.retain(|a| a.turns > 0 || a.live);
    by.sort_by(|a, b| b.usd.total_cmp(&a.usd));
    by
}

type Names = HashMap<String, (String, Option<String>, crate::sprites::Sprite)>;

/// The cost box's numbers of thread ID.
pub fn snapshot(id: &str) -> Snap {
    fetch(id, &names())
}

fn fetch(id: &str, names: &Names) -> Snap {
    let Some(t) = crate::engine::store::load(id) else { return Snap::default() };
    let (_, busy) = if crate::engine::client::alive(&t.id) { crate::engine::client::state(&t.id) } else { Default::default() };
    let status = if busy.is_empty() { t.state.clone() } else { format!("working: {}", busy.join(", ")) };
    let hour = now().saturating_sub(3600);
    let mut snap = Snap { found: true, status, ..Default::default() };
    for u in uses(&t.id) {
        snap.cost += u.usd;
        snap.input += u.input;
        snap.output += u.output;
        snap.cache_read += u.cache_read;
        snap.cache_write += u.cache_write;
        snap.turns += u.turns;
        let (name, color, sprite) = names.get(&u.agent).cloned().map_or_else(|| (u.agent.clone(), None, None), |(n, c, sp)| (n, c, Some(sp)));
        snap.agents.push(AgentRow {
            name,
            color,
            sprite,
            tokens: u.input + u.output,
            usd: u.usd,
            ctx: u.ctx.filter(|c| c.1 > 0).map(|(used, size)| used * 100 / size),
            samples: u.samples.into_iter().filter(|(at, _)| *at > hour).collect(),
            live: u.live,
            ctx_tokens: u.ctx.map_or(0, |c| c.0),
            usd_per_token: u.usd_per_token,
        });
    }
    snap.limits = crate::engine::limits::read();
    snap
}

/// 250 → "250", 98049 → "98.0k", 12812803 → "12.8M".
pub fn human(v: u64) -> String {
    match v {
        0 => "·".into(),
        1..1_000 => v.to_string(),
        1_000..1_000_000 => format!("{:.1}k", v as f64 / 1e3),
        _ => format!("{:.1}M", v as f64 / 1e6),
    }
}

/// WIDTH buckets over the last hour; empty bucket = "·".
fn spark(samples: &[(u64, u64)], width: usize, now: u64) -> String {
    if width == 0 {
        return String::new();
    }
    let mut b = vec![0u64; width];
    let start = now.saturating_sub(3600);
    for &(at, tok) in samples {
        if at > start {
            let i = (((at - start) as usize) * width / 3600).min(width - 1);
            b[i] += tok;
        }
    }
    let max = b.iter().copied().max().unwrap_or(0).max(1);
    b.iter().map(|&v| if v == 0 { '·' } else { SPARK[((v * 7) / max) as usize] }).collect()
}

/// Short name of plan window W: `five_hour` → `5h`, `seven_day` → `7d`, `seven_day_opus` → `7d opus`.
fn window(w: &str) -> String {
    match w {
        "five_hour" => "5h".into(),
        "seven_day" | "week" => "7d".into(),
        o => match o.strip_prefix("seven_day_") {
            Some(rest) => format!("7d {}", rest.replace('_', " ")),
            None => o.replace('_', " "),
        },
    }
}

fn until(t: u64, now: u64) -> String {
    let d = t.saturating_sub(now);
    match d {
        0..3600 => format!("{}m", d / 60),
        3600..86400 => format!("{}h", d / 3600),
        _ => format!("{}d", d / 86400),
    }
}

/// The next message of the thread view: its recipients, its length (chars).
#[derive(Clone, Debug, Default)]
pub struct Next {
    pub to: Vec<String>,
    pub chars: usize,
}

/// The next message's least cost: (tokens, $). Each recipient reads its context + the message
/// (1 model call; tools add more). 0 recipient with a known context: None.
fn next_cost(s: &Snap, n: &Next) -> Option<(u64, f64)> {
    let msg = (n.chars as u64).div_ceil(4);
    let rows: Vec<&AgentRow> = s.agents.iter().filter(|a| n.to.contains(&a.name) && a.ctx_tokens > 0).collect();
    if rows.is_empty() {
        return None;
    }
    let tok = rows.iter().map(|a| a.ctx_tokens + msg).sum();
    let usd = rows.iter().map(|a| (a.ctx_tokens + msg) as f64 * a.usd_per_token).sum();
    Some((tok, usd))
}

/// Window L as a bar of WIDTH cells: the used % a solid bg, the share of the next message `▒`,
/// the rest dithered (`░`).
fn bar(pct: f64, next: f64, width: usize) -> Vec<Span<'static>> {
    let col = if pct >= 90.0 { theme::pal().danger } else if pct >= 70.0 { theme::pal().warn } else { theme::pal().muted };
    let cells = |p: f64| ((p.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
    let used = cells(pct).min(width);
    // a share > 0: 1 cell at least (visible)
    let share = if next > 0.0 { cells(next).max(1).min(width - used) } else { 0 };
    let rest = width - used - share;
    vec![
        Span::styled(" ".repeat(used), Style::default().bg(col)),
        Span::styled("▒".repeat(share), theme::accent()),
        Span::styled("░".repeat(rest), theme::line()),
    ]
}

/// The cost box in AREA. NEXT: the next message (its cost line, its share in the window bars).
pub fn draw_in(f: &mut Frame, area: Rect, s: &Snap, age: Duration, next: &Next) {
    draw_at(f, area, s, age, now(), next)
}

fn draw(f: &mut Frame, s: &Snap, age: Duration, now: u64) {
    draw_at(f, f.area(), s, age, now, &Next::default())
}

fn draw_at(f: &mut Frame, area: Rect, s: &Snap, age: Duration, now: u64, next: &Next) {
    f.render_widget(Paragraph::new("").style(theme::text()), area);
    let inner = Rect { x: area.x + 1, width: area.width.saturating_sub(2), ..area };
    let w = inner.width as usize;
    let mut lines = Vec::new();
    let cost = format!("${:.2} thread", s.cost);
    // agent sprites side by side (live first), thread cost right of the 1st row. no title: the numbers speak
    let fit = (w.saturating_sub(cost.len() + 1) + 2) / (crate::sprites::W + 2);
    let mut by_live: Vec<&AgentRow> = s.agents.iter().filter(|a| a.sprite.is_some()).collect();
    by_live.sort_by_key(|a| !a.live);
    // compact (< 12 rows, the thread view's header): 0 sprites, the cost on the column row, 0 footer
    let compact = area.height < 12;
    let shown: Vec<&AgentRow> = if compact { Vec::new() } else { by_live.into_iter().take(fit).collect() };
    let rows = if compact { 0 } else if shown.is_empty() { 1 } else { crate::sprites::H };
    let frames: Vec<[String; crate::sprites::H]> = shown.iter().map(|a| a.sprite.as_ref().expect("filtered above").frame(crate::avatar::Status::Idle)).collect();
    for r in 0..rows {
        let mut spans = Vec::new();
        for (i, a) in shown.iter().enumerate() {
            let dim = if a.live { Style::default() } else { Style::default().add_modifier(ratatui::style::Modifier::DIM) };
            spans.push(Span::styled(format!("{}{}", if i > 0 { "  " } else { "" }, frames[i][r]), dim.fg(agents::color(a.color.as_deref()))));
        }
        if r == 0 {
            let used: usize = spans.iter().map(|sp| sp.width()).sum();
            spans.push(Span::raw(" ".repeat(w.saturating_sub(used + cost.len()))));
            spans.push(Span::styled(cost.clone(), theme::muted()));
        }
        lines.push(Line::from(spans));
    }
    if !s.found {
        if compact {
            lines.push(Line::styled(format!("{cost:>w$}"), theme::muted()));
        }
        lines.push(Line::styled("thread not found (yet)", theme::muted()));
        f.render_widget(Paragraph::new(lines), inner);
        return;
    }
    let name_w = s.agents.iter().map(|a| a.name.chars().count()).max().unwrap_or(3).max(3) + 1;
    // name, tok 7, $ 9, ctx 5, spark rest
    let fixed = name_w + 7 + 9 + 5 + 2;
    let spark_w = w.saturating_sub(fixed);
    let head = format!("{:name_w$}{:>7}{:>9}{:>5}  {}", "", "tok", "$", "ctx", if spark_w >= 9 { "last hour" } else { "" });
    if compact {
        // the thread cost right of the column names
        let room = w.saturating_sub(head.trim_end().chars().count());
        let head = head.trim_end().to_string();
        let tail = if room > cost.len() { format!("{cost:>room$}") } else { String::new() };
        lines.push(Line::styled(format!("{head}{tail}"), theme::muted()));
    } else {
        lines.push(Line::styled(head, theme::muted()));
    }
    for a in &s.agents {
        // live session: bold name
        let name = Style::default().fg(agents::color(a.color.as_deref()));
        let name = if a.live { name.add_modifier(ratatui::style::Modifier::BOLD) } else { name };
        lines.push(Line::from(vec![
            Span::styled(format!("{:name_w$}", a.name), name),
            Span::styled(format!("{:>7}", human(a.tokens)), theme::text()),
            Span::styled(format!("{:>9}", format!("${:.2}", a.usd)), theme::text()),
            Span::styled(format!("{:>5}", a.ctx.map_or("·".into(), |c| format!("{c}%"))), theme::text()),
            Span::raw("  "),
            Span::styled(spark(&a.samples, spark_w, now), theme::accent()),
        ]));
    }
    if s.agents.len() != 1 {
        lines.push(Line::from(vec![
            Span::styled(format!("{:name_w$}", "all"), theme::muted()),
            Span::styled(format!("{:>7}", human(s.input + s.output)), theme::text()),
            Span::styled(format!("{:>9}", format!("${:.2}", s.cost)), theme::text()),
        ]));
    }
    // compact: the room goes to the window bars and the next message
    if !compact {
        lines.push(Line::styled(format!("{} turns · cache {} read, {} write", s.turns, human(s.cache_read), human(s.cache_write)), theme::muted()));
    }
    let cost_next = next_cost(s, next);
    if !s.limits.is_empty() {
        if !compact {
            lines.push(Line::raw(""));
            lines.push(Line::styled("plan limits", theme::muted()));
        }
        // the harness in the label: 2+ harnesses only
        let many = s.limits.iter().any(|l| l.harness != s.limits[0].harness);
        let label = |l: &Limit| if many { format!("{} {}", l.harness, window(&l.window)) } else { window(&l.window) };
        let lw = s.limits.iter().map(|l| label(l).chars().count()).max().unwrap_or(0).max(name_w.saturating_sub(1));
        for l in &s.limits {
            // window reset since the last report: 0 %
            let fresh = l.resets.is_some_and(|r| r <= now);
            let pct = if fresh { 0.0 } else { l.pct };
            let share = match (cost_next, l.usd_per_pct) {
                (Some((_, usd)), Some(rate)) if rate > 0.0 => usd / rate,
                _ => 0.0,
            };
            let tail = match l.resets {
                _ if fresh => " new".to_string(),
                Some(r) => format!(" {:>3.0}% {:>3}", pct, until(r, now)),
                None => format!(" {:>3.0}%", pct),
            };
            let bw = w.saturating_sub(lw + 1 + 9).max(4);
            let mut spans = vec![Span::styled(format!("{:lw$} ", label(l)), theme::muted())];
            spans.extend(bar(pct, share, bw));
            spans.push(Span::styled(tail, theme::muted()));
            lines.push(Line::from(spans));
        }
    }
    // the next message (thread view only): tokens, $, its share of each window
    let mut spans = vec![Span::styled(format!("{:lw$} ", "next", lw = name_w.saturating_sub(1)), theme::muted())];
    match cost_next {
        Some((tok, usd)) => {
            spans.push(Span::styled(format!("≥{} tok", human(tok)), theme::text()));
            spans.push(Span::styled(format!(" ~${usd:.2}"), theme::muted()));
            for l in &s.limits {
                let share = l.usd_per_pct.filter(|r| *r > 0.0).map_or("?".into(), |r| format!("+{:.1}%", usd / r));
                spans.push(Span::styled(format!(" · {} ", window(&l.window)), theme::muted()));
                spans.push(Span::styled(share, theme::accent()));
            }
        }
        None => spans.push(Span::styled("· (0 context yet)", theme::muted())),
    }
    if !next.to.is_empty() {
        lines.push(Line::from(spans));
    }
    if !s.status.is_empty() {
        lines.push(Line::styled(s.status.clone(), theme::muted()));
    }
    f.render_widget(Paragraph::new(lines), inner);
    if area.height > 1 && !compact {
        let foot = Rect { y: area.bottom() - 1, height: 1, ..inner };
        f.render_widget(Paragraph::new(Line::styled(format!("updated {}s ago · q close", age.as_secs()), theme::line())), foot);
    }
}

fn names() -> Names {
    agents::load()
        .into_iter()
        .map(|a| {
            let sp = agents::sprite(&a);
            (a.name.clone(), (a.name, a.color, sp))
        })
        .collect()
}

fn run_loop(term: &mut DefaultTerminal, id: &str) -> std::io::Result<()> {
    let names = names();
    let log = crate::engine::store::events_path(id);
    let stamp_of = || std::fs::metadata(&log).ok().map(|m| (m.len(), m.modified().ok()));
    let mut stamp = stamp_of();
    let mut snap = fetch(id, &names);
    let mut at = Instant::now();
    loop {
        term.draw(|f| draw(f, &snap, at.elapsed(), now()))?;
        if event::poll(Duration::from_millis(500))?
            && let Event::Key(k) = event::read()?
            && k.kind != KeyEventKind::Release
        {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Char('r') => {
                    at = Instant::now() - POLL;
                    stamp = None;
                }
                _ => {}
            }
        }
        // a read only after the log changed; every 30s anyway (the last-hour sparkline moves
        // with the clock, the live agents change)
        if at.elapsed() >= POLL {
            let now_stamp = stamp_of();
            if now_stamp != stamp || at.elapsed() >= Duration::from_secs(30) {
                snap = fetch(id, &names);
                stamp = now_stamp;
                at = Instant::now();
            }
        }
    }
}

pub fn run(id: &str) -> ! {
    let mut term = ratatui::init();
    let r = run_loop(&mut term, id);
    ratatui::restore();
    if let Err(e) = r {
        crate::fail(&format!("cost: {e}"), 1);
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_windows_get_short_names() {
        assert_eq!(window("five_hour"), "5h");
        assert_eq!(window("seven_day"), "7d");
        assert_eq!(window("seven_day_opus"), "7d opus");
        assert_eq!(window("other_window"), "other window");
    }

    #[test]
    fn humanizes() {
        assert_eq!(human(0), "·");
        assert_eq!(human(250), "250");
        assert_eq!(human(98_049), "98.0k");
        assert_eq!(human(12_812_803), "12.8M");
    }

    #[test]
    fn sparks() {
        assert_eq!(spark(&[], 4, 3600), "····");
        assert_eq!(spark(&[(3599, 10), (100, 5)], 4, 3600), "▄··█");
    }

    /// Live check: CODEHEAD_THREAD=<id> cargo test live_fetch -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_fetch() {
        let id = std::env::var("CODEHEAD_THREAD").unwrap();
        let s = fetch(&id, &names());
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 14)).unwrap();
        t.draw(|f| draw(f, &s, Duration::from_secs(0), now())).unwrap();
        let b = t.backend().buffer().clone();
        let text: String = (0..14).map(|y| (0..60).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string() + "\n").collect();
        println!("{text}");
        assert!(s.found);
    }

    #[test]
    fn renders() {
        let now = 1_000_000;
        let s = Snap {
            found: true,
            status: "working · turn 3".into(),
            cost: 28.47,
            input: 250,
            output: 98_049,
            cache_read: 12_812_803,
            cache_write: 534_032,
            turns: 9,
            agents: vec![
                AgentRow { name: "amon".into(), color: Some("orange".into()), tokens: 98_000, usd: 26.87, ctx: Some(19), samples: vec![(now - 60, 500)], live: true, sprite: Some(crate::sprites::default_for("amon")), ctx_tokens: 38_000, usd_per_token: 0.000_01 },
                AgentRow { name: "lilith".into(), tokens: 92, usd: 1.6, ..Default::default() },
            ],
            limits: vec![
                Limit { harness: "claude".into(), window: "five_hour".into(), pct: 13.0, resets: Some(now + 7200), usd_per_pct: Some(2.0) },
                Limit { harness: "claude".into(), window: "seven_day".into(), pct: 95.0, resets: Some(now + 3 * 86_400), usd_per_pct: None },
            ],
        };
        let (w, h) = (56, 16);
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, &s, Duration::from_secs(1), now)).unwrap();
        let b = t.backend().buffer().clone();
        let text: String = (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string() + "\n").collect();
        println!("{text}");
        assert!(text.contains("$28.47 thread"));
        assert!(text.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)), "sprite row");
        assert!(text.contains("$26.87"));
        assert!(text.contains("19%"));
        assert!(text.contains(" 13%  2h") && text.contains(" 95%  3d"), "{text}");
        // compact: the thread view's header, a 400-char message to amon
        let (w, h) = (56, 7);
        let next = Next { to: vec!["amon".into()], chars: 400 };
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw_at(f, f.area(), &s, Duration::from_secs(1), now, &next)).unwrap();
        let b = t.backend().buffer().clone();
        let text: String = (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string() + "\n").collect();
        println!("{text}");
        let first = text.lines().next().unwrap();
        assert!(first.contains("tok") && first.ends_with("$28.47 thread"), "{text}");
        assert!(text.contains("$26.87") && !text.contains("9 turns"), "{text}");
        assert!(!text.contains("q close") && !text.contains("◆"), "{text}");
        // the bars: used part solid (bg), the next message's share ▒, the rest dithered ░
        let row = text.lines().position(|l| l.starts_with(" 5h")).expect("5h row");
        let cells: Vec<_> = (0..w).map(|x| b[(x, row as u16)].clone()).collect();
        assert!(cells.iter().any(|c| c.symbol() == " " && c.bg == theme::pal().muted), "used part");
        assert!(text.lines().nth(row).unwrap().contains('▒') && text.contains('░'), "{text}");
        // the next message: ≥ context + text tokens, $, share per window (0 rate: ?)
        assert!(text.contains("≥38.1k tok ~$0.38 · 5h +0.2% · 7d ?"), "{text}");
    }
}

// ---------- tokens tab of the F2 menu: the token box in detail ----------

/// Per agent of a thread: sessions, token kinds, turns, $, context, last 24h.
#[derive(Clone, Debug, Default)]
pub struct AgentUse {
    pub agent: String,
    pub harness: String,
    pub sessions: u64,
    pub live: bool,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub turns: u64,
    pub usd: f64,
    /// Context of the latest session: (used, size).
    pub ctx: Option<(u64, u64)>,
    /// Last 24h: (unix time, tokens).
    pub samples: Vec<(u64, u64)>,
    /// $ per token of the last turn (every kind, cache incl.).
    pub usd_per_token: f64,
}

/// The tokens tab: the thread in view (None: start screen), every thread's cost, plan limits.
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub agents: Vec<AgentUse>,
    /// Thread ids by cost: (id, $, tokens, turns). The top 8.
    pub threads: Vec<(String, f64, u64, u64)>,
    /// $ and tokens of every thread, and of the last 24h (sessions updated since).
    pub all_usd: f64,
    pub all_tokens: u64,
    pub day_usd: f64,
    pub limits: Vec<(String, String, f64, Option<u64>)>,
}

/// Read the detail of thread ID (None: 0 thread part).
pub fn detail(id: Option<&str>) -> Detail {
    let mut d = Detail::default();
    let since = now().saturating_sub(86_400);
    if let Some(id) = id.filter(|i| !i.is_empty()) {
        d.agents = uses(id);
        for a in &mut d.agents {
            a.samples.retain(|(at, _)| *at > since);
        }
    }
    let mut rows: Vec<(String, f64, u64, u64)> = Vec::new();
    for t in crate::engine::store::all() {
        let (mut usd, mut tok, mut turns) = (0.0, 0, 0);
        for e in crate::engine::store::events(&t.id) {
            if let crate::engine::store::Kind::TurnEnd { cost, input, output, .. } = e.kind {
                usd += cost;
                tok += input + output;
                turns += 1;
                if e.ts / 1000 > since {
                    d.day_usd += cost;
                }
            }
        }
        if turns > 0 {
            rows.push((t.id, usd, tok, turns));
        }
    }
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    d.all_usd = rows.iter().map(|r| r.1).sum();
    d.all_tokens = rows.iter().map(|r| r.2).sum();
    d.threads = rows.into_iter().take(8).collect();
    d.limits = limits();
    d
}

/// BUCKETS buckets of SPAN seconds up to NOW; empty: `·`.
fn spark_over(samples: &[(u64, u64)], buckets: usize, span: u64, now: u64) -> String {
    if buckets == 0 || span == 0 {
        return String::new();
    }
    let mut b = vec![0u64; buckets];
    let start = now.saturating_sub(span);
    for &(at, tok) in samples {
        if at > start {
            let i = (((at - start) as u128 * buckets as u128 / span as u128) as usize).min(buckets - 1);
            b[i] += tok;
        }
    }
    let max = b.iter().copied().max().unwrap_or(0).max(1);
    b.iter().map(|&v| if v == 0 { '·' } else { SPARK[((v * 7) / max) as usize] }).collect()
}

/// Lines of the tokens tab, W columns wide. NAMES: agent → (name, color); TITLES: thread id → title.
pub fn detail_lines(d: &Detail, here: Option<&str>, names: &HashMap<String, (String, Option<String>)>, titles: &HashMap<String, String>, w: usize) -> Vec<Line<'static>> {
    let now = now();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let head = |t: &str| Line::styled(t.to_string(), theme::accent_bold());
    let kv = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<10}"), theme::muted()), Span::styled(v, theme::text())]);
    if let Some(id) = here {
        let title = titles.get(id).cloned().unwrap_or_else(|| "this thread".into());
        lines.push(head(&crate::nav::fit(&title, w)));
        let sum = |f: fn(&AgentUse) -> u64| d.agents.iter().map(f).sum::<u64>();
        let (i, o, cr, cw, turns) = (sum(|a| a.input), sum(|a| a.output), sum(|a| a.cache_read), sum(|a| a.cache_write), sum(|a| a.turns));
        let usd: f64 = d.agents.iter().map(|a| a.usd).sum();
        lines.push(kv("cost", format!("${usd:.2} · {} tokens (in {} · out {})", human(i + o), human(i), human(o))));
        let hit = if i + cr > 0 { cr * 100 / (i + cr) } else { 0 };
        lines.push(kv("cache", format!("read {} · write {} · hit {hit}%", human(cr), human(cw))));
        if turns > 0 {
            lines.push(kv("per turn", format!("{turns} turns · ${:.3} · {} tokens", usd / turns as f64, human((i + o) / turns))));
        }
        let all: Vec<(u64, u64)> = d.agents.iter().flat_map(|a| a.samples.iter().copied()).collect();
        let sw = w.saturating_sub(10).min(48);
        lines.push(Line::from(vec![Span::styled(format!("{:<10}", "last 24h"), theme::muted()), Span::styled(spark_over(&all, sw, 86_400, now), theme::accent())]));
        lines.push(Line::from(vec![Span::styled(format!("{:<10}", "last hour"), theme::muted()), Span::styled(spark_over(&all, sw, 3_600, now), theme::accent())]));
        lines.push(Line::raw(""));
        lines.push(head("agents"));
        let nw = d.agents.iter().map(|a| names.get(&a.agent).map_or(8, |n| n.0.chars().count())).max().unwrap_or(5).max(5) + 1;
        lines.push(Line::styled(format!("{:nw$}{:>7}{:>7}{:>8}{:>8}{:>6}{:>9}{:>6}", "", "in", "out", "cache r", "cache w", "turns", "$", "ctx"), theme::muted()));
        for a in &d.agents {
            let (name, color) = names.get(&a.agent).cloned().unwrap_or_else(|| (a.agent.clone(), None));
            let mut st = Style::default().fg(agents::color(color.as_deref()));
            if a.live {
                st = st.add_modifier(ratatui::style::Modifier::BOLD);
            }
            let ctx = a.ctx.filter(|c| c.1 > 0).map_or("·".into(), |(u, s)| format!("{}%", u * 100 / s));
            lines.push(Line::from(vec![
                Span::styled(format!("{name:nw$}"), st),
                Span::styled(format!("{:>7}{:>7}{:>8}{:>8}{:>6}{:>9}{:>6}", human(a.input), human(a.output), human(a.cache_read), human(a.cache_write), a.turns, format!("${:.2}", a.usd), ctx), theme::text()),
            ]));
            let ctx_line = a.ctx.filter(|c| c.1 > 0).map(|(u, s)| format!(" · ctx {} of {}", human(u), human(s))).unwrap_or_default();
            let live = if a.live { "live" } else { "ended" };
            lines.push(Line::styled(format!("{:nw$}{} · {} session(s) · {live}{ctx_line}", "", a.harness, a.sessions), theme::muted()));
        }
        if d.agents.is_empty() {
            lines.push(Line::styled("0 usage yet", theme::muted()));
        }
        lines.push(Line::raw(""));
    } else {
        lines.push(Line::styled("0 thread in view: every thread below", theme::muted()));
        lines.push(Line::raw(""));
    }
    lines.push(head("all threads"));
    lines.push(kv("total", format!("${:.2} · {} tokens · last 24h ${:.2}", d.all_usd, human(d.all_tokens), d.day_usd)));
    for (id, usd, tok, turns) in &d.threads {
        let title = titles.get(id).cloned().unwrap_or_else(|| id.chars().take(8).collect());
        let mark = if here == Some(id.as_str()) { "› " } else { "  " };
        let st = if here == Some(id.as_str()) { theme::accent() } else { theme::text() };
        let tail = format!("  {} · {turns} turns", human(*tok));
        let room = w.saturating_sub(10 + tail.chars().count() + 2);
        lines.push(Line::from(vec![
            Span::styled(format!("{mark}{:>8}", format!("${usd:.2}")), st),
            Span::styled(format!("  {}", crate::nav::fit(&title, room)), st),
            Span::styled(tail, theme::muted()),
        ]));
    }
    if !d.limits.is_empty() {
        lines.push(Line::raw(""));
        lines.push(head("plan limits"));
        for (h, win, pct, resets) in &d.limits {
            let col = if *pct >= 90.0 { theme::pal().danger } else if *pct >= 70.0 { theme::pal().warn } else { theme::pal().ok };
            let reset = match resets {
                Some(r) if *r <= now => " · new window".to_string(),
                Some(r) => format!(" · resets in {}", until(*r, now)),
                None => String::new(),
            };
            let mut spans = vec![Span::styled(format!("{:<10}", format!("{h} {}", window(win))), theme::muted())];
            spans.extend(bar(*pct, 0.0, 20));
            spans.push(Span::styled(format!(" {pct:.0}%"), Style::default().fg(col)));
            spans.push(Span::styled(reset, theme::muted()));
            lines.push(Line::from(spans));
        }
    }
    lines
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    #[test]
    fn detail_lines_show_thread_agents_threads_and_limits() {
        let d = Detail {
            agents: vec![AgentUse { agent: "pk".into(), harness: "claude".into(), sessions: 2, live: true, input: 1000, output: 500, cache_read: 9000, turns: 3, usd: 1.5, ctx: Some((50, 200)), ..Default::default() }],
            threads: vec![("t1".into(), 1.5, 1500, 3), ("t2".into(), 0.5, 100, 1)],
            all_usd: 2.0,
            all_tokens: 1600,
            day_usd: 1.0,
            limits: vec![("claude".into(), "five_hour".into(), 42.0, None)],
        };
        let names = HashMap::from([("pk".to_string(), ("amon".to_string(), None))]);
        let titles = HashMap::from([("t1".to_string(), "codehead v2".to_string())]);
        let text: Vec<String> = detail_lines(&d, Some("t1"), &names, &titles, 70).iter().map(|l| l.to_string()).collect();
        let all = text.join("\n");
        for want in ["codehead v2", "$1.50 · 1.5k tokens", "hit 90%", "3 turns", "amon", "25%", "2 session(s) · live", "all threads", "› ", "claude 5h", "42%"] {
            assert!(all.contains(want), "{want}: {all}");
        }
        let none: String = detail_lines(&d, None, &names, &titles, 70).iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        assert!(none.contains("0 thread in view") && !none.contains("agents"));
    }
}
