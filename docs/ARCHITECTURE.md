# Architecture

This is a living document. It describes what `lwd-mixnet-proxy` is, how bytes flow through it, and
what each module is responsible for.

## Mental model

Two processes, each a byte pipe, with the Nym mixnet between them:

```
  wallet  --TCP-->  [lwd-mixnet-client]  --mixnet-->  [lwd-mixnet-server]  --TCP-->  lightwalletd
        127.0.0.1:9068                                              127.0.0.1:9067
```

**Neither half understands gRPC**, and neither needs to. A mixnet stream implements
`AsyncRead + AsyncWrite`, so an HTTP/2 connection travels through unmodified: the wallet is pointed at
a local port and sees an ordinary TCP connection, while the server sees an ordinary client. The
serving half takes its upstream from configuration, so it works in front of any implementation of the
light-client protocol.

What the halves add is a **deadline**.

## Why there is a deadline

The transport loses a stream's first payload, often and silently: the stream opens, the far side
accepts it, `write_all` and `flush` both return `Ok`, and nothing arrives. Neither end errors and
neither times out. Measured over three days, the rate moved between 2% and 51%, and it does not settle.

An error is something a gRPC library can act on. Silence is not, so the job here is turning a hang
into either an invisible retry or a fast error. Most of what follows is downstream of that.

## The two halves

**`lwd-mixnet-client`** (`src/bin/client.rs`) runs next to the wallet. It listens on a local TCP port
and, for each connection, opens a mixnet stream that has been shown to work before splicing the two
together. Its mixnet identity is ephemeral on purpose: a stable one would let a server correlate a
wallet's requests across sessions.

**`lwd-mixnet-server`** (`src/bin/server.rs`) runs next to the server. It accepts mixnet streams,
requires each to introduce itself, and splices it to a configured TCP upstream. Its identity is
persistent, because the address is derived from keys on disk and clients have to be able to find it.
That directory is private key material.

**`lwd-mixnet-bench`** (`src/bin/bench.rs`) is the measurement, and it drives the same `dial` the
client half uses rather than a copy of it. What a run has to show is in
[0005](decisions/0005-what-the-measurement-has-to-show.md); what the runs showed is in
[`measurements/`](measurements/README.md).

## The library

`src/lib.rs` holds what both halves need and what deserves tests.

**`handshake`** is a 14-byte header: a magic, a version, a flag, and a token. The dialling half sends
it; the listening half echoes it when asked. This is the probe: the same round trip the transport
loses, run while nothing of the wallet's is at stake yet. The header is mandatory even when the probe
is off, because it is also how the listening half tells one of our streams from arbitrary mixnet
traffic that would otherwise reach the upstream.

**`dial`** opens streams until one answers, grouped into **rounds** of three by default. A round of one
is a sequential retry, where each failure costs a whole deadline before the next attempt starts; a
round of several opens them at once and takes the first to answer, turning that sum into a minimum at
the cost of reply blocks. Measured side by side, that is the difference between a 31.3 s and a 6.3 s
p99 to establish. It returns every round and every discarded stream to the caller: the gap between how
often a stream fails and how often a wallet notices is what the metrics and the benchmark are both
built on.

**`splice`** copies bytes both ways under two timers. **Stall** means the far side was written to and
has answered nothing since, which the dialling half uses to close the local connection and turn a hang
into a reconnect. **Idle** means nothing has moved at all, which the listening half uses to let go of
streams whose dialler is gone. The transport carries no close, so nothing else ends a dead
conversation.

**`metrics`** holds what each half counts. The number that matters is a **pair**, both taken from the
same dial: how often a freshly opened round goes unanswered, against how often the wallet is left with
nothing. Why it has to be two numbers is
[0007](decisions/0007-report-a-pair-of-rates-over-an-endpoint-that-is-off-by-default.md).

**`health`** is the three states a half moves through: `starting`, `registered`, `serving`. Startup is
neither instant nor reliable, so a binary up/down cannot tell "still registering" from "registered and
broken".

**`endpoint`** serves both over HTTP, on a port that is only opened when one is configured.

**`shutdown`** waits for either signal a stop can arrive as. Without `SIGTERM` the drain below is
unreachable in a container.

## Observability

Both halves take `--metrics-bind`, and neither has a default: the dialling half runs on a wallet's
machine, and the listening half is someone's server. Where one is given, `/metrics` carries the
Prometheus exposition format and `/health` a small JSON body, answering 200 only once the half is
serving. That port is bound **before** the mixnet client connects, because connecting is the slow,
failure-prone part of startup, and that is when someone wants to ask what is going on.

Nothing exported or logged identifies a client: the counters count streams and connections, and the
stream identifiers that appear in logs are random per stream. The endpoint is unauthenticated and
belongs on loopback or a private network.

Both halves drain on `SIGINT` and on `SIGTERM`, letting connections already in flight finish within
`--shutdown-grace-secs` before closing what remains. Both, because the target deployment is a
container and a runtime asks with `SIGTERM`: listening for an interrupt alone means the grace period
is never reached, and every stop is a kill.

## What is deliberately not here

- **No retry once bytes are moving.** Resuming would mean rebuilding HTTP/2 state and replaying
  requests already in flight, and a request delivered twice is worse than one that failed cleanly.
- **No gRPC parsing**, and therefore no per-method routing. Which calls are worth carrying over a
  mixnet is the wallet's decision, not this proxy's.
- **No pool of pre-probed streams.** A stream that answered a probe a minute ago is not a stream that
  works now, and the transport gives no way to tell the difference.
- **No upstream connection until a request arrives.** A stream that was only probed, or one whose
  dialler kept a sibling from the same round instead, never becomes a connection to the node, and is
  let go under a deadline of its own.

## Design decisions

| ADR | Decision |
|---|---|
| [0001](decisions/0001-its-own-repository.md) | Ship this as its own repository, as two binaries |
| [0002](decisions/0002-pin-the-sdk-and-ship-a-container.md) | Pin the SDK exactly, commit the lockfile, ship a container |
| [0003](decisions/0003-probe-every-stream-before-the-wallet-uses-it.md) | Probe every stream before the wallet is allowed near it |
| [0004](decisions/0004-deadlines-are-the-only-close.md) | Deadlines are the only close |
| [0005](decisions/0005-what-the-measurement-has-to-show.md) | What the measurement has to show, and how it is taken |
| [0006](decisions/0006-no-pool-of-pre-probed-streams.md) | No pool of pre-probed streams |
| [0007](decisions/0007-report-a-pair-of-rates-over-an-endpoint-that-is-off-by-default.md) | Report a pair of rates, over an endpoint that is off by default |
| [0008](decisions/0008-run-as-a-fixed-unprivileged-uid.md) | Run as a fixed unprivileged uid |
| [0009](decisions/0009-watch-advisories-against-a-pinned-tree.md) | Watch advisories against a pinned tree |
| [0010](decisions/0010-exit-when-the-local-client-degrades.md) | Exit when the local client degrades |
| [0011](decisions/0011-capacity-is-the-only-abuse-control.md) | Capacity is the only abuse control |
| [0012](decisions/0012-spend-the-attempt-budget-in-whole-rounds.md) | Spend the attempt budget in whole rounds |
| [0013](decisions/0013-bound-the-wait-for-a-registered-gateway.md) | Bound the wait for a registered gateway |
| [0014](decisions/0014-say-degraded-rather-than-exit-when-nothing-answers.md) | Say degraded rather than exit when nothing answers |
