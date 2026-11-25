use super::Receiver;

pub(super) struct Reviewer {
    /// The reviewer's findings, a review on its way, why 0, the model (None: `off`).
    pub(super) review: Option<crate::features::changes::review::Review>,
    pub(super) reviewing: Option<Receiver<Result<crate::features::changes::review::Review, String>>>,
    pub(super) review_error: Option<String>,
    pub(super) review_model: Option<String>,
}

impl Reviewer {
    pub(super) fn new(id: &str) -> Self {
        Self {
            review: crate::features::changes::review::cached(id),
            reviewing: None,
            review_error: None,
            review_model: if cfg!(test) {
                None
            } else {
                crate::infra::config::review_model()
            },
        }
    }
}
