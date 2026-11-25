use super::{STATUS_FOR, ThreadList};

impl ThreadList {
    /// Reads, a job's end, the status's end. SHOWN: the list shows (reads every 2s).
    pub(crate) fn poll(&mut self, shown: bool) {
        poll_thread_list(self, shown);
    }
}

fn take_read(cards: Vec<crate::ui::thread_card::Card>, thread_list: &mut ThreadList) {
    thread_list.card_reads.loading = None;

    // the 1st read after the job's end: the store holds what the job did
    if std::mem::take(&mut thread_list.job_state.settle) {
        thread_list.job_state.moved.clear();
    }

    thread_list.set_cards(cards);
}

fn refresh_lands(lv: u64, thread_list: &mut ThreadList) {
    thread_list.card_reads.land_v = lv;
    let mut cards = thread_list.cards.clone();

    for card in &mut cards {
        card.land = crate::domain::landed::of(&card.id).or(card.land.take());
    }

    thread_list.set_cards(cards);
}

fn poll_thread_list(thread_list: &mut ThreadList, shown: bool) {
    if let Some(rx) = &thread_list.card_reads.loading
        && let Ok(cards) = rx.try_recv()
    {
        take_read(cards, thread_list);
    }

    // a thread's files changed (state, log, draft): the cards again (0 timer)
    for topic in thread_list.card_reads.topics.try_iter() {
        thread_list.card_reads.cards_due |= matches!(
            topic,
            crate::infra::bus::Topic::Thread(..) | crate::infra::bus::Topic::Rescan(_)
        );
    }

    let due = thread_list.card_reads.cards_at.is_none() || thread_list.card_reads.cards_due;

    if shown && due && thread_list.card_reads.loading.is_none() && !cfg!(test) {
        thread_list.card_reads.cards_due = false;
        thread_list.reload();
    }

    // new landed states: on the cards at once (0 wait for the next card read)
    let lv = crate::domain::landed::version();

    if shown && lv != thread_list.card_reads.land_v {
        refresh_lands(lv, thread_list);
    }

    if shown {
        crate::domain::landed::visible();
    }

    if let Some(rx) = &thread_list.job_state.job
        && let Ok((text, err)) = rx.try_recv()
    {
        thread_list.job_state.job = None;
        thread_list.done(text, err);
    }

    if thread_list
        .job_state
        .status
        .as_ref()
        .is_some_and(|status| status.2.elapsed() > STATUS_FOR)
    {
        thread_list.job_state.status = None;
    }
}
