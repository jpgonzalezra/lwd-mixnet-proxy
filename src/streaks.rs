//! Whether anything is getting through right now, folded one dial at a time.
//!
//! Two runs are counted and they answer different questions. Dials where the local client refused
//! every open are this machine failing, and enough of them end the process ([ADR
//! 0010](../../docs/decisions/0010-exit-when-the-local-client-degrades.md)). Dials that opened
//! streams and got nothing back are the far side or the transport, which no restart mends, so a run
//! of those is reported and nothing else (ADR 0014).
//!
//! Both live behind one lock rather than in atomics. The report they feed is spread over three
//! places, the counters, the gauge and the health flag, and updating those separately lets two
//! connections finishing at once leave a half claiming it is degraded with a streak of zero, or the
//! reverse, and stay that way until traffic happens to correct it.
//!
//! The lock settles what writers do to each other, not what a reader sees mid-write. `/metrics` is
//! scraped through a registry this cannot lock, so a scrape landing between the gauge and the flag
//! reads one of them a moment early. What the lock buys is that the disagreement lasts a few
//! instructions instead of until the next connection: every fold leaves the three agreeing.

use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::health::Health;
use crate::metrics::ClientMetrics;

/// Consecutive dials that opened no stream at all before the process gives up. Enough to rule out a
/// blip, and a client this far gone refuses instantly, so five cost milliseconds.
const REFUSAL_LIMIT: usize = 5;

/// Consecutive dials that ended with nothing before this half calls itself degraded. Each one has
/// already spent the whole attempt budget, so three is a long time with no answer.
const DEAD_END_LIMIT: usize = 3;

/// How one dial ended, as far as anything outside the dialling code needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A stream answered, so the wallet has something to talk to.
    Answered,
    /// A stream was handed to the wallet without anything coming back from the far side, which is
    /// what `--no-probe` asks for. It proves the local client can open streams and proves nothing
    /// about the other end, so it breaks a refusal run and leaves the rest alone.
    Unprobed,
    /// The attempt budget ran out with the local client refusing every open.
    NothingOpened,
    /// The attempt budget ran out with streams opened and never answered.
    NothingAnswered,
}

#[derive(Debug, Default)]
struct Counts {
    refusals: usize,
    dead_ends: usize,
}

/// The two runs, and what they are reported through.
pub struct Streaks {
    counts: Mutex<Counts>,
    health: Health,
    metrics: Arc<ClientMetrics>,
    client_is_dead: Notify,
}

impl Streaks {
    pub fn new(health: Health, metrics: Arc<ClientMetrics>) -> Self {
        Self {
            counts: Mutex::new(Counts::default()),
            health,
            metrics,
            client_is_dead: Notify::new(),
        }
    }

    /// Notified once the local client has refused every open for [`REFUSAL_LIMIT`] dials in a row.
    pub fn client_is_dead(&self) -> &Notify {
        &self.client_is_dead
    }

    /// Fold one dial's outcome in and publish what it changed.
    ///
    /// Everything happens under the one lock, so the streak, the gauge and the health flag cannot
    /// be read in states that contradict each other. Nothing here awaits, so holding a blocking
    /// mutex across it is safe.
    ///
    /// **Call this when the dial resolves, not when the connection it carried ends.** A connection
    /// that ran for ten minutes and closed says nothing about whether the far side is answering
    /// now, and folding it in at that point lets a long-lived success wipe out a degradation that
    /// began after it started.
    pub fn fold(&self, outcome: Outcome) {
        // A panic elsewhere should not take the proxy's reporting with it: the counts are a report,
        // and a stale one beats a process that stops carrying traffic.
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match outcome {
            Outcome::Answered => {
                counts.refusals = 0;
                counts.dead_ends = 0;
            }
            Outcome::Unprobed => counts.refusals = 0,
            Outcome::NothingOpened => {
                counts.refusals += 1;
                counts.dead_ends += 1;
            }
            Outcome::NothingAnswered => {
                counts.refusals = 0;
                counts.dead_ends += 1;
            }
        }

        self.metrics.failed_in_a_row(counts.dead_ends);

        let degraded = counts.dead_ends >= DEAD_END_LIMIT;
        if degraded != self.health.is_degraded() {
            self.health.set_degraded(degraded);
            if degraded {
                tracing::error!(
                    streak = counts.dead_ends,
                    "nothing has answered for several connections in a row; the far side may be \
                     gone, and from here that looks the same as a bad hour on the transport"
                );
            } else {
                tracing::info!("a connection got through again");
            }
        }

        if counts.refusals >= REFUSAL_LIMIT {
            self.client_is_dead.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::Registry;

    fn streaks() -> (Streaks, Health, Arc<ClientMetrics>) {
        let health = Health::starting();
        let metrics = Arc::new(ClientMetrics::new().expect("building the metrics"));
        (
            Streaks::new(health.clone(), Arc::clone(&metrics)),
            health,
            metrics,
        )
    }

    fn gauge(registry: &Registry) -> f64 {
        registry
            .gather()
            .iter()
            .filter(|family| family.name() == "lwd_mixnet_client_connections_failed_in_a_row")
            .flat_map(|family| family.get_metric())
            .map(|metric| metric.get_gauge().value())
            .sum()
    }

    #[test]
    fn a_half_nothing_has_failed_on_is_not_degraded() {
        let (streaks, health, _) = streaks();
        streaks.fold(Outcome::Answered);

        assert!(!health.is_degraded());
    }

    #[test]
    fn three_dials_that_ended_with_nothing_degrade_the_half() {
        let (streaks, health, _) = streaks();
        for _ in 0..3 {
            streaks.fold(Outcome::NothingAnswered);
        }

        assert!(health.is_degraded());
    }

    #[test]
    fn two_are_not_enough() {
        let (streaks, health, _) = streaks();
        for _ in 0..2 {
            streaks.fold(Outcome::NothingAnswered);
        }

        assert!(!health.is_degraded());
    }

    #[test]
    fn an_unprobed_dial_neither_degrades_nor_clears() {
        let (streaks, health, metrics) = streaks();
        for _ in 0..2 {
            streaks.fold(Outcome::NothingAnswered);
        }
        streaks.fold(Outcome::Unprobed);
        assert_eq!(
            gauge(metrics.registry()),
            2.0,
            "it cannot count as an answer"
        );
        assert!(!health.is_degraded());

        streaks.fold(Outcome::NothingAnswered);
        assert!(health.is_degraded(), "nor can it hold the run open");
    }

    #[tokio::test]
    async fn an_unprobed_dial_still_ends_a_refusal_run() {
        let (streaks, _, _) = streaks();
        let dead = streaks.client_is_dead().notified();
        tokio::pin!(dead);

        for _ in 0..4 {
            streaks.fold(Outcome::NothingOpened);
        }
        streaks.fold(Outcome::Unprobed);
        for _ in 0..4 {
            streaks.fold(Outcome::NothingOpened);
        }

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), dead)
                .await
                .is_err()
        );
    }

    #[test]
    fn a_dial_that_answered_clears_it() {
        let (streaks, health, _) = streaks();
        for _ in 0..4 {
            streaks.fold(Outcome::NothingAnswered);
        }
        streaks.fold(Outcome::Answered);

        assert!(!health.is_degraded());
    }

    #[test]
    fn refused_opens_count_as_dead_ends_too() {
        let (streaks, health, _) = streaks();
        for _ in 0..3 {
            streaks.fold(Outcome::NothingOpened);
        }

        assert!(health.is_degraded());
    }

    #[test]
    fn the_run_has_to_be_unbroken() {
        let (streaks, health, _) = streaks();
        streaks.fold(Outcome::NothingAnswered);
        streaks.fold(Outcome::NothingAnswered);
        streaks.fold(Outcome::Answered);
        streaks.fold(Outcome::NothingAnswered);
        streaks.fold(Outcome::NothingAnswered);

        assert!(!health.is_degraded());
    }

    /// The invariant the atomics this replaced could not hold: once a fold returns, the number
    /// reported and the flag reported describe the same moment. A reader that lands mid-fold can
    /// still catch one of them early; what cannot happen any more is the two staying apart.
    #[test]
    fn every_fold_leaves_the_gauge_and_the_flag_agreeing() {
        let (streaks, health, metrics) = streaks();
        let sequence = [
            Outcome::NothingAnswered,
            Outcome::NothingOpened,
            Outcome::Answered,
            Outcome::NothingAnswered,
            Outcome::NothingAnswered,
            Outcome::NothingOpened,
            Outcome::NothingAnswered,
            Outcome::Answered,
        ];

        for outcome in sequence {
            streaks.fold(outcome);
            assert_eq!(
                health.is_degraded(),
                gauge(metrics.registry()) >= DEAD_END_LIMIT as f64,
                "the flag and the gauge disagree after {outcome:?}"
            );
        }
    }

    #[tokio::test]
    async fn five_refusals_in_a_row_say_the_local_client_is_dead() {
        let (streaks, _, _) = streaks();
        let dead = streaks.client_is_dead().notified();
        tokio::pin!(dead);

        for _ in 0..4 {
            streaks.fold(Outcome::NothingOpened);
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut dead)
                .await
                .is_err(),
            "four refusals should not be enough"
        );

        streaks.fold(Outcome::NothingOpened);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), dead)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_dial_that_opened_a_stream_ends_the_refusal_run() {
        let (streaks, _, _) = streaks();
        let dead = streaks.client_is_dead().notified();
        tokio::pin!(dead);

        for _ in 0..4 {
            streaks.fold(Outcome::NothingOpened);
        }
        streaks.fold(Outcome::NothingAnswered);
        for _ in 0..4 {
            streaks.fold(Outcome::NothingOpened);
        }

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), dead)
                .await
                .is_err()
        );
    }
}
