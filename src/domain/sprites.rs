use crate::domain::avatar::{Avatar, Cat, Status};
pub(crate) const WIDTH: usize = crate::domain::avatar::WIDTH;

pub(crate) const HEIGHT: usize = crate::domain::avatar::HEIGHT;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sprite {
    /// Avatar id: `regi:12 eyes=3 speed=0.5`.
    pub id: String,
    pub avatar: Avatar,
    /// The rest frame.
    pub rows: [String; HEIGHT],
}

// the speed is never NaN
impl Eq for Sprite {}

impl Sprite {
    pub(crate) fn of(avatar: Avatar) -> Sprite {
        Sprite {
            id: avatar.id(),
            avatar,
            rows: avatar.rows(Status::Idle, 0.0),
        }
    }

    /// The sprite of agent NAME on a screen with other agents: its float speed ±6 % (stable per
    /// name), so 2 agents never move in sync. The saved id stays.
    pub(crate) fn desynced(mut self, name: &str) -> Sprite {
        let key = (fnv(name) % 1001) as f32 / 1000.0;

        self.avatar.speed *= 0.94 + 0.12 * key;
        self
    }

    /// The live frame for status S.
    pub(crate) fn frame(&self, status: Status) -> [String; HEIGHT] {
        self.avatar.rows(status, crate::domain::avatar::now())
    }
}

/// The saved id ID. None: not an avatar (an old text sprite: the agent gets its default).
pub(crate) fn from_id(id: &str) -> Option<Sprite> {
    Avatar::parse(id).map(Sprite::of)
}

/// FNV-1a: stable across builds (std hashers are not).
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Avatar for an agent without a saved one: a stable regi pick.
pub(crate) fn default_for(name: &str) -> Sprite {
    let count = Cat::Regi.count() as u64;

    Sprite::of(Avatar {
        cat: Cat::Regi,
        idx: (fnv(name) % count) as usize,
        ..Avatar::default()
    })
}

/// Random avatar of CAT.
pub(crate) fn random(cat: Cat) -> Sprite {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);

    Sprite::of(Avatar {
        cat,
        idx: (fnv(&nanos.to_string()) % cat.count() as u64) as usize,
        ..Avatar::default()
    })
}
