use super::{STATUS_FOR, ThreadList};

impl ThreadList {
    /// Reads, a job's end, the status's end. SHOWN: the list shows (reads every 2s).
    pub(crate) fn poll(&mut self, shown: bool) {
        poll_thread_list(self, shown);
    }
}

fn poll_body(character: Vec<crate::ui::thread_card::Card>, view_state: &mut ThreadList) {
    view_state.card_reads.loading = None;

    // the 1st read after the job's end: the store holds what the job did
    if std::mem::take(&mut view_state.job_state.settle) {
        view_state.job_state.moved.clear();
    }

    view_state.set_cards(character);
}

fn poll_character_value(lv: u64, view_state: &mut ThreadList) {
    view_state.card_reads.land_v = lv;
    let mut character_value = view_state.cards.clone();

    for key in &mut character_value {
        key.land = crate::domain::landed::of(&key.id).or(key.land.take());
    }

    view_state.set_cards(character_value);
}

fn poll_thread_list(receiver_state: &mut ThreadList, shown: bool) {
    if let Some(rx) = &receiver_state.card_reads.loading
        && let Ok(character) = rx.try_recv()
    {
        poll_body(character, receiver_state);
    }

    // a thread's files changed (state, log, draft): the cards again (0 timer)
    for thread in receiver_state.card_reads.topics.try_iter() {
        receiver_state.card_reads.cards_due |= matches!(
            thread,
            crate::infra::bus::Topic::Thread(..) | crate::infra::bus::Topic::Rescan(_)
        );
    }

    let due = receiver_state.card_reads.cards_at.is_none() || receiver_state.card_reads.cards_due;

    if shown && due && receiver_state.card_reads.loading.is_none() && !cfg!(test) {
        receiver_state.card_reads.cards_due = false;
        receiver_state.reload();
    }

    // new landed states: on the cards at once (0 wait for the next card read)
    let lv = crate::domain::landed::version();

    if shown && lv != receiver_state.card_reads.land_v {
        poll_character_value(lv, receiver_state);
    }

    if shown {
        crate::domain::landed::visible();
    }

    if let Some(rx) = &receiver_state.job_state.job
        && let Ok((text, err)) = rx.try_recv()
    {
        receiver_state.job_state.job = None;
        receiver_state.done(text, err);
    }

    if receiver_state
        .job_state
        .status
        .as_ref()
        .is_some_and(|text_value| text_value.2.elapsed() > STATUS_FOR)
    {
        receiver_state.job_state.status = None;
    }
}
