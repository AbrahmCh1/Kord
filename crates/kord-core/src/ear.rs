//! Ear detection: decides when to pause and resume playback.
//!
//! The detector only looks at *how many* buds are in an ear. That makes it work
//! the same whether you wear both buds or a single one: taking out the only
//! bud you're wearing pauses, putting it back resumes.

use std::time::{Duration, Instant};

/// When a removal should pause playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PauseMode {
    /// Pause as soon as any worn bud is taken out (what Apple devices do).
    #[default]
    AnyRemoved,
    /// Pause only once no bud is left in an ear.
    AllRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarConfig {
    pub mode: PauseMode,
    /// How long a new reading must hold before it is trusted. Buds briefly
    /// report wrong values while being handled.
    pub debounce: Duration,
    /// Don't resume if the buds go back in after this long; you've probably
    /// moved on. `None` always resumes.
    pub resume_window: Option<Duration>,
}

impl Default for EarConfig {
    fn default() -> Self {
        Self {
            mode: PauseMode::AnyRemoved,
            debounce: Duration::from_millis(300),
            resume_window: Some(Duration::from_secs(10 * 60)),
        }
    }
}

/// What the caller should do with media playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarAction {
    Pause,
    Resume,
}

#[derive(Debug, Clone)]
pub struct EarDetector {
    config: EarConfig,
    /// Last debounced number of buds in an ear.
    stable: Option<u8>,
    /// A different reading waiting to outlast the debounce period.
    pending: Option<(u8, Instant)>,
    /// Set while playback is paused because of us: the number of buds that
    /// must be back in before resuming, and when we paused.
    paused: Option<(u8, Instant)>,
}

impl EarDetector {
    pub fn new(config: EarConfig) -> Self {
        Self { config, stable: None, pending: None, paused: None }
    }

    pub fn set_config(&mut self, config: EarConfig) {
        self.config = config;
    }

    /// Forgets everything, e.g. when the tracked device changes or disappears.
    pub fn reset(&mut self) {
        self.stable = None;
        self.pending = None;
        self.paused = None;
    }

    /// Feeds the latest worn-bud count. The first reading only establishes a
    /// baseline, so starting Kord never pauses anything.
    pub fn update(&mut self, worn: u8, now: Instant) -> Option<EarAction> {
        let Some(stable) = self.stable else {
            self.stable = Some(worn);
            return None;
        };
        if worn == stable {
            self.pending = None;
            return None;
        }
        if self.pending.map(|(count, _)| count) != Some(worn) {
            self.pending = Some((worn, now));
        }
        self.poll(now)
    }

    /// When a pending reading will have outlasted the debounce period. Call
    /// [`poll`](Self::poll) then, so acting doesn't wait for the next
    /// advertisement, which can be seconds away.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.map(|(_, since)| since + self.config.debounce)
    }

    /// Commits a pending reading once it has held for the debounce period.
    pub fn poll(&mut self, now: Instant) -> Option<EarAction> {
        let (worn, since) = self.pending?;
        if now.duration_since(since) < self.config.debounce {
            return None;
        }
        self.pending = None;
        let from = self.stable.replace(worn)?;
        self.transition(from, worn, now)
    }

    fn transition(&mut self, from: u8, to: u8, now: Instant) -> Option<EarAction> {
        if to < from {
            let should_pause = match self.config.mode {
                PauseMode::AnyRemoved => true,
                PauseMode::AllRemoved => to == 0,
            };
            if should_pause && self.paused.is_none() {
                let resume_at = match self.config.mode {
                    PauseMode::AnyRemoved => from,
                    PauseMode::AllRemoved => 1,
                };
                self.paused = Some((resume_at, now));
                return Some(EarAction::Pause);
            }
        } else if let Some((resume_at, paused_at)) = self.paused
            && to >= resume_at
        {
            self.paused = None;
            let expired = self.config.resume_window.is_some_and(|window| now.duration_since(paused_at) > window);
            if !expired {
                return Some(EarAction::Resume);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    struct Harness {
        detector: EarDetector,
        start: Instant,
    }

    impl Harness {
        fn new(config: EarConfig) -> Self {
            // Tests were written for a 500 ms debounce.
            let config = EarConfig { debounce: Duration::from_millis(500), ..config };
            Self { detector: EarDetector::new(config), start: Instant::now() }
        }

        fn at(&mut self, ms: u64, worn: u8) -> Option<EarAction> {
            self.detector.update(worn, self.start + MS * ms as u32)
        }
    }

    #[test]
    fn baseline_never_triggers() {
        let mut h = Harness::new(EarConfig::default());
        assert_eq!(h.at(0, 0), None);
        assert_eq!(h.at(1000, 0), None);
    }

    #[test]
    fn removing_one_bud_pauses_and_reinserting_resumes() {
        let mut h = Harness::new(EarConfig::default());
        h.at(0, 2);
        assert_eq!(h.at(100, 1), None, "still debouncing");
        assert_eq!(h.at(700, 1), Some(EarAction::Pause));
        assert_eq!(h.at(800, 0), None, "already paused");
        assert_eq!(h.at(1400, 0), None);
        assert_eq!(h.at(1500, 1), None);
        assert_eq!(h.at(2100, 1), None, "only one bud back in");
        assert_eq!(h.at(2200, 2), None);
        assert_eq!(h.at(2800, 2), Some(EarAction::Resume));
    }

    #[test]
    fn single_bud_listening() {
        let mut h = Harness::new(EarConfig::default());
        h.at(0, 1);
        h.at(10, 0);
        assert_eq!(h.at(600, 0), Some(EarAction::Pause));
        h.at(700, 1);
        assert_eq!(h.at(1300, 1), Some(EarAction::Resume));
    }

    #[test]
    fn timer_commits_without_another_reading() {
        let mut h = Harness::new(EarConfig::default());
        h.at(0, 2);
        assert_eq!(h.at(100, 1), None);
        assert_eq!(h.detector.deadline(), Some(h.start + MS * 600));
        assert_eq!(h.detector.poll(h.start + MS * 599), None);
        assert_eq!(h.detector.poll(h.start + MS * 600), Some(EarAction::Pause));
        assert_eq!(h.detector.deadline(), None);
    }

    #[test]
    fn glitches_shorter_than_debounce_are_ignored() {
        let mut h = Harness::new(EarConfig::default());
        h.at(0, 2);
        h.at(100, 1);
        assert_eq!(h.at(300, 2), None);
        assert_eq!(h.at(1000, 2), None);
    }

    #[test]
    fn all_removed_mode_waits_for_both() {
        let mut h = Harness::new(EarConfig { mode: PauseMode::AllRemoved, ..EarConfig::default() });
        h.at(0, 2);
        h.at(10, 1);
        assert_eq!(h.at(600, 1), None);
        h.at(700, 0);
        assert_eq!(h.at(1300, 0), Some(EarAction::Pause));
        h.at(1400, 1);
        assert_eq!(h.at(2000, 1), Some(EarAction::Resume));
    }

    #[test]
    fn does_not_resume_after_window() {
        let mut h = Harness::new(EarConfig { resume_window: Some(Duration::from_secs(5)), ..EarConfig::default() });
        h.at(0, 2);
        h.at(10, 0);
        assert_eq!(h.at(600, 0), Some(EarAction::Pause));
        h.at(10_000, 2);
        assert_eq!(h.at(10_600, 2), None);
        // A fresh removal pauses again.
        h.at(11_000, 1);
        assert_eq!(h.at(11_600, 1), Some(EarAction::Pause));
    }
}
