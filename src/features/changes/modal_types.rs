use super::Spot;

/// A modal over the changes view.
pub(super) enum Modal {
    /// A comment: on SPOTS (new), an edit, a reply (TARGET); the diff lines it is on (a reply:
    /// the thread too).
    /// LEVEL: its severity; `ON_LEVEL`: the keys go to the severity row (←→), else the text.
    Comment {
        title: String,
        quote: Vec<QLine>,
        scroll: usize,
        target: Target,
        spots: Vec<Spot>,
        text: Box<crate::ui::textarea::TextArea>,
        level: crate::domain::changes::rank::Level,
        on_level: bool,
    },
    /// A yes / no question; THEN runs on yes.
    Confirm { question: String, yes: bool, then: Then },
}

/// A step of the ship section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ship {
    Push,
    /// Push up to the newest picked commit (a question lists the commits first).
    PushTo,
    Update,
    OpenPr,
    /// The lead opens the pull request in a sub-thread (title + body its own): ready / draft.
    AgentPr {
        draft: bool,
    },
    /// The lead brings the open PR's title + body up to date in a sub-thread.
    PrDesc,
    /// The lead rebases the PR's branch on the newest base, resolves the conflicts, pushes.
    PrRebase,
    /// The lead ships the branch in a sub-thread: commit, push, PR, review, CI until green.
    Full,
    Merge,
    CiLog,
    Resolve,
    Continue,
    Abort,
}

/// A section of the ship section: the agent's whole run, git, the pull request, a stopped rebase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShipGroup {
    Ship,
    Git,
    Pr,
    Rebase,
}

impl Ship {
    pub(super) fn group(self) -> ShipGroup {
        match self {
            Ship::Full => ShipGroup::Ship,
            Ship::Push | Ship::PushTo | Ship::Update => ShipGroup::Git,
            Ship::OpenPr | Ship::AgentPr { .. } | Ship::PrDesc | Ship::PrRebase | Ship::Merge | Ship::CiLog => {
                ShipGroup::Pr
            }
            Ship::Resolve | Ship::Continue | Ship::Abort => ShipGroup::Rebase,
        }
    }

    /// The step pushes commits (git or the agent).
    pub(super) fn pushes(self) -> bool {
        matches!(
            self,
            Ship::Push | Ship::PushTo | Ship::OpenPr | Ship::AgentPr { .. } | Ship::PrRebase | Ship::Full
        )
    }

    /// An agent does the step (a sub-thread, a message to the agents).
    pub(super) fn agent(self) -> bool {
        matches!(
            self,
            Ship::Full | Ship::AgentPr { .. } | Ship::PrDesc | Ship::PrRebase | Ship::CiLog | Ship::Resolve
        )
    }
}

/// A row of the ship section: its step, label, state, and whether it asks first (enter enter).
pub(super) struct ShipRow {
    pub(super) step: Ship,
    /// Its section: a rule between 2 sections.
    pub(super) group: ShipGroup,
    /// An agent does it (✦).
    pub(super) agent: bool,
    /// Grayed: 0 run now (a push runs and the step pushes).
    pub(super) off: bool,
    pub(super) label: String,
    pub(super) state: String,
    pub(super) asks: bool,
}

/// A line of the comment modal's selected lines: a file's path, a diff line, a gap between
/// files.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum QLine {
    Path(String),
    Code(String),
    Gap,
    /// A row of a comment's thread (a reply modal): its author row (true), a text row.
    Note(bool, String),
}

/// What the comment modal writes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    /// A new comment on the modal's spots.
    New,
    /// The text, severity of comment ID.
    Edit(String),
    /// The text of reply R of comment ID.
    EditReply(String, usize),
    /// A new reply on comment ID.
    Reply(String),
}

impl Target {
    /// The modal has a severity row.
    pub(super) fn has_level(&self) -> bool {
        matches!(self, Target::New | Target::Edit(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Then {
    DropStash(String),
    ClearStash,
    /// Uncommit these commits (oids); HEAD when the question asked (an agent commit after it: 0 uncommit).
    Uncommit(Vec<String>, String),
    /// Push up to this commit.
    PushTo(String),
}
