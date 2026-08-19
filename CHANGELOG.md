# Changelog

All notable changes to this project are documented here. The format is loosely based on
[Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Changed
- `--probe-attempts` now defaults to 6 instead of 4, which is two whole rounds at the default
  concurrency (ADR 0012). Four left a second round of a single stream, so a connection the transport
  had already failed once fell back to sequential retry. Failures between rounds are independent, so
  the budget is the exponent on the rate a wallet sees: at the worst per-stream rate measured so far,
  4.7% instead of 13%. The extra streams open only when the first round found nothing.

### Added
- The dialling half now reports itself degraded after three connections in a row that carried
  nothing: `/health` gains `"degraded": true` and `lwd_mixnet_client_connections_failed_in_a_row`
  carries the streak. It clears on the first connection that gets through, and neither the status
  code nor the process changes, because a far side that is gone and a transport that is losing look
  the same from here and neither is mended by restarting (ADR 0014). Until now a half whose
  destination had disappeared sat at `serving`: an operator dialling the public instance during its
  23 hour outage took 19 connections, opened 76 streams, had none answered, and was told nothing.

### Fixed
- The serving half no longer waits out the SDK's 70 minute gateway deadline. A gateway that leaves
  the topology never comes back, so that wait turned a dead registration into a restart loop that
  reads as a process still starting up: the public testnet instance spent 23 hours in one on
  2026-08-18, answering 503 on `/health` the whole time. Startup now gives up after
  `--gateway-wait-secs` (300 by default) and exits, so the failure shows within minutes.
  Re-registering is left to the operator because it changes the published address; the README says
  how (ADR 0013).
- `lwd_mixnet_client_establishment_seconds` recorded the round that answered rather than the wait it
  is named after. Rounds that came up empty first were left out, each of them a whole probe deadline,
  so a connection the wallet waited 11 s for could land in a 1 s bucket. It now runs from the moment
  the connection is accepted. Histograms scraped from earlier builds undercount and do not compare
  with later ones. The pair of counters beside it is unaffected.

### Measurement
- The public testnet serving half ran 96 hours on one process. Its gateway allowance emptied inside
  the first second of 00:00 UTC on all four midnights it crossed, and the client refilled it by
  itself every time, in 202 to 525 ms. An operator running the dialling half elsewhere hit the same
  window on one of those nights and recovered the same way, so the schedule is not a property of this
  registration. Report, counters and raw logs in
  `docs/measurements/2026-08-15-daily-bandwidth-cliff.md`.
- The first dialling half measured by someone else. An operator pointed a client at the public
  testnet serving address from their own machine and published the counters: on the old
  four-attempt default it left 2 of 30 connections with nothing, the bench running beside it put
  the per-stream failure rate at 0.60, and the midnight allowance reset reached their gateway too.
  ADR 0012 rests on those numbers. Report and raw output in
  `docs/measurements/2026-08-15-external-client.md`.

## [0.1.0] - 2026-08-12

First public release (beta). Two binaries that carry a Zcash light wallet's gRPC connection over the
Nym mixnet without either end knowing: the wallet is pointed at a local port, and the light-client
server sees an ordinary client.

### Transport
- `lwd-mixnet-client` listens on TCP and splices each connection to a mixnet stream.
  `lwd-mixnet-server` accepts streams and splices them to a configured upstream. Neither half parses
  gRPC, so an HTTP/2 connection travels through unchanged. The upstream comes from configuration,
  which puts the serving half in front of any implementation of the light-client protocol.
- Every stream introduces itself with a 14-byte handshake. It doubles as the filter that keeps
  arbitrary mixnet traffic away from the upstream, so it is required even when probing is off.
- Probe-and-retry dialling (ADR 0003). The transport accepts a stream and then loses its first
  payload, often, with no error on either side. Both ends hang. Every stream has to answer a probe
  before the wallet's bytes go near it: the one that would have swallowed the first request swallows
  the probe instead.
- Streams open three at a time, keeping the first to answer (ADR 0006). A silent failure is only
  discovered when the deadline expires, so retrying one at a time pays a full deadline per failure:
  measured side by side, a p99 of 31.3 s against 6.3 s to establish.
- Stall and idle deadlines are the only thing that ends a conversation (ADR 0004). The transport
  carries no close, so a far side that stopped answering would otherwise hang forever. A stall closes
  the wallet's connection and hands its gRPC library an error it already knows how to retry.
- No pool of pre-probed streams (ADR 0006): a stream that answered a minute ago says nothing about
  whether it works now.

### Operations
- Prometheus `/metrics` and a three-state `/health`, on a port that stays closed unless
  `--metrics-bind` is given (ADR 0007). That port is bound before the mixnet client connects, since
  registering with a gateway is the slow part of startup and the part that fails.
- The counters report a pair of rates from the same dial: how often a freshly opened round goes
  unanswered, against how often the wallet is left with nothing. The transport's own rate swung by an
  order of magnitude across three days, so either number alone mostly records which afternoon it was
  taken on. Nothing exported or logged identifies a client.
- Both halves drain on `SIGINT` and on `SIGTERM`, finishing connections in flight within
  `--shutdown-grace-secs` before closing the rest.
- The dialling half exits with an error when its local mixnet client refuses every stream open across
  several connections in a row, since a restart is the only thing known to bring one back (ADR 0010).
- `--max-streams` bounds what the serving half holds at once, which is the only abuse control it has
  (ADR 0011).
- Every setting is a flag with a matching environment variable, and the flag wins. There is no
  configuration file: everything is a scalar, and a file would mostly add a precedence order to get
  wrong. `.env.example` ships the names and their defaults.

### Packaging
- One image for both halves, running as uid 10001 (ADR 0008). Neither needs root: they bind ports
  above 1024 and write nothing outside the state directory. The compose file runs each half as its
  own service, with a container health check on both.
- The SDK is pinned exactly and `Cargo.lock` is committed (ADR 0002). The measured behaviour this
  project is built around belongs to a specific release, so upgrading means measuring again.
- `cargo audit` runs weekly against that pinned tree and fails only on advisories it has not already
  reported (ADR 0009). `SECURITY.md` states the posture and how to report a vulnerability.
- CI runs rustfmt, clippy with warnings denied, the build and the tests on every push and pull
  request.

### Measurement
- `lwd-mixnet-bench` drives the same dialling code the client half uses, rather than a copy of it. It
  reports the raw per-stream failure rate next to the wallet-visible one from the same attempts, what
  establishing costs, and whether failures are independent enough for retrying to help at all
  (ADR 0005).
- What the 2026-08-06 run showed: the transport lost one stream in three (34.67%), and not one of the
  300 connections failed from the wallet's side, at a 6.3 s p99 to establish. That is one afternoon
  on one network path. Raw output from every run, including the two that had to be thrown out, is in
  `docs/measurements/`.

[Unreleased]: https://github.com/jpgonzalezra/lwd-mixnet-proxy/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jpgonzalezra/lwd-mixnet-proxy/releases/tag/v0.1.0
