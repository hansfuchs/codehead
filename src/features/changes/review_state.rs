use super::{Act, View};

impl View {
    /// V: the reviewer reads the diff (on a thread; minutes for a big diff).
    pub(super) fn start_review(&mut self) -> Act {
        start_review_view(self)
    }

    pub(super) fn poll_review(&mut self) -> bool {
        let Some(rx) = &self.reviewer.reviewing else {
            return false;
        };

        let Ok(result) = rx.try_recv() else { return false };

        self.reviewer.reviewing = None;

        match result {
            Ok(result) => {
                self.operation_state.note = Some(format!("review: {} findings", result.findings.len()));
                self.reviewer.review = Some(result);
            }
            Err(error) => self.reviewer.review_error = Some(error),
        }

        true
    }

    /// Drop finding I (dismissed or taken into the batch).
    pub(super) fn drop_finding(&mut self, i: usize) {
        if let Some(result) = self.reviewer.review.as_mut()
            && i < result.findings.len()
        {
            result.findings.remove(i);
            crate::features::changes::review::store(&self.id, result);
        }
    }
}

fn start_review_view(view: &mut View) -> Act {
    let Some(model) = view.reviewer.review_model.clone() else {
        return Act::Toast("review: off (setting review)".into());
    };

    if view.reviewer.reviewing.is_some() {
        return Act::Toast("review: on its way".into());
    }

    let (tx, rx) = crate::infra::bus::channel();
    let (id, dir, files) = (view.id.clone(), view.dir.clone(), view.changes.files.clone());

    std::thread::spawn(move || {
        let _ = tx.send(crate::features::changes::review::run(&id, &dir, &files, &model));
    });

    view.reviewer.reviewing = Some(rx);
    view.reviewer.review_error = None;
    view.layout.built = None;

    Act::Toast("review: on its way".into())
}
