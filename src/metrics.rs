//! What each half counts, and why these numbers and not others.
//!
//! The transport's failure rate is not stationary: across three days of measurement it swung by more
//! than an order of magnitude. So no single number tells an operator whether a deployment is broken
//! or the network is having a bad afternoon.
//!
//! What separates the two is a **pair**, counted from the same attempt: how often a freshly opened
//! stream goes unanswered, against how often the wallet ends up with nothing. The first drifts with
//! the transport; the second is the one to keep near zero. The reasoning is in ADR 0007.

use prometheus::{
    Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry, TextEncoder,
    exponential_buckets,
};
use tokio::time::Instant;

use crate::dial::{DialError, Round};
use crate::handshake::HandshakeError;
use crate::splice::{Ended, Transfer};

/// Establishment spans a probe deadline of seconds and up to several rounds of it, so the buckets
/// cover from well under one measured round trip to well past the point a wallet has given up.
fn establishment_buckets() -> Result<Vec<f64>, prometheus::Error> {
    exponential_buckets(0.5, 1.6, 10)
}

/// Bytes carried, per direction, for either half.
fn byte_counter(registry: &Registry, half: &str) -> Result<IntCounterVec, prometheus::Error> {
    let counter = IntCounterVec::new(
        Opts::new(
            format!("lwd_mixnet_{half}_bytes_total"),
            "Bytes copied, by direction relative to the mixnet.",
        ),
        &["direction"],
    )?;
    registry.register(Box::new(counter.clone()))?;
    Ok(counter)
}

/// How connections ended, for either half.
fn ended_counter(registry: &Registry, half: &str) -> Result<IntCounterVec, prometheus::Error> {
    let counter = IntCounterVec::new(
        Opts::new(
            format!("lwd_mixnet_{half}_connections_ended_total"),
            "Finished connections, by how they ended.",
        ),
        &["reason"],
    )?;
    registry.register(Box::new(counter.clone()))?;
    Ok(counter)
}

fn counter(registry: &Registry, name: &str, help: &str) -> Result<IntCounter, prometheus::Error> {
    let counter = IntCounter::with_opts(Opts::new(name, help))?;
    registry.register(Box::new(counter.clone()))?;
    Ok(counter)
}

fn gauge(registry: &Registry, name: &str, help: &str) -> Result<IntGauge, prometheus::Error> {
    let gauge = IntGauge::with_opts(Opts::new(name, help))?;
    registry.register(Box::new(gauge.clone()))?;
    Ok(gauge)
}

/// What the dialling half counts.
pub struct ClientMetrics {
    registry: Registry,
    connections: IntCounter,
    in_flight: IntGauge,
    unestablished: IntCounter,
    failed_in_a_row: IntGauge,
    first_round_failures: IntCounter,
    streams_opened: IntCounter,
    streams_discarded: IntCounterVec,
    rounds: IntCounter,
    establishment: Histogram,
    ended: IntCounterVec,
    bytes: IntCounterVec,
}

impl ClientMetrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        Ok(Self {
            connections: counter(
                &registry,
                "lwd_mixnet_client_connections_total",
                "Local connections accepted from the wallet.",
            )?,
            in_flight: gauge(
                &registry,
                "lwd_mixnet_client_connections_in_flight",
                "Local connections currently being carried.",
            )?,
            unestablished: counter(
                &registry,
                "lwd_mixnet_client_connections_unestablished_total",
                "Connections closed because no stream ever answered. Against \
                 lwd_mixnet_client_connections_total this is the failure rate the wallet sees.",
            )?,
            failed_in_a_row: gauge(
                &registry,
                "lwd_mixnet_client_connections_failed_in_a_row",
                "Connections that ended with no stream, back to back, zero again as soon as one \
                 works. The counters beside it are rates over a process lifetime and say nothing \
                 about now: a serving half that has gone away shows up here and nowhere else.",
            )?,
            first_round_failures: counter(
                &registry,
                "lwd_mixnet_client_first_round_failures_total",
                "Connections whose first round of streams all went unanswered. Against \
                 lwd_mixnet_client_connections_total this is the transport's own failure rate, \
                 before any retry.",
            )?,
            streams_opened: counter(
                &registry,
                "lwd_mixnet_client_streams_opened_total",
                "Mixnet streams opened, across every round.",
            )?,
            streams_discarded: {
                let discarded = IntCounterVec::new(
                    Opts::new(
                        "lwd_mixnet_client_streams_discarded_total",
                        "Streams thrown away before carrying anything, by why.",
                    ),
                    &["reason"],
                )?;
                registry.register(Box::new(discarded.clone()))?;
                discarded
            },
            rounds: counter(
                &registry,
                "lwd_mixnet_client_rounds_total",
                "Rounds of streams opened. Above one per connection is retry doing its job.",
            )?,
            establishment: {
                let establishment = Histogram::with_opts(
                    HistogramOpts::new(
                        "lwd_mixnet_client_establishment_seconds",
                        "Time from accepting a local connection to holding a stream that answered. \
                         Rounds that found nothing are inside it, each costing a probe deadline, so \
                         this is where retrying shows up as a wait.",
                    )
                    .buckets(establishment_buckets()?),
                )?;
                registry.register(Box::new(establishment.clone()))?;
                establishment
            },
            ended: ended_counter(&registry, "client")?,
            bytes: byte_counter(&registry, "client")?,
            registry,
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// A wallet connection was accepted. The returned guard keeps the in-flight gauge honest even
    /// if the connection ends by unwinding.
    pub fn connection_accepted(&self) -> InFlight<'_> {
        self.connections.inc();
        self.in_flight.inc();
        InFlight {
            gauge: &self.in_flight,
        }
    }

    /// A dial that ended holding a stream, `accepted` being when the connection it will carry
    /// arrived.
    ///
    /// The wait is timed from there rather than from the round that answered. A round that finds
    /// nothing costs a whole probe deadline, and leaving those out would hide what retrying spends
    /// to reach the rate beside it.
    pub fn established(&self, rounds: &[Round], accepted: Instant) {
        self.dialled(rounds);
        self.establishment.observe(accepted.elapsed().as_secs_f64());
    }

    /// A dial that ran out of attempts, so the wallet gets a closed socket.
    pub fn gave_up(&self, rounds: &[Round]) {
        self.dialled(rounds);
        self.unestablished.inc();
    }

    /// How many connections have ended with nothing, one after another.
    pub fn failed_in_a_row(&self, streak: usize) {
        self.failed_in_a_row.set(streak as i64);
    }

    /// What every dial costs, whichever way it ended.
    ///
    /// The transport's rate is counted here and the wallet's by the method that called this, both
    /// off the same dial. That is what makes the two comparable rather than two measurements of two
    /// different moments.
    fn dialled(&self, rounds: &[Round]) {
        self.rounds.inc_by(rounds.len() as u64);
        for round in rounds {
            self.streams_opened.inc_by(u64::from(round.opened));
            for discarded in &round.discarded {
                self.streams_discarded
                    .with_label_values(&[discard_reason(&discarded.error)])
                    .inc();
            }
        }
        if rounds.first().is_some_and(|round| !round.answered) {
            self.first_round_failures.inc();
        }
    }

    /// A carried connection finished.
    pub fn finished(&self, transfer: &Transfer, ended: &Ended) {
        record_transfer(&self.bytes, transfer);
        self.ended.with_label_values(&[ended_reason(ended)]).inc();
    }
}

/// What the listening half counts.
pub struct ServerMetrics {
    registry: Registry,
    streams_accepted: IntCounter,
    streams_rejected: IntCounterVec,
    streams_without_request: IntCounter,
    in_flight: IntGauge,
    upstream_connections: IntCounter,
    upstream_failures: IntCounter,
    ended: IntCounterVec,
    bytes: IntCounterVec,
}

impl ServerMetrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        Ok(Self {
            streams_accepted: counter(
                &registry,
                "lwd_mixnet_server_streams_accepted_total",
                "Mixnet streams that introduced themselves with a header this build understands.",
            )?,
            streams_rejected: {
                let rejected = IntCounterVec::new(
                    Opts::new(
                        "lwd_mixnet_server_streams_rejected_total",
                        "Streams dropped before reaching the upstream, by why.",
                    ),
                    &["reason"],
                )?;
                registry.register(Box::new(rejected.clone()))?;
                rejected
            },
            streams_without_request: counter(
                &registry,
                "lwd_mixnet_server_streams_without_request_total",
                "Accepted streams let go without ever carrying a request. Mostly the siblings a \
                 dialler opened together and did not keep, so this tracks what its round size costs \
                 here.",
            )?,
            in_flight: gauge(
                &registry,
                "lwd_mixnet_server_connections_in_flight",
                "Streams currently spliced to the upstream.",
            )?,
            upstream_connections: counter(
                &registry,
                "lwd_mixnet_server_upstream_connections_total",
                "Connections opened to the configured upstream.",
            )?,
            upstream_failures: counter(
                &registry,
                "lwd_mixnet_server_upstream_failures_total",
                "Times the upstream could not be reached.",
            )?,
            ended: ended_counter(&registry, "server")?,
            bytes: byte_counter(&registry, "server")?,
            registry,
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn stream_accepted(&self) {
        self.streams_accepted.inc();
    }

    pub fn stream_rejected(&self, error: &HandshakeError) {
        self.streams_rejected
            .with_label_values(&[handshake_reason(error)])
            .inc();
    }

    /// A stream let go before its handshake was even read, because the cap was already full.
    pub fn stream_dropped_over_capacity(&self) {
        self.streams_rejected
            .with_label_values(&["over_capacity"])
            .inc();
    }

    pub fn stream_without_request(&self) {
        self.streams_without_request.inc();
    }

    pub fn upstream_unreachable(&self) {
        self.upstream_failures.inc();
    }

    /// The upstream accepted a connection and is about to be spliced.
    pub fn upstream_connected(&self) -> InFlight<'_> {
        self.upstream_connections.inc();
        self.in_flight.inc();
        InFlight {
            gauge: &self.in_flight,
        }
    }

    pub fn finished(&self, transfer: &Transfer, ended: &Ended) {
        record_transfer(&self.bytes, transfer);
        self.ended.with_label_values(&[ended_reason(ended)]).inc();
    }
}

/// Holds a gauge up for as long as the work it counts is running.
pub struct InFlight<'a> {
    gauge: &'a IntGauge,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.gauge.dec();
    }
}

/// Render a registry in the Prometheus text exposition format.
pub fn encode(registry: &Registry) -> Result<String, prometheus::Error> {
    TextEncoder::new().encode_to_string(&registry.gather())
}

fn record_transfer(bytes: &IntCounterVec, transfer: &Transfer) {
    bytes
        .with_label_values(&["to_mixnet"])
        .inc_by(transfer.to_remote);
    bytes
        .with_label_values(&["from_mixnet"])
        .inc_by(transfer.from_remote);
}

fn ended_reason(ended: &Ended) -> &'static str {
    match ended {
        Ended::Closed => "closed",
        Ended::Stalled(_) => "stalled",
        Ended::Idle(_) => "idle",
        Ended::Failed(_) => "failed",
    }
}

/// A stream the SDK refused to open is this machine failing, not the transport, and folding the two
/// together makes a degraded local client read as a degraded network.
fn discard_reason(error: &DialError) -> &'static str {
    match error {
        DialError::Open(_) => "refused_open",
        DialError::Probe(probe) => handshake_reason(probe),
    }
}

fn handshake_reason(error: &HandshakeError) -> &'static str {
    match error {
        HandshakeError::TimedOut(_) => "unanswered",
        HandshakeError::Read(_) | HandshakeError::Write(_) => "io",
        HandshakeError::ForeignStream => "foreign",
        HandshakeError::UnsupportedVersion(_) => "unsupported_version",
        HandshakeError::TokenMismatch { .. } => "token_mismatch",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn answering_round() -> Round {
        Round {
            opened: 1,
            discarded: Vec::new(),
            answered: true,
        }
    }

    fn unanswered_round() -> Round {
        Round {
            opened: 1,
            discarded: vec![crate::dial::Discarded {
                elapsed: Duration::from_secs(10),
                error: HandshakeError::TimedOut(Duration::from_secs(10)).into(),
            }],
            answered: false,
        }
    }

    fn value_of(registry: &Registry, needle: &str) -> f64 {
        registry
            .gather()
            .iter()
            .filter(|family| family.name() == needle)
            .flat_map(|family| family.get_metric())
            .map(|metric| metric.get_counter().value() + metric.get_gauge().value())
            .sum()
    }

    /// What the establishment histogram was told, in seconds.
    fn establishment_seconds(metrics: &ClientMetrics) -> f64 {
        metrics
            .registry()
            .gather()
            .iter()
            .filter(|family| family.name() == "lwd_mixnet_client_establishment_seconds")
            .flat_map(|family| family.get_metric())
            .map(|metric| metric.get_histogram().get_sample_sum())
            .sum()
    }

    /// The two rates the pair is made of, for one dial.
    fn rates(metrics: &ClientMetrics) -> (f64, f64) {
        (
            value_of(
                metrics.registry(),
                "lwd_mixnet_client_first_round_failures_total",
            ),
            value_of(
                metrics.registry(),
                "lwd_mixnet_client_connections_unestablished_total",
            ),
        )
    }

    #[test]
    fn a_connection_that_needed_a_retry_counts_against_the_transport_but_not_against_the_wallet() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.established(&[unanswered_round(), answering_round()], Instant::now());

        assert_eq!(rates(&metrics), (1.0, 0.0));
    }

    #[test]
    fn a_connection_no_stream_answered_counts_against_both() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.gave_up(&[unanswered_round(), unanswered_round()]);

        assert_eq!(rates(&metrics), (1.0, 1.0));
    }

    #[test]
    fn the_streak_gauge_reads_back_what_it_was_told() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.failed_in_a_row(3);

        assert_eq!(
            value_of(
                metrics.registry(),
                "lwd_mixnet_client_connections_failed_in_a_row"
            ),
            3.0
        );
    }

    #[test]
    fn a_connection_that_works_puts_the_streak_back_to_zero() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.failed_in_a_row(19);
        metrics.failed_in_a_row(0);

        assert_eq!(
            value_of(
                metrics.registry(),
                "lwd_mixnet_client_connections_failed_in_a_row"
            ),
            0.0
        );
    }

    #[test]
    fn a_connection_that_answered_first_time_counts_against_neither() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.established(&[answering_round()], Instant::now());

        assert_eq!(rates(&metrics), (0.0, 0.0));
    }

    /// One round spent its whole 10 s probe deadline finding nothing, and the next answered in a
    /// second. The wallet waited the eleven.
    #[tokio::test(start_paused = true)]
    async fn establishment_holds_the_rounds_that_found_nothing() {
        let metrics = ClientMetrics::new().unwrap();
        let accepted = Instant::now();
        tokio::time::advance(Duration::from_secs(11)).await;

        metrics.established(&[unanswered_round(), answering_round()], accepted);

        assert_eq!(establishment_seconds(&metrics), 11.0);
    }

    #[test]
    fn a_stream_that_went_unanswered_is_labelled_as_such() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.gave_up(&[unanswered_round()]);

        let encoded = encode(metrics.registry()).unwrap();
        assert!(
            encoded.contains("lwd_mixnet_client_streams_discarded_total{reason=\"unanswered\"} 1"),
            "{encoded}"
        );
    }

    #[test]
    fn the_in_flight_gauge_comes_back_down_when_the_connection_ends() {
        let metrics = ClientMetrics::new().unwrap();
        drop(metrics.connection_accepted());

        assert_eq!(
            value_of(
                metrics.registry(),
                "lwd_mixnet_client_connections_in_flight"
            ),
            0.0
        );
    }

    #[test]
    fn a_stalled_connection_is_distinguishable_from_one_that_closed() {
        let metrics = ClientMetrics::new().unwrap();
        metrics.finished(
            &Transfer::default(),
            &Ended::Stalled(Duration::from_secs(60)),
        );

        let encoded = encode(metrics.registry()).unwrap();
        assert!(
            encoded.contains("lwd_mixnet_client_connections_ended_total{reason=\"stalled\"} 1"),
            "{encoded}"
        );
    }

    #[test]
    fn a_stream_shed_at_the_cap_is_told_apart_from_one_that_failed_its_handshake() {
        let metrics = ServerMetrics::new().unwrap();
        metrics.stream_dropped_over_capacity();

        let encoded = encode(metrics.registry()).unwrap();
        let expected = "lwd_mixnet_server_streams_rejected_total{reason=\"over_capacity\"} 1";

        assert!(encoded.contains(expected), "{encoded}");
    }

    #[test]
    fn both_halves_register_their_metrics_without_colliding() {
        assert!(ClientMetrics::new().is_ok() && ServerMetrics::new().is_ok());
    }
}
