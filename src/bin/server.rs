//! Runs next to a lightwalletd. Accepts mixnet streams and splices each one to an upstream.
//!
//! The upstream is whatever the operator points it at, so this serves any implementation of the
//! light-client protocol without knowing which one it is.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use lwd_mixnet_proxy::endpoint;
use lwd_mixnet_proxy::handshake;
use lwd_mixnet_proxy::health::{Health, State};
use lwd_mixnet_proxy::metrics::ServerMetrics;
use lwd_mixnet_proxy::shutdown;
use lwd_mixnet_proxy::splice::{self, Watchdog};
use nym_sdk::mixnet::{MixnetClient, MixnetClientBuilder, MixnetStream, StoragePaths};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

/// Big enough for a gRPC client's opening burst, so the first write upstream is a single one.
const FIRST_CHUNK: usize = 8 * 1024;

#[derive(Parser)]
#[command(
    about = "Serve an upstream light-client endpoint over the mixnet",
    version
)]
struct Arguments {
    /// Endpoint every accepted stream is spliced to.
    #[arg(long, env = "LWD_MIXNET_UPSTREAM", default_value = "127.0.0.1:9067")]
    upstream: String,

    /// Directory holding the client identity. Without it the Nym address is ephemeral and rotates
    /// on every restart, which makes this half unreachable by anyone who wrote the address down.
    ///
    /// The directory holds private keys: losing it changes the address, copying it allows
    /// impersonation.
    #[arg(long, env = "LWD_MIXNET_STATE_DIR")]
    state_dir: Option<PathBuf>,

    /// How long an accepted stream has to say who it is. This is what a stream whose payload never
    /// arrived costs: without it, the SDK holds one for half an hour.
    #[arg(long, env = "LWD_MIXNET_HANDSHAKE_TIMEOUT_SECS", default_value_t = 30)]
    handshake_timeout_secs: u64,

    /// How long a stream that introduced itself has to send a request before it is let go.
    ///
    /// Short on purpose, and separate from the idle deadline below. A dialler that opens several
    /// streams at once and keeps the first to answer leaves the rest here, introduced and silent:
    /// one measured run left 300 of them behind across 150 connections. Holding those for the idle
    /// deadline would mean carrying two dead streams per connection for ten minutes.
    #[arg(
        long,
        env = "LWD_MIXNET_FIRST_REQUEST_TIMEOUT_SECS",
        default_value_t = 60
    )]
    first_request_timeout_secs: u64,

    /// How long a stream carrying a connection may go without moving a byte before it is let go.
    /// The transport carries no close, so a dialler that walked away is only ever noticed by this
    /// timer.
    #[arg(long, env = "LWD_MIXNET_IDLE_TIMEOUT_SECS", default_value_t = 600)]
    idle_timeout_secs: u64,

    /// How many streams may be held at once. Each one costs a task, and each that gets as far as a
    /// request costs a connection to the upstream too.
    ///
    /// This transport carries no address to rate-limit and no identity to ban, so a flooder cannot
    /// be told apart from a crowd: a cap on how much can be held at once is the only control this
    /// half has. Streams arriving above it are dropped unread, and their dialler learns that the
    /// way it learns everything here, by its own deadline.
    #[arg(long, env = "LWD_MIXNET_MAX_STREAMS", default_value = "256")]
    max_streams: NonZeroUsize,

    /// Where to serve `/metrics` and `/health`. Unset means neither is served.
    ///
    /// The failure rate this half meets moves by an order of magnitude between one hour and the
    /// next, so an operator without these has no way to tell a bad afternoon on the transport from
    /// a deployment that is actually broken.
    #[arg(long, env = "LWD_MIXNET_METRICS_BIND")]
    metrics_bind: Option<String>,

    /// How long streams already being carried are given to finish once a shutdown is asked for.
    #[arg(long, env = "LWD_MIXNET_SHUTDOWN_GRACE_SECS", default_value_t = 10)]
    shutdown_grace_secs: u64,

    /// How long startup may spend waiting for the registered gateway before this half gives up and
    /// exits.
    ///
    /// This half asks the SDK to wait out a gateway that is briefly unbonded rather than fail
    /// startup, and the SDK's own deadline for that is 70 minutes. One gateway was out of the
    /// topology for 23 hours and then came back, which is far past brief and still recoverable:
    /// waiting 70 minutes per attempt only makes the outage look like a process coming up.
    #[arg(long, env = "LWD_MIXNET_GATEWAY_WAIT_SECS", default_value_t = 300)]
    gateway_wait_secs: u64,
}

#[derive(Clone)]
struct Settings {
    upstream: String,
    handshake_timeout: Duration,
    first_request_timeout: Duration,
    idle_timeout: Duration,
    metrics: Arc<ServerMetrics>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs go to stderr so stdout carries nothing but the address line below.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let arguments = Arguments::parse();

    let settings = Settings {
        upstream: arguments.upstream.clone(),
        handshake_timeout: Duration::from_secs(arguments.handshake_timeout_secs),
        first_request_timeout: Duration::from_secs(arguments.first_request_timeout_secs),
        idle_timeout: Duration::from_secs(arguments.idle_timeout_secs),
        metrics: Arc::new(ServerMetrics::new().context("building the metrics")?),
    };
    let health = Health::starting();

    // Bound before the mixnet client connects, which takes seconds and does not always succeed:
    // without it there is nothing to ask why startup is taking so long.
    let (shutting_down, shutdown) = tokio::sync::watch::channel(false);
    if let Some(bind) = &arguments.metrics_bind {
        let listener = TcpListener::bind(bind)
            .await
            .with_context(|| format!("binding the metrics endpoint on {bind}"))?;
        let registry = settings.metrics.registry().clone();
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

    let gateway_wait = Duration::from_secs(arguments.gateway_wait_secs);
    let connecting = connect(&arguments, settings.idle_timeout);
    let mut client = match tokio::time::timeout(gateway_wait, connecting).await {
        Ok(connected) => connected?,
        Err(_) => {
            tracing::error!(
                waited_secs = arguments.gateway_wait_secs,
                "gave up connecting to the mixnet. the lines above name the gateway this half is \
                 registered with: if they say it is not online, it is out of the topology. it may \
                 return, and every restart tries again; if it does not, re-registering picks \
                 another gateway and changes the published address"
            );
            anyhow::bail!("connecting to the mixnet: no gateway within {gateway_wait:?}");
        }
    };
    health.advance_to(State::Registered);

    // Printed rather than logged so it survives whatever the log filter is set to: an operator
    // cannot configure the dialling half without it.
    let address = *client.nym_address();
    println!("NYM_ADDRESS={address}");

    let mut listener = client.listener().context("taking the stream listener")?;
    health.advance_to(State::Serving);
    tracing::info!(
        %address,
        upstream = %settings.upstream,
        handshake_timeout_secs = arguments.handshake_timeout_secs,
        first_request_timeout_secs = arguments.first_request_timeout_secs,
        idle_timeout_secs = arguments.idle_timeout_secs,
        max_streams = arguments.max_streams.get(),
        "accepting mixnet streams"
    );

    let mut streams = JoinSet::new();
    let capacity = Arc::new(tokio::sync::Semaphore::new(arguments.max_streams.get()));
    // Created once rather than per iteration: a fresh future would register the handler again on
    // every pass through the loop.
    let asked_to_stop = shutdown::requested();
    tokio::pin!(asked_to_stop);
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Some(stream) => match Arc::clone(&capacity).try_acquire_owned() {
                    Ok(permit) => {
                        let settings = settings.clone();
                        streams.spawn(async move {
                            // Named so the block captures it: the permit has to outlive the stream
                            // rather than this arm.
                            let _permit = permit;
                            serve(stream, settings).await;
                        });
                    }
                    // Dropping the stream deregisters it, and the dialler finds out by its own
                    // deadline. Nothing here may await: this arm holds up the whole loop.
                    Err(_) => {
                        settings.metrics.stream_dropped_over_capacity();
                        tracing::warn!("dropping a stream over the concurrent-stream cap");
                    }
                },
                None => {
                    tracing::warn!("the mixnet listener closed");
                    let _ = shutting_down.send(true);
                    drain(streams, Duration::from_secs(arguments.shutdown_grace_secs)).await;
                    return Ok(());
                }
            },
            // Finished streams are reaped as they end, so the set holds what is in flight rather
            // than everything this process has ever served.
            Some(_) = streams.join_next(), if !streams.is_empty() => {}
            _ = &mut asked_to_stop => {
                let _ = shutting_down.send(true);
                drain(streams, Duration::from_secs(arguments.shutdown_grace_secs)).await;
                return Ok(());
            }
        }
    }
}

/// Let streams already in flight finish, then stop waiting.
///
/// A dialler recovers from a closed connection, so cutting one short is recoverable rather than
/// fatal. Waiting a little anyway is what keeps an ordinary restart from reaching a wallet.
async fn drain(mut streams: JoinSet<()>, grace: Duration) {
    if streams.is_empty() {
        tracing::info!("shutting down");
        return;
    }

    tracing::info!(
        in_flight = streams.len(),
        ?grace,
        "shutting down, letting streams in flight finish"
    );
    let drained = tokio::time::timeout(grace, async {
        while streams.join_next().await.is_some() {}
    })
    .await;

    if drained.is_err() {
        tracing::warn!(
            abandoned = streams.len(),
            "closing streams that did not finish in time"
        );
    }
}

async fn connect(arguments: &Arguments, idle_timeout: Duration) -> Result<MixnetClient> {
    match &arguments.state_dir {
        Some(directory) => {
            let paths = StoragePaths::new_from_dir(directory)
                .context("preparing the client state directory")?;
            MixnetClientBuilder::new_with_default_storage(paths)
                .await
                .context("building a client with persistent storage")?
                // A registered gateway that is temporarily unbonded should delay startup rather
                // than fail it: registration itself was observed to fail on 2 of 15 attempts. The
                // SDK takes a bool and waits 70 minutes on it, so the caller bounds it.
                .with_wait_for_gateway(true)
                .with_stream_idle_timeout(idle_timeout)
                .build()
                .context("assembling the client")?
                .connect_to_mixnet()
                .await
                .context("connecting to the mixnet")
        }
        None => MixnetClientBuilder::new_ephemeral()
            .with_stream_idle_timeout(idle_timeout)
            .build()
            .context("assembling an ephemeral client")?
            .connect_to_mixnet()
            .await
            .context("connecting an ephemeral client to the mixnet"),
    }
}

/// Splice one accepted stream to the upstream, once it has proven to be one of ours.
async fn serve(mut stream: MixnetStream, settings: Settings) {
    let stream_id = stream.id();

    if let Err(error) = handshake::accept(&mut stream, settings.handshake_timeout).await {
        settings.metrics.stream_rejected(&error);
        tracing::debug!(%stream_id, %error, "dropping a stream that never introduced itself");
        return;
    }
    settings.metrics.stream_accepted();

    // The upstream is not touched until the dialler sends something to send it. A stream that was
    // only probed, or one whose dialler kept a sibling instead, therefore never becomes a connection
    // to the node: it waits here, briefly, and is let go.
    let mut opening = vec![0u8; FIRST_CHUNK];
    let read = match tokio::time::timeout(settings.first_request_timeout, stream.read(&mut opening))
        .await
    {
        Ok(Ok(0)) | Err(_) => {
            settings.metrics.stream_without_request();
            tracing::debug!(%stream_id, "letting go of a stream that carried no request");
            return;
        }
        Ok(Ok(read)) => read,
        Ok(Err(error)) => {
            settings.metrics.stream_without_request();
            tracing::debug!(%stream_id, %error, "reading the first request failed");
            return;
        }
    };

    let mut upstream = match TcpStream::connect(&settings.upstream).await {
        Ok(connection) => connection,
        Err(error) => {
            settings.metrics.upstream_unreachable();
            tracing::error!(%stream_id, %error, upstream = %settings.upstream, "upstream unreachable");
            return;
        }
    };
    let _in_flight = settings.metrics.upstream_connected();
    if let Err(error) = upstream.write_all(&opening[..read]).await {
        tracing::warn!(%stream_id, %error, "writing the first request upstream failed");
        return;
    }

    let watchdog = Watchdog {
        // A completed response leaves the connection legitimately quiet, so a stalled-request
        // deadline would fire on healthy streams here. Only the reaper applies.
        stall: None,
        idle: Some(settings.idle_timeout),
    };
    let (transfer, ended) = splice::splice(upstream, stream, watchdog).await;
    settings.metrics.finished(&transfer, &ended);
    tracing::info!(
        %stream_id,
        from_client = transfer.from_remote + read as u64,
        to_client = transfer.to_remote,
        ?ended,
        "stream finished"
    );
}
