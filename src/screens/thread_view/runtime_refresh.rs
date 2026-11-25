use super::{Duration, Runtime, client, store};

impl Runtime {
    pub(super) fn refresh(&mut self) {
        self.dirty |= self.view.tick_esc();
        self.resume_countdown();
        let mut roster_due = false;

        while let Ok(topic) = self.topics.try_recv() {
            roster_due |= self.topic(topic);
        }

        if self.wake {
            self.wake = false;
            self.dirty |= self.view.read();
        }

        self.activity();
        self.flows();

        if roster_due {
            self.view.set_roster(crate::domain::agents::roster_only());
            crate::ui::thread_card::mark_seen(&self.id);
        }
    }

    fn resume_countdown(&mut self) {
        let second = crate::engine::now_ms() / 1000;

        if !self.view.thread.resumes.is_empty() && second != self.count_second {
            self.count_second = second;
            self.view.stream.dirty = true;
            self.dirty = true;
        }
    }

    fn topic(&mut self, topic: crate::infra::bus::Topic) -> bool {
        match topic {
            crate::infra::bus::Topic::Thread(id, crate::infra::bus::Part::Log) if id == self.id => {
                self.wake = true;
                true
            }
            crate::infra::bus::Topic::Thread(id, crate::infra::bus::Part::Meta) if id == self.id => {
                self.flows_due = true;
                true
            }
            _ => false,
        }
    }

    fn activity(&mut self) {
        let kids: Vec<String> = self.view.subthreads.kids.iter().map(|kid| kid.id.clone()).collect();

        self.dirty |= self.view.monitors.tick(&self.id, &kids);
        self.dirty |= self.view.subagents.tick(&self.view.thread.subs);
        self.dirty |= self.view.subthreads.tick(&self.id);
    }

    fn flows(&mut self) {
        let expired = self.kickoff.is_some_and(|at| at.elapsed() > Duration::from_secs(5));

        if !self.flows_due && !expired {
            return;
        }

        self.flows_due = false;

        let Some(thread) = store::load(&self.id) else {
            return;
        };

        let mut changed = false;

        for flow in &mut self.view.flowbars {
            changed |= flow.sync(&thread, false);
        }

        if !thread.kickoff {
            self.kickoff = None;
        } else if expired {
            self.kickoff_failed(&thread.task);
        }

        if changed {
            self.view.sync_flow_ask();
            self.view.stream.dirty = true;
            self.dirty = true;
        }
    }

    fn kickoff_failed(&mut self, task: &str) {
        self.kickoff = None;

        if !client::alive(&self.id) {
            self.view.kickoff_failed(task);
            self.dirty = true;
        }
    }
}
