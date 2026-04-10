use roux_core::WatchOutcome;

/// Tracks recent outcome transitions for flap debouncing.
pub struct FlapTracker {
    last_outcomes: Vec<(WatchOutcome, u64)>,
}

impl FlapTracker {
    pub fn new() -> Self {
        Self { last_outcomes: Vec::new() }
    }

    pub fn record(&mut self, outcome: WatchOutcome, now_ms: u64) {
        self.last_outcomes.push((outcome, now_ms));
        if self.last_outcomes.len() > 3 {
            self.last_outcomes.remove(0);
        }
    }

    pub fn is_flapping(&self) -> bool {
        if self.last_outcomes.len() < 3 {
            return false;
        }
        let recent = &self.last_outcomes;
        let window = recent.last().unwrap().1 - recent[recent.len() - 3].1;
        if window > 60_000 {
            return false;
        }
        let last = &recent[recent.len() - 1].0;
        let prev = &recent[recent.len() - 2].0;
        if last == prev {
            return false;
        }
        true
    }
}
