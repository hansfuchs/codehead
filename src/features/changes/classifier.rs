use super::Receiver;

pub(super) struct Classification {
    /// The agent's order (groups, whys, levels) of the current set of paths; a call on its way;
    /// why 0; the rank holds for the current paths (set by `build`).
    pub(super) rank: Option<crate::domain::changes::rank::Rank>,
    pub(super) ranking: Option<Receiver<Result<crate::domain::changes::rank::Rank, String>>>,
    pub(super) rank_error: Option<String>,
    pub(super) fresh_rank: bool,
    /// The classifier (setting `classify`), None: path rules.
    pub(super) classifier: Option<crate::domain::changes::rank::Runner>,
    /// The level of each file (by index), reviewed / all files per level (by `build`).
    pub(super) lv: Vec<crate::domain::changes::rank::Level>,
    pub(super) counts: [(usize, usize); 4],
}

impl Classification {
    pub(super) fn new(id: &str) -> Self {
        Self {
            rank: crate::domain::changes::rank::cached(id),
            ranking: None,
            rank_error: None,
            fresh_rank: false,
            classifier: if cfg!(test) {
                None
            } else {
                crate::domain::changes::rank::runner()
            },
            lv: Vec::new(),
            counts: [(0, 0); 4],
        }
    }
}
