//! Runs next to a wallet. Listens on a local TCP port and carries each connection over the mixnet.
//!
//! The wallet is pointed at the local port and needs no changes: what it gets is an ordinary TCP
//! connection that happens to be answered from far away, slowly.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use lwd_mixnet_proxy::dial::{self, DialSettings, ProbeSettings};
use lwd_mixnet_proxy::endpoint;
use lwd_mixnet_proxy::health::{Health, State};
use lwd_mixnet_proxy::metrics::ClientMetrics;
use lwd_mixnet_proxy::shutdown;
use lwd_mixnet_proxy::splice::{self, Watchdog};
use lwd_mixnet_proxy::streaks::{Outcome, Streaks};
use nym_sdk::mixnet::{MixnetClient, Recipient};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::Instant;

#[derive(Parser)]
#[command(about = "Reach a light-client endpoint over the mixnet", version)]
struct Arguments {
    /// Nym address the serving half printed on startup.
    #[arg(long, env = "LWD_MIXNET_SERVER")]
    server: String,

    /// Local address the wallet connects to. Loopback by default: this port is an unauthenticated
    /// door to the upstream.
    #[arg(long, env = "LWD_MIXNET_BIND", default_value = "127.0.0.1:9068")]
    bind: String,

    /// How long a freshly opened stream has to answer its probe before it is discarded. Round trips
    /// measured seconds with a long tail, so a tighter deadline throws away healthy streams.
    #[arg(long, env = "LWD_MIXNET_PROBE_TIMEOUT_SECS", default_value_t = 10)]
    probe_timeout_secs: u64,

    /// How many streams may be opened for one wallet connection, the first included.
    ///
    /// Six by default, two full rounds at the default concurrency (ADR 0012). Failures between
    /// rounds are independent, so the budget is the exponent on the rate a wallet sees: at a
    /// per-stream rate of 0.55, four attempts leave 9.2% of connections with nothing, six leave
    /// 2.8%. A budget that does not divide by the round size ends on a short round, which retries
    /// in series exactly when the transport is worst.
    #[arg(long, env = "LWD_MIXNET_PROBE_ATTEMPTS", default_value = "6")]
    probe_attempts: NonZeroU32,

    /// How many of those are opened at once. One retries in series, so each failure costs a whole
    /// probe timeout before the next attempt starts; more opens several and takes the first to
    /// answer.
    ///
    /// Three by default, because that is what measurement supports: interleaved against sequential
    /// retry under a 34.7% per-stream failure rate, rounds of three held establishment at a 6.3 s
    /// p99 where retrying in series reached 31.3 s. The cost is three streams and three reply-block
    /// budgets per connection instead of one.
    #[arg(long, env = "LWD_MIXNET_PROBE_CONCURRENCY", default_value = "3")]
    probe_concurrency: NonZeroU32,

    /// Hand the wallet the first stream that opens, without checking that it works. The probe
    /// exists because the transport loses first payloads silently; this is the switch for the day
    /// it stops.
    ///
    /// It also takes the only evidence the degraded signal runs on. Without a probe nothing comes
    /// back before the wallet's own bytes, so a dial can only report that a stream opened, and
    /// `/health` will not call this half degraded while opens keep succeeding (ADR 0014).
    #[arg(long, env = "LWD_MIXNET_NO_PROBE")]
    no_probe: bool,

    /// Reply blocks attached to each outbound message; unset leaves the SDK default of 10. Raising
    /// it lowers the failure rate without reaching zero, and costs latency.
    #[arg(long, env = "LWD_MIXNET_REPLY_SURBS")]
    reply_surbs: Option<u32>,

    /// How long a connection may wait for an answer that never comes before it is closed. Closing
    /// it is what turns a silent hang into an error the wallet's gRPC library reconnects from.
    #[arg(long, env = "LWD_MIXNET_STALL_TIMEOUT_SECS", default_value_t = 60)]
    stall_timeout_secs: u64,

    /// Where to serve `/metrics` and `/health`. Unset means neither is served: this half runs on
    /// the same machine as the wallet, so opening a port there should be the operator's call.
    #[arg(long, env = "LWD_MIXNET_METRICS_BIND")]
    metrics_bind: Option<String>,

    /// How long connections already being carried are given to finish once a shutdown is asked for.
    #[arg(long, env = "LWD_MIXNET_SHUTDOWN_GRACE_SECS", default_value_t = 10)]
    shutdown_grace_secs: u64,
}

/// What every connection is carried with, fixed at startup.
#[derive(Clone, Copy)]
struct Carrying {
    server: Recipient,
    dial: DialSettings,
    watchdog: Watchdog,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let arguments = Arguments::parse();

    let server: Recipient = arguments
        .server
        .parse()
        .context("parsing the server's Nym address")?;

    let dial_settings = DialSettings {
        reply_surbs: arguments.reply_surbs,
        probe: (!arguments.no_probe).then_some(ProbeSettings {
            timeout: Duration::from_secs(arguments.probe_timeout_secs),
            attempts: arguments.probe_attempts,
            concurrency: arguments.probe_concurrency,
        }),
    };
    let carrying = Carrying {
        server,
        dial: dial_settings,
        watchdog: Watchdog {
            stall: Some(Duration::from_secs(arguments.stall_timeout_secs)),
            idle: None,
        },
    };

    let metrics = Arc::new(ClientMetrics::new().context("building the metrics")?);
    let health = Health::starting();

    // Bound before the mixnet client connects, which takes seconds and sometimes fails: without it
    // there is nothing to ask why startup is taking so long.
    let observability = match &arguments.metrics_bind {
        Some(bind) => Some((
            TcpListener::bind(bind)
                .await
                .with_context(|| format!("binding the metrics endpoint on {bind}"))?,
            bind.clone(),
        )),
        None => None,
    };
    let (shutting_down, shutdown) = tokio::sync::watch::channel(false);
    if let Some((listener, bind)) = observability {
        let registry = metrics.registry().clone();
        let health = health.clone();
        let mut shutdown = shutdown.clone();
        tracing::info!(%bind, "serving /metrics and /health");
        tokio::spawn(async move {
            endpoint::serve(listener, registry, health, async move {
                let _ = shutdown.wait_for(|asked| *asked).await;
            })
            .await;
        });
    }

    // Deliberately ephemeral: a stable client identity is exactly what would let a server correlate
    // one wallet's requests across sessions.
    let client = MixnetClient::connect_new()
        .await
        .context("connecting to the mixnet")?;
    let client = Arc::new(Mutex::new(client));
    health.advance_to(State::Registered);

    let listener = TcpListener::bind(&arguments.bind)
        .await
        .with_context(|| format!("binding {}", arguments.bind))?;
    health.advance_to(State::Serving);
    tracing::info!(
        bind = %arguments.bind,
        probe = !arguments.no_probe,
        probe_attempts = arguments.probe_attempts,
        "carrying local connections over the mixnet"
    );

    let mut connections = JoinSet::new();
    let streaks = Arc::new(Streaks::new(health.clone(), Arc::clone(&metrics)));
    // Created once rather than per iteration: a fresh future would register the handler again on
    // every pass through the loop.
    let asked_to_stop = shutdown::requested();
    let gave_up_on_the_client = streaks.client_is_dead().notified();
    tokio::pin!(asked_to_stop, gave_up_on_the_client);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (connection, wallet) = match accepted {
                    Ok(accepted) => accepted,
                    // Accept fails for passing reasons — fd pressure, a connection aborted before
                    // it was picked up — and the wallet comes back. Dropping every connection in
                    // flight over one of them would be the wrong answer. The pause is what stops fd
                    // exhaustion from spinning this loop hot.
                    Err(error) => {
                        tracing::warn!(%error, "a local connection was not accepted");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let accepted = Instant::now();
                let client = Arc::clone(&client);
                let metrics = Arc::clone(&metrics);
                let streaks = Arc::clone(&streaks);
                connections.spawn(async move {
                    carry(connection, accepted, &client, carrying, &metrics, &streaks).await;
                    tracing::debug!(%wallet, "local connection done");
                });
            }
            // Finished connections are reaped as they end, so the set holds what is in flight
            // rather than everything this process has ever carried.
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            _ = &mut gave_up_on_the_client => {
                tracing::error!(
                    "the local mixnet client refused every open; a restart is the only recovery"
                );
                let _ = shutting_down.send(true);
                drain(connections, Duration::from_secs(arguments.shutdown_grace_secs)).await;
                anyhow::bail!("the local mixnet client stopped opening streams");
            }
            _ = &mut asked_to_stop => {
                let _ = shutting_down.send(true);
                drain(connections, Duration::from_secs(arguments.shutdown_grace_secs)).await;
                return Ok(());
            }
        }
    }
}

/// Let connections already in flight finish, then stop waiting.
///
/// A wallet's gRPC library reconnects from a closed connection, so cutting one short is recoverable
/// rather than fatal. Waiting a little anyway is what keeps an ordinary restart from showing up as
/// an error in someone's wallet.
async fn drain(mut connections: JoinSet<()>, grace: Duration) {
    if connections.is_empty() {
        tracing::info!("shutting down");
        return;
    }

    tracing::info!(
        in_flight = connections.len(),
        ?grace,
        "shutting down, letting connections in flight finish"
    );
    let drained = tokio::time::timeout(grace, async {
        while connections.join_next().await.is_some() {}
    })
    .await;

    if drained.is_err() {
        tracing::warn!(
            abandoned = connections.len(),
            "closing connections that did not finish in time"
        );
    }
}

/// Carry one wallet connection over a stream that has been shown to work.
///
/// `accepted` is when the wallet's connection arrived, which is where the metrics time
/// establishment from.
async fn carry(
    connection: TcpStream,
    accepted: Instant,
    client: &Mutex<MixnetClient>,
    carrying: Carrying,
    metrics: &ClientMetrics,
    streaks: &Streaks,
) {
    let _in_flight = metrics.connection_accepted();

    // The dial is where both streaks are told what happened, rather than the end of this function.
    // A connection carries for as long as the wallet keeps it, and one that closes after ten
    // minutes says nothing about whether the far side is answering now.
    let dialled = match dial::dial(client, carrying.server, carrying.dial).await {
        Ok(dialled) => {
            metrics.established(&dialled.rounds, accepted);
            // With the probe off, a stream counts as dialled once the announcement is written, and
            // nothing has come back from the far side. Calling that an answer would clear the run
            // this half is meant to notice.
            streaks.fold(match carrying.dial.probe {
                Some(_) => Outcome::Answered,
                None => Outcome::Unprobed,
            });
            dialled
        }
        Err(gave_up) => {
            metrics.gave_up(&gave_up.rounds);
            streaks.fold(match gave_up.nothing_opened() {
                true => Outcome::NothingOpened,
                false => Outcome::NothingAnswered,
            });
            // Dropping the connection is the point: the wallet sees a closed socket, which its gRPC
            // library knows how to retry, rather than a request that never returns.
            tracing::warn!(
                attempts = gave_up.attempts(),
                last_error = gave_up.last_error().map(|error| error.to_string()),
                "closing a local connection with no working stream to carry it"
            );
            return;
        }
    };

    if dialled.discarded() > 0 {
        tracing::info!(
            discarded = dialled.discarded(),
            rounds = dialled.rounds.len(),
            waited = ?accepted.elapsed(),
            answering_round = ?dialled.answering_round,
            "a stream answered after discarding streams that did not"
        );
    }

    let (transfer, ended) = splice::splice(connection, dialled.stream, carrying.watchdog).await;
    metrics.finished(&transfer, &ended);
    tracing::info!(
        sent = transfer.to_remote,
        received = transfer.from_remote,
        ?ended,
        "connection finished"
    );
}
