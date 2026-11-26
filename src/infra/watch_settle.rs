//! Work tree topics wait until their burst settles: an agent that writes 20 files (or 1 file in
//! steps: a temp file, a rename) costs the readers 1 read, not 20. A constant stream still
//! publishes every `MAX_WAIT`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Quiet time after a work tree's last event before its topic goes out.
pub(super) const QUIET: Duration = Duration::from_millis(200);
/// The longest a work tree's topic waits while its events keep coming.
pub(super) const MAX_WAIT: Duration = Duration::from_secs(1);

/// The held work trees: (first event, last event).
#[derive(Default)]
pub(super) struct Settle {
    held: HashMap<PathBuf, (Instant, Instant)>,
}

impl Settle {
    /// An event in work tree WORK at NOW.
    pub(super) fn hold(&mut self, work: PathBuf, now: Instant) {
        self.held
            .entry(work)
            .and_modify(|(_, last)| *last = now)
            .or_insert((now, now));
    }

    /// The work trees whose topic is due at NOW: out of the hold.
    pub(super) fn due(&mut self, now: Instant) -> Vec<PathBuf> {
        let due: Vec<PathBuf> = self
            .held
            .iter()
            .filter(|(_, times)| deadline(**times) <= now)
            .map(|(work, _)| work.clone())
            .collect();

        for work in &due {
            self.held.remove(work);
        }

        due
    }

    /// The next deadline (None: 0 held).
    pub(super) fn next(&self) -> Option<Instant> {
        self.held.values().map(|times| deadline(*times)).min()
    }
}

fn deadline((first, last): (Instant, Instant)) -> Instant {
    (last + QUIET).min(first + MAX_WAIT)
}

#[cfg(test)]
#[path = "watch_settle_tests.rs"]
mod tests;
