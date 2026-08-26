//! Opening a stream that has been shown to work.
//!
//! Every attempt is a fresh stream, discarded whole when its probe goes unanswered. Nothing is
//! retried once the wallet's bytes are moving: resuming a conversation would mean rebuilding HTTP/2
//! state and replaying requests already in flight, and a request that was delivered twice is worse
//! than one that failed cleanly.
//!
//! Attempts are grouped into **rounds**, because how they are grouped decides what establishing a
//! connection costs. A failure is silent, so it is only ever discovered by the probe deadline
//! expiring: a round of one pays that deadline once per failure, in series, while a round of several
//! pays it once for the whole round and takes whichever stream answers first. The trade is reply
//! blocks, and that streams opened together meet the same conditions, so a bad moment can take all
//! of them at once.
//!
//! Both the rounds and the streams within them are reported to the caller rather than only logged.
//! The gap between how often a stream fails and how often the wallet notices is the pair the metrics
//! are built on, and whether retrying helps at all depends on failures being independent, which can
//! only be seen by counting them separately.

use std::num::NonZeroU32;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use nym_sdk::mixnet::{MixnetClient, MixnetStream, Recipient};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::handshake::{self, HandshakeError};

/// How hard to insist on a stream that answers.
#[derive(Debug, Clone, Copy)]
pub struct ProbeSettings {
    /// How long one probe waits for its echo. Round trips measured seconds with a long tail, so a
    /// deadline tight enough to keep establishment quick also discards healthy streams. The two
    /// distributions overlap, so any setting here gives up one for the other.
    pub timeout: Duration,
    /// How many streams may be opened in total before giving up.
    pub attempts: NonZeroU32,
    /// How many are opened at once. One makes each failure cost a full deadline before the next
    /// attempt begins; more turns that sum into a minimum.
    pub concurrency: NonZeroU32,
}

/// How to reach the listening half.
#[derive(Debug, Clone, Copy)]
pub struct DialSettings {
    /// Reply blocks attached to each outbound message; `None` leaves the SDK default of 10. Raising
    /// it lowers the failure rate and costs latency, and not for the reason it looks like: every
    /// message carries the budget, so a larger one puts a larger `Open` in front of a larger first
    /// `Data` and the two stop racing. It will not get you to zero.
    pub reply_surbs: Option<u32>,
    /// `None` sends the header without waiting for it, which is the shape this takes if the
    /// transport ever stops losing first payloads.
    pub probe: Option<ProbeSettings>,
}

/// A stream that answered, and what it cost to get one.
pub struct Dialled {
    pub stream: MixnetStream,
    /// How long the round that answered took. Every round before it is outside this, so it is what
    /// one round costs and not what the caller waited.
    pub answering_round: Duration,
    /// Every round run, the answering one last.
    pub rounds: Vec<Round>,
}

/// Every attempt failed.
#[derive(Debug, thiserror::Error)]
#[error("no stream answered after {} attempts", .rounds.iter().map(Round::attempted).sum::<usize>())]
pub struct GaveUp {
    pub rounds: Vec<Round>,
}

/// One group of streams opened together.
#[derive(Debug, Default)]
pub struct Round {
    /// Streams the SDK actually opened. Counted separately because a round stops at the first
    /// answer, so the rest are cancelled without a verdict: they cost reply blocks and left a
    /// stream on the far side, and only this number says how many.
    pub opened: u32,
    /// Streams thrown away, with why and how long each took.
    pub discarded: Vec<Discarded>,
    /// Whether one of this round's streams answered.
    pub answered: bool,
}

/// A stream that was opened and thrown away.
#[derive(Debug)]
pub struct Discarded {
    pub elapsed: Duration,
    pub error: DialError,
}

/// Why one attempt did not produce a usable stream.
#[derive(Debug, thiserror::Error)]
pub enum DialError {
    #[error("opening a mixnet stream: {0}")]
    Open(#[from] nym_sdk::Error),
    #[error("probing a fresh stream: {0}")]
    Probe(#[from] HandshakeError),
}

impl Round {
    /// How many times this round tried to open a stream, failures to open included.
    pub fn attempted(&self) -> usize {
        self.opened as usize + self.failures(DialError::is_open_failure)
    }

    /// Streams that were opened and then abandoned without a verdict, because another answered
    /// first. They are the price of opening several at once.
    pub fn cancelled(&self) -> usize {
        (self.opened as usize)
            .saturating_sub(self.failures(|error| !error.is_open_failure()))
            .saturating_sub(usize::from(self.answered))
    }

    /// Streams that were opened and whose probe went unanswered.
    pub fn unanswered(&self) -> usize {
        self.failures(|error| !error.is_open_failure())
    }

    fn failures(&self, matching: impl Fn(&DialError) -> bool) -> usize {
        self.discarded
            .iter()
            .filter(|stream| matching(&stream.error))
            .count()
    }
}

impl DialError {
    /// Whether the SDK refused to open the stream at all.
    ///
    /// This is a local failure, reported immediately, and not the silent loss this project exists to
    /// filter. Counting the two together makes a degraded client look like a degraded network.
    pub fn is_open_failure(&self) -> bool {
        matches!(self, DialError::Open(_))
    }
}

/// Open streams until one answers its probe, or until the attempts run out.
pub async fn dial(
    client: &Mutex<MixnetClient>,
    server: Recipient,
    settings: DialSettings,
) -> Result<Dialled, GaveUp> {
    let (attempts, concurrency) = match settings.probe {
        Some(probe) => (probe.attempts.get(), probe.concurrency.get()),
        None => (1, 1),
    };

    let mut rounds = Vec::new();
    let mut remaining = attempts;

    while remaining > 0 {
        let size = concurrency.min(remaining);
        remaining -= size;

        let (answered, round) = run_round(client, server, settings, size).await;
        rounds.push(round);

        if let Some((stream, answering_round)) = answered {
            return Ok(Dialled {
                stream,
                answering_round,
                rounds,
            });
        }
    }

    Err(GaveUp { rounds })
}

/// Open `size` streams together and keep the first one to answer.
///
/// The opens run before any probe does, deliberately: `open_stream` is documented as not cancel
/// safe, and cancelling one mid-flight leaves a stream registered with no owner. Probes are
/// cancelled freely, because by then the stream exists and dropping it deregisters it.
async fn run_round(
    client: &Mutex<MixnetClient>,
    server: Recipient,
    settings: DialSettings,
    size: u32,
) -> (Option<(MixnetStream, Duration)>, Round) {
    let started = Instant::now();
    let mut round = Round::default();
    let mut opened = Vec::with_capacity(size as usize);

    for _ in 0..size {
        // The lock is held for the open alone, which only queues a message. Concurrent callers
        // serialise on that and then run their probes in parallel.
        match client
            .lock()
            .await
            .open_stream(server, settings.reply_surbs)
            .await
        {
            Ok(stream) => {
                round.opened += 1;
                opened.push(stream);
            }
            Err(error) => round.discarded.push(Discarded {
                elapsed: started.elapsed(),
                error: error.into(),
            }),
        }
    }

    let Some(probe) = settings.probe else {
        // With no probe there is nothing to wait for, so the header goes out and the stream is used
        // as it is.
        let Some(mut stream) = opened.pop() else {
            return (None, round);
        };
        return match handshake::announce(&mut stream).await {
            Ok(()) => {
                round.answered = true;
                (Some((stream, started.elapsed())), round)
            }
            Err(error) => {
                round.discarded.push(Discarded {
                    elapsed: started.elapsed(),
                    error: error.into(),
                });
                (None, round)
            }
        };
    };

    let mut probing: FuturesUnordered<_> = opened
        .into_iter()
        .map(|mut stream| async move {
            match handshake::probe(&mut stream, probe.timeout).await {
                Ok(round_trip) => Ok((stream, round_trip)),
                Err(error) => Err(error),
            }
        })
        .collect();

    while let Some(result) = probing.next().await {
        match result {
            Ok((stream, round_trip)) => {
                tracing::debug!(?round_trip, "a stream answered its probe");
                round.answered = true;
                // Dropping the rest cancels their probes and deregisters their streams.
                return (Some((stream, started.elapsed())), round);
            }
            Err(error) => {
                tracing::debug!(%error, "discarding a stream that did not answer");
                round.discarded.push(Discarded {
                    elapsed: started.elapsed(),
                    error: error.into(),
                });
            }
        }
    }

    (None, round)
}

impl Dialled {
    /// How many streams were thrown away before one answered.
    pub fn discarded(&self) -> usize {
        self.rounds.iter().map(|round| round.discarded.len()).sum()
    }
}

impl GaveUp {
    /// What the last attempt failed with, which is the one worth reporting.
    pub fn last_error(&self) -> Option<&DialError> {
        self.rounds
            .last()?
            .discarded
            .last()
            .map(|attempt| &attempt.error)
    }

    /// How many streams were opened in total.
    pub fn attempts(&self) -> usize {
        self.rounds.iter().map(Round::attempted).sum()
    }

    /// Whether the SDK refused every open, so no stream ever existed.
    ///
    /// This is the local client failing rather than the transport: a healthy client on a bad network
    /// still opens streams, and it is their probes that go unanswered.
    pub fn nothing_opened(&self) -> bool {
        self.rounds.iter().all(|round| round.opened == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_failure() -> Discarded {
        Discarded {
            elapsed: Duration::from_secs(10),
            error: HandshakeError::TimedOut(Duration::from_secs(10)).into(),
        }
    }

    fn refused_open() -> Discarded {
        Discarded {
            elapsed: Duration::ZERO,
            error: nym_sdk::Error::IoError(std::io::Error::other("the client refused")).into(),
        }
    }

    #[test]
    fn an_answered_round_counts_the_streams_left_without_a_verdict() {
        let round = Round {
            opened: 3,
            discarded: vec![probe_failure()],
            answered: true,
        };

        assert_eq!(
            (round.attempted(), round.unanswered(), round.cancelled()),
            (3, 1, 1)
        );
    }

    #[test]
    fn an_unanswered_round_cancels_nothing() {
        let round = Round {
            opened: 3,
            discarded: vec![probe_failure(), probe_failure(), probe_failure()],
            answered: false,
        };

        assert_eq!(
            (round.attempted(), round.unanswered(), round.cancelled()),
            (3, 3, 0)
        );
    }

    #[test]
    fn a_refused_open_is_an_attempt_that_never_became_a_stream() {
        let round = Round {
            opened: 0,
            discarded: vec![refused_open()],
            answered: false,
        };

        assert_eq!(
            (round.attempted(), round.unanswered(), round.cancelled()),
            (1, 0, 0)
        );
    }

    #[test]
    fn giving_up_reports_the_last_error_and_every_attempt() {
        let gave_up = GaveUp {
            rounds: vec![
                Round {
                    opened: 1,
                    discarded: vec![probe_failure()],
                    answered: false,
                },
                Round {
                    opened: 0,
                    discarded: vec![refused_open()],
                    answered: false,
                },
            ],
        };

        assert_eq!(
            (
                gave_up.attempts(),
                gave_up.last_error().map(DialError::is_open_failure)
            ),
            (2, Some(true))
        );
    }

    #[test]
    fn giving_up_with_streams_opened_is_the_transport_failing_and_not_the_client() {
        let gave_up = GaveUp {
            rounds: vec![Round {
                opened: 3,
                discarded: vec![probe_failure(), probe_failure(), probe_failure()],
                answered: false,
            }],
        };

        assert!(!gave_up.nothing_opened());
    }

    #[test]
    fn giving_up_without_ever_opening_a_stream_is_the_local_client_failing() {
        let gave_up = GaveUp {
            rounds: vec![
                Round {
                    opened: 0,
                    discarded: vec![refused_open()],
                    answered: false,
                },
                Round {
                    opened: 0,
                    discarded: vec![refused_open()],
                    answered: false,
                },
            ],
        };

        assert!(gave_up.nothing_opened());
    }
}
