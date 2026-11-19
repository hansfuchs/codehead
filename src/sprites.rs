//! Agent sprites: animated 7x4 braille avatars (avatar/), 2 categories: regi (the default) and
//! geo. Choice per agent: its profile (agents::Profile), else a stable regi pick by name. New
//! agent: a random regi avatar. ROWS: the rest frame (every eye on, idle), the text an agent's
//! prompt shows; the screens draw the live frame (`frame`, `lines`).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::avatar::{Avatar, Cat, Status};

pub const W: usize = crate::avatar::W;
pub const H: usize = crate::avatar::H;

#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    /// Avatar id: `regi:12 eyes=3 speed=0.5`.
    pub id: String,
    pub avatar: Avatar,
    /// The rest frame.
    pub rows: [String; H],
}

// the speed is never NaN
impl Eq for Sprite {}

impl Sprite {
    pub fn of(avatar: Avatar) -> Sprite {
        Sprite { id: avatar.id(), avatar, rows: avatar.rows(Status::Idle, 0.0) }
    }

    /// The live frame for status S.
    pub fn frame(&self, s: Status) -> [String; H] {
        self.avatar.rows(s, crate::avatar::now())
    }
}

/// The saved id ID. None: not an avatar (an old text sprite: the agent gets its default).
pub fn from_id(id: &str) -> Option<Sprite> {
    Avatar::parse(id).map(Sprite::of)
}

/// FNV-1a: stable across builds (std hashers are not).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Avatar for an agent without a saved one: a stable regi pick.
pub fn default_for(name: &str) -> Sprite {
    let n = Cat::Regi.count() as u64;
    Sprite::of(Avatar { cat: Cat::Regi, idx: (fnv(name) % n) as usize, ..Avatar::default() })
}

/// Random avatar of CAT.
pub fn random(cat: Cat) -> Sprite {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    Sprite::of(Avatar { cat, idx: (fnv(&nanos.to_string()) % cat.count() as u64) as usize, ..Avatar::default() })
}


/// H lines of the live frame (status S) in COLOR, SIDE[i] appended to row i.
pub fn lines_as(s: &Sprite, color: Color, st: Status, side: Vec<Vec<Span<'static>>>) -> Vec<Line<'static>> {
    let mut side = side.into_iter();
    s.frame(st)
        .into_iter()
        .map(|r| {
            let mut spans = vec![Span::styled(r, Style::default().fg(color))];
            spans.extend(side.next().unwrap_or_default());
            Line::from(spans)
        })
        .collect()
}

/// H lines of the live frame, idle.
pub fn lines(s: &Sprite, color: Color, side: Vec<Vec<Span<'static>>>) -> Vec<Line<'static>> {
    lines_as(s, color, Status::Idle, side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn default_is_stable_regi() {
        assert_eq!(default_for("abc"), default_for("abc"));
        for k in ["a", "b", "c", "d"] {
            assert_eq!(default_for(k).avatar.cat, Cat::Regi);
        }
    }

    #[test]
    fn old_ids_are_not_avatars() {
        assert!(from_id("pcb-042").is_none() && from_id("custom").is_none());
        let s = from_id("geo:3 speed=2").unwrap();
        assert_eq!((s.avatar.cat, s.avatar.idx, s.avatar.speed), (Cat::Geo, 3, 2.0));
        assert!(s.rows.iter().all(|r| r.width() == W));
    }
}
