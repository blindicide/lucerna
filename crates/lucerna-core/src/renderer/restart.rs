//! Bounded automatic restarts (directive §51).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often a failing renderer may be restarted before Lucerna gives up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestartPolicy {
    pub max_restarts: u32,
    pub window: Duration,
    /// Delay before the 1st, 2nd and 3rd (and later) restart.
    pub backoff: [Duration; 3],
}

impl RestartPolicy {
    pub const DEFAULT_MAX_RESTARTS: u32 = 3;
    pub const DEFAULT_WINDOW_SECS: u64 = 60;
    pub const BACKOFF: [Duration; 3] = [
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(4),
    ];

    /// Build from user configuration. Values are clamped so the policy can never be unbounded:
    /// `max_restarts` to 1..=10 and `window_secs` to 10..=3600.
    pub fn clamped(max_restarts: u32, window_secs: u64) -> Self {
        Self {
            max_restarts: max_restarts.clamp(1, 10),
            window: Duration::from_secs(window_secs.clamp(10, 3600)),
            backoff: Self::BACKOFF,
        }
    }
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::clamped(Self::DEFAULT_MAX_RESTARTS, Self::DEFAULT_WINDOW_SECS)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartDecision {
    RetryAfter(Duration),
    GiveUp,
}

/// Sliding-window failure counter.
#[derive(Clone, Debug)]
pub struct RestartTracker {
    policy: RestartPolicy,
    failures: VecDeque<Instant>,
}

impl RestartTracker {
    pub fn new(policy: RestartPolicy) -> Self {
        Self {
            policy,
            failures: VecDeque::new(),
        }
    }

    pub fn policy(&self) -> &RestartPolicy {
        &self.policy
    }

    fn prune(&mut self, now: Instant) {
        while let Some(&oldest) = self.failures.front() {
            if now.saturating_duration_since(oldest) >= self.policy.window {
                self.failures.pop_front();
            } else {
                break;
            }
        }
    }

    /// Record a failure at `now` and decide what to do about it.
    ///
    /// Failures older than the window are forgotten, so a renderer that ran cleanly for longer
    /// than the window gets its full budget back.
    pub fn on_failure(&mut self, now: Instant) -> RestartDecision {
        self.prune(now);
        if self.failures.len() as u32 >= self.policy.max_restarts {
            return RestartDecision::GiveUp;
        }
        self.failures.push_back(now);
        let index = (self.failures.len() - 1).min(self.policy.backoff.len() - 1);
        RestartDecision::RetryAfter(self.policy.backoff[index])
    }

    /// Forget all failures (an explicit user action).
    pub fn reset(&mut self) {
        self.failures.clear();
    }

    /// Failures currently counted inside the window.
    pub fn count_in_window(&self, now: Instant) -> u32 {
        self.failures
            .iter()
            .filter(|&&t| now.saturating_duration_since(t) < self.policy.window)
            .count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn three_failures_retry_then_give_up_with_growing_backoff() {
        let t0 = Instant::now();
        let mut tracker = RestartTracker::new(RestartPolicy::default());
        assert_eq!(tracker.on_failure(t0), RestartDecision::RetryAfter(secs(1)));
        assert_eq!(
            tracker.on_failure(t0 + secs(2)),
            RestartDecision::RetryAfter(secs(2))
        );
        assert_eq!(
            tracker.on_failure(t0 + secs(5)),
            RestartDecision::RetryAfter(secs(4))
        );
        assert_eq!(tracker.on_failure(t0 + secs(10)), RestartDecision::GiveUp);
        assert_eq!(tracker.on_failure(t0 + secs(11)), RestartDecision::GiveUp);
    }

    #[test]
    fn budget_returns_after_the_window_slides() {
        let t0 = Instant::now();
        let mut tracker = RestartTracker::new(RestartPolicy::default());
        for i in 0..3 {
            assert!(matches!(
                tracker.on_failure(t0 + secs(i)),
                RestartDecision::RetryAfter(_)
            ));
        }
        assert_eq!(tracker.on_failure(t0 + secs(30)), RestartDecision::GiveUp);
        // 61 s after the first failure it has slid out of the window.
        assert_eq!(
            tracker.on_failure(t0 + secs(61)),
            RestartDecision::RetryAfter(secs(2))
        );
    }

    #[test]
    fn reset_restores_the_full_budget() {
        let t0 = Instant::now();
        let mut tracker = RestartTracker::new(RestartPolicy::default());
        for _ in 0..3 {
            tracker.on_failure(t0);
        }
        assert_eq!(tracker.on_failure(t0), RestartDecision::GiveUp);
        tracker.reset();
        assert_eq!(tracker.count_in_window(t0), 0);
        assert_eq!(tracker.on_failure(t0), RestartDecision::RetryAfter(secs(1)));
    }

    #[test]
    fn config_values_are_clamped_so_the_policy_is_always_bounded() {
        let p = RestartPolicy::clamped(0, 0);
        assert_eq!((p.max_restarts, p.window), (1, secs(10)));
        let p = RestartPolicy::clamped(9999, 999_999);
        assert_eq!((p.max_restarts, p.window), (10, secs(3600)));
        let p = RestartPolicy::clamped(3, 60);
        assert_eq!(p, RestartPolicy::default());
    }

    #[test]
    fn count_in_window_ignores_old_failures() {
        let t0 = Instant::now();
        let mut tracker = RestartTracker::new(RestartPolicy::default());
        tracker.on_failure(t0);
        tracker.on_failure(t0 + secs(50));
        assert_eq!(tracker.count_in_window(t0 + secs(55)), 2);
        assert_eq!(tracker.count_in_window(t0 + secs(70)), 1);
    }

    #[test]
    fn max_restarts_of_one_gives_up_on_the_second_failure() {
        let t0 = Instant::now();
        let mut tracker = RestartTracker::new(RestartPolicy::clamped(1, 60));
        assert_eq!(tracker.on_failure(t0), RestartDecision::RetryAfter(secs(1)));
        assert_eq!(tracker.on_failure(t0), RestartDecision::GiveUp);
    }
}
