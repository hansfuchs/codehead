//! Thread cards: the thread header of the thread view and the threads tab of the F2 menu.
//! Sources: the thread records, the hosts (agents at work), the event logs (last agent event).
//! Unread: an agent event newer than the last time codehead showed the thread (~/.cache/codehead/seen.json).

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub state: String,
    /// Live sessions.
    pub live: usize,
    /// An agent works (session running / starting, or a Claude transcript mid-turn).
    pub busy: bool,
    /// BUSY only by a monitor or background command: `monitoring…`, not `working…`.
    #[serde(default)]
    pub monitoring: bool,
    /// A question (an ask) waits on the owner: blocked, also while its turn runs.
    pub blocked: bool,
    pub unread: bool,
    /// Last activity, unix ms: the last agent item, else the thread's update time.
    pub activity: u64,
    /// Worktree (else project dir) of the thread: the thread view works there.
    pub dir: Option<PathBuf>,
    pub branch: String,
    /// Main checkout of the thread's git repo (repo filter of the threads tab).
    pub repo: Option<PathBuf>,
    /// The prompt holds an unsent message (the thread view's draft on disk).
    pub draft: bool,
    /// Work not in the base yet (open) or 0 left (shipped: ready to archive). None: 0 read yet.
    #[serde(default)]
    pub land: Option<crate::landed::Land>,
    /// Every agent on the thread reported done (0 turn since).
    #[serde(default)]
    pub done: bool,
    /// DIR is a linked worktree codehead made (delete / archive remove it).
    #[serde(default)]
    pub worktree: bool,
}

impl Card {
    pub fn archived(&self) -> bool {
        self.state == "archived"
    }

    /// 0 work left to land: the branch landed / abandoned, or 0 code and every agent done (a new
    /// thread has 0 code too: not shipped).
    pub fn shipped(&self) -> bool {
        use crate::landed::Land;
        match &self.land {
            Some(Land::Landed | Land::Abandoned(_)) => true,
            Some(Land::NoCode) => self.done,
            _ => false,
        }
    }
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

/// Thread ID has an unsent message: its prompt draft (`prompt.json`, written by the thread view
/// after each key, gone once empty) has text, or a send the host has not taken yet.
pub fn has_draft(id: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(crate::engine::store::thread_dir(id).join("prompt.json")) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return false };

    let text = v["text"].as_str().is_some_and(|t| !t.trim().is_empty());
    let unsent = v["unsent"].as_array().is_some_and(|u| !u.is_empty());
    text || unsent
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

/// Every thread (archived ones too: `Card::archived`), newest activity first.
pub fn load() -> Vec<Card> {
    let threads = crate::threads::all_threads_lite();
    let mut seen = read_seen();
    let mut new_seen = false;
    let mut cards: Vec<Card> = threads
        .into_iter()
        .filter(|t| t.state != "deleted")
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
                repo: crate::threads::repo_of_thread(&t),
                busy: t.busy,
                monitoring: t.watching,
                blocked: t.needs.as_deref() == Some("question"),
                unread,
                activity: last.unwrap_or(0).max(t.updated * 1000),
                dir,
                branch: t.branch.clone().unwrap_or_default(),
                draft: has_draft(&t.id),
                land: crate::landed::of(&t.id),
                done: crate::notify::reported_done(&t.id),
                worktree: t.worktree.as_deref().is_some_and(|p| std::path::Path::new(p).is_dir()),
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
    use ratatui::style::Modifier;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
    if r.width < 6 || r.height < CARD_H {
        return;
    }
    let border = if cur { theme::accent_bold() } else { theme::line() };
    let mut block = Block::new().borders(Borders::ALL).border_type(BorderType::Plain).border_style(border).style(theme::text());
    if here {
        block = block.title(Span::styled(" here ", if cur { theme::accent_bold() } else { theme::muted() }));
    }
    let inner = block.inner(r);
    block.render(r, buf);
    let w = inner.width.saturating_sub(2) as usize;
    let title_st = if c.unread || cur { theme::bold() } else { theme::text() };
    let open = c.land.as_ref().filter(|l| l.open());
    let mut status: Vec<Span> = if c.archived() {
        let mut v = vec![Span::styled("▣ archived", theme::muted())];
        if let Some(l) = open {
            v.push(Span::styled(format!(" · ◇ {}", l.badge()), theme::fg(theme::pal().caution())));
        }
        v
    } else if c.blocked {
        vec![Span::styled("? blocked", theme::fg(theme::pal().attention()))]
    } else if c.busy {
        let word = if c.monitoring { "monitoring…" } else { "working…" };
        vec![Span::styled(format!("{} ", WAVE[tick % WAVE.len()]), theme::fg(theme::pal().progress())), Span::styled(word, theme::muted())]
    } else if let Some(l) = open {
        vec![Span::styled(format!("◇ {}", l.badge()), theme::fg(theme::pal().caution()))]
    } else if let Some(l) = c.land.as_ref().filter(|_| c.shipped()) {
        let st = if *l == crate::landed::Land::Landed { theme::fg(theme::pal().success()) } else { theme::muted() };
        vec![Span::styled(format!("⇲ {}", l.badge()), st)]
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
    if cur {
        theme::accent_edge(buf, r);
    }
    if c.unread {
        let dot = ratatui::layout::Rect { x: r.right().saturating_sub(3), y: r.y, width: 1, height: 1 };
        Paragraph::new(Span::styled("●", theme::fg(theme::pal().info()).add_modifier(Modifier::BOLD))).render(dot, buf);
    }

    // an unsent message in the prompt: on the bottom border, right
    let tag = " ✎ unsent ";
    if c.draft && r.width >= tag.chars().count() as u16 + 8 {
        let w = tag.chars().count() as u16;
        let at = ratatui::layout::Rect { x: r.right() - w - 3, y: r.bottom() - 1, width: w, height: 1 };
        Paragraph::new(Span::styled(tag, theme::fg(theme::pal().caution()).add_modifier(Modifier::BOLD))).render(at, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, state: &str, activity: u64) -> Card {
        Card { id: id.into(), title: id.into(), state: state.into(), activity, ..Default::default() }
    }

    #[test]
    fn a_card_with_an_open_question_shows_blocked_also_while_its_turn_runs() {
        let c = Card { busy: true, blocked: true, live: 1, ..card("t", "open", 0) };
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, CARD_H));
        let area = buf.area;
        draw_card(&mut buf, area, &c, false, false, false, 0);
        let text: String = (0..CARD_H).flat_map(|y| (0..40).map(move |x| (x, y))).map(|p| buf[p].symbol().to_string()).collect();
        assert!(text.contains("? blocked") && !text.contains("working"), "{text}");
    }

    /// A monitor or background command runs, 0 turn: `monitoring…`, not `idle` or `working…`.
    #[test]
    fn a_card_with_a_running_monitor_shows_monitoring() {
        let c = Card { busy: true, monitoring: true, live: 1, ..card("t", "open", 0) };
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, CARD_H));
        let area = buf.area;
        draw_card(&mut buf, area, &c, false, false, false, 0);
        let text: String = (0..CARD_H).flat_map(|y| (0..40).map(move |x| (x, y))).map(|p| buf[p].symbol().to_string()).collect();
        assert!(text.contains("monitoring…") && !text.contains("idle") && !text.contains("working"), "{text}");
    }

    /// An unsent message in the thread's prompt: the card says so (bottom border).
    #[test]
    fn a_card_with_an_unsent_prompt_shows_it() {
        let id = "drafttest-0000";
        let dir = crate::engine::store::thread_dir(id);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!has_draft(id));

        crate::view::save_prompt_draft(id, "   ");
        assert!(!has_draft(id), "only spaces: 0 message");

        crate::view::save_prompt_draft(id, "fix the deploy");
        assert!(has_draft(id));

        let c = Card { draft: true, live: 1, ..card(id, "open", 0) };
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, CARD_H));
        let area = buf.area;
        draw_card(&mut buf, area, &c, false, false, false, 0);
        let bottom: String = (0..40).map(|x| buf[(x, CARD_H - 1)].symbol().to_string()).collect();
        assert!(bottom.contains("✎ unsent"), "{bottom}");

        let _ = std::fs::remove_dir_all(&dir);
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
