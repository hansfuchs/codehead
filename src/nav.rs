//! Thread cards: the thread header of the thread view and the threads tab of the F2 menu.
//! Sources: the thread records, the hosts (agents at work), the event logs (last agent event).
//! Unread: an agent event newer than the last time codehead showed the thread (~/.cache/codehead/seen.json).

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub state: String,
    /// Live sessions.
    pub live: usize,
    /// An agent works (session running / starting, or a Claude transcript mid-turn).
    pub busy: bool,
    pub unread: bool,
    /// Last activity, unix ms: the last agent item, else the thread's update time.
    pub activity: u64,
    /// Worktree (else project dir) of the thread: the thread view works there.
    pub dir: Option<PathBuf>,
    pub branch: String,
}

impl Card {
    /// 1st line of the title, "untitled" for none.
    pub fn name(&self) -> String {
        let t = self.title.lines().next().unwrap_or_default().trim();
        if t.is_empty() { "untitled".into() } else { t.to_string() }
    }
}

fn seen_path() -> PathBuf {
    crate::config::home().join(".cache/codehead/seen.json")
}

fn read_seen() -> HashMap<String, u64> {
    std::fs::read_to_string(seen_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_seen(seen: &HashMap<String, u64>) {
    let p = seen_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string(seen) {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Thread ID shown now: its agent items so far are read.
pub fn mark_seen(id: &str) {
    mark_seen_at(id, false)
}

/// The view ends: its thread is read up to now, 0 throttle.
pub fn mark_seen_now(id: &str) {
    mark_seen_at(id, true)
}

fn mark_seen_at(id: &str, force: bool) {
    if id.is_empty() {
        return;
    }
    let mut seen = read_seen();
    let now = now_ms();
    // the thread view marks every 3s: 1 write per 15s is enough (its last mark comes at its end)
    if !force && seen.get(id).is_some_and(|t| now.saturating_sub(*t) < 15_000) {
        return;
    }
    seen.insert(id.to_string(), now);
    write_seen(&seen);
}

/// Time of the last agent event of thread ID (its text, error, question), unix ms. Reads
/// only the tail of the log.
fn last_item(id: &str) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(crate::engine::store::events_path(id)).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(64 << 10))).ok()?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).ok()?;
    buf.lines().rev().filter_map(|l| serde_json::from_str::<crate::engine::store::Event>(l).ok()).find_map(|e| {
        use crate::engine::store::Kind;
        matches!(e.kind, Kind::Text { .. } | Kind::Error { .. } | Kind::Ask { .. }).then_some(e.ts)
    })
}

/// Every thread (archived ones: 0), newest activity first.
pub fn load() -> Vec<Card> {
    let threads = crate::threads::all_threads();
    let mut seen = read_seen();
    let mut new_seen = false;
    let mut cards: Vec<Card> = threads
        .into_iter()
        .filter(|t| t.state != "archived" && t.state != "deleted")
        .map(|t| {
            let last = last_item(&t.id);
            let unread = match (last, seen.get(&t.id)) {
                (Some(at), Some(s)) => at > *s,
                // a thread codehead never showed: its items so far count as read
                (Some(at), None) => {
                    seen.insert(t.id.clone(), at);
                    new_seen = true;
                    false
                }
                (None, _) => false,
            };
            let dir = [t.worktree.clone(), t.dir.clone()].into_iter().flatten().map(PathBuf::from).find(|p| p.is_dir());
            Card {
                busy: t.live > 0 && !crate::engine::client::state(&t.id).1.is_empty(),
                unread,
                activity: last.unwrap_or(0).max(t.updated * 1000),
                dir,
                branch: t.branch.clone().unwrap_or_default(),
                id: t.id,
                title: t.title,
                state: t.state,
                live: t.live,
            }
        })
        .collect();
    if new_seen {
        write_seen(&seen);
    }
    cards.sort_by_key(|c| std::cmp::Reverse(c.activity));
    cards
}


/// Fuzzy filter (title, branch) of CARDS: best match first; empty Q: all, order kept.
pub fn filter(cards: &[Card], q: &str) -> Vec<usize> {
    let q = q.trim();
    if q.is_empty() {
        return (0..cards.len()).collect();
    }
    let mut hits: Vec<(i64, usize)> = cards
        .iter()
        .enumerate()
        .filter_map(|(i, c)| crate::app::fuzzy(q, &format!("{} {}", c.name(), c.branch)).map(|s| (s, i)))
        .collect();
    hits.sort_by_key(|(s, i)| (std::cmp::Reverse(*s), *i));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// "3m ago"-style age of unix ms AT.
pub fn ago(at: u64) -> String {
    if at == 0 {
        return String::new();
    }
    crate::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_millis(at))
}

/// Working wave frames (as the agent header), 1 per WAVE_MS.
pub const WAVE: [&str; 4] = ["░▒▓", "▒▓▒", "▓▒░", "▒░▒"];
pub const WAVE_MS: u128 = 150;

/// Rows of a card: border, title, status, border.
pub const CARD_H: u16 = 4;

/// S cut to W display columns (… at the cut).
pub fn fit(s: &str, w: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if unicode_width::UnicodeWidthStr::width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}


pub fn draw_card(buf: &mut ratatui::buffer::Buffer, r: ratatui::layout::Rect, c: &Card, cur: bool, here: bool, age: bool, tick: usize) {
    use crate::theme;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
    if r.width < 6 || r.height < CARD_H {
        return;
    }
    let border = if cur { theme::accent_bold() } else { theme::line() };
    let mut block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(border).style(theme::text());
    if here {
        block = block.title(Span::styled(" here ", if cur { theme::accent_bold() } else { theme::muted() }));
    }
    let inner = block.inner(r);
    block.render(r, buf);
    let w = inner.width.saturating_sub(2) as usize;
    let title_st = if c.unread || cur { theme::bold() } else { theme::text() };
    let mut status: Vec<Span> = if c.busy {
        vec![Span::styled(format!("{} ", WAVE[tick % WAVE.len()]), theme::accent()), Span::styled("working…", theme::muted())]
    } else if c.live == 0 {
        vec![Span::styled("◦ ended", theme::muted())]
    } else {
        vec![Span::styled("◦ idle", theme::muted())]
    };
    if age {
        let used: usize = status.iter().map(|s| s.width()).sum();
        let tail = [ago(c.activity), c.branch.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        if !tail.is_empty() && used + 3 < w {
            status.push(Span::styled(fit(&format!(" · {tail}"), w - used), theme::muted()));
        }
    }
    let lines = vec![Line::from(Span::styled(fit(&c.name(), w), title_st)), Line::from(status)];
    let body = ratatui::layout::Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    Paragraph::new(lines).render(body, buf);
    if c.unread {
        let dot = ratatui::layout::Rect { x: r.right().saturating_sub(3), y: r.y, width: 1, height: 1 };
        Paragraph::new(Span::styled("●", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))).render(dot, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, state: &str, activity: u64) -> Card {
        Card { id: id.into(), title: id.into(), state: state.into(), activity, ..Default::default() }
    }

    #[test]
    fn filter_matches_title_and_branch() {
        let mut cards = vec![card("x", "open", 1), card("y", "open", 2)];
        cards[0].title = "fix the deploy".into();
        cards[1].branch = "task/menu".into();
        assert_eq!(filter(&cards, ""), [0, 1]);
        assert_eq!(filter(&cards, "deploy"), [0]);
        assert_eq!(filter(&cards, "menu"), [1]);
    }
}
