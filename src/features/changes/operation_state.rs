use super::Receiver;

pub(super) struct OperationState {
    /// The CI fix message on its way; messages for the thread (the view sends them; `@name`
    /// first: to that agent).
    pub(super) ci: Option<Receiver<Result<String, String>>>,
    pub(super) outbox: Vec<String>,
    /// A git step on its way, and what the last one did (the thread view shows it).
    pub(super) op: Option<Receiver<Result<String, String>>>,
    pub(super) note: Option<String>,
    /// The commit message box gets this after the git step on its way ends well (an uncommit:
    /// the messages of the commits taken out).
    pub(super) prefill: Option<String>,
}

impl OperationState {
    pub(super) fn new() -> Self {
        Self {
            ci: None,
            outbox: Vec::new(),
            op: None,
            note: None,
            prefill: None,
        }
    }
}
