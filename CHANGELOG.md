# Changelog

All notable changes to this project are documented here. The format is loosely based on
[Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.1.2] - 2026-08-19

### Security
- `h2` moves to 0.4.17, which closes RUSTSEC-2026-0258: empty DATA frames were accepted and queued
  with no limit, so a peer could push the receiving side's memory up or overflow a counter. Low
  severity, and neither half of this proxy serves HTTP/2. The tree carries a second copy, `h2` 0.3.27,
  reachable only through the pinned SDK's Cosmos RPC client, where the fix does not exist: the 0.3
  line has none. That one is in the audit ignore list with its reason (ADR 0009).

## [0.1.1] - 2026-08-19

### Added
- The dialling half reports itself degraded after three dials in a row that carry nothing: `/health`
  gains `"degraded": true` and `lwd_mixnet_client_connections_failed_in_a_row` holds the streak. It
  clears on the first dial that answers. The status code and the process stay as they are, because a
  far side that is gone and a transport that is losing look the same from here, and restarting mends
  neither (ADR 0014). The case it exists for: through the 23 hour outage below, an operator dialling
  the public instance made 19 connections and opened 76 streams. None answered, and their half
  reported `serving` the whole time. `--no-probe` takes the signal with it, since nothing comes back
  from the far side before the wallet's own bytes.

### Changed
- `--probe-attempts` defaults to 6, which is two whole rounds at the default concurrency (ADR 0012).
  Four left a second round of a single stream, so a connection the transport had already failed once
  fell back to sequential retry. Failures between rounds are independent, which makes the budget an
  exponent on the rate a wallet sees: at the worst per-stream rate measured so far, 4.7% instead of
  13%. The extra streams open only when the first round found nothing.

### Fixed
- The serving half bounds how long startup waits for the gateway it registered with:
  `--gateway-wait-secs`, 300 by default, after which it says what it was waiting for and exits (ADR
  0013). The SDK's own deadline is 4200 s, which turns a gateway that has left the topology into a
  restart loop slow enough to read as a process still coming up. The public testnet instance spent 23
  hours in one on 2026-08-18, answering 503 on `/health` throughout. That gateway came back the next
  afternoon, so the loop is the recovery path as well, and bounding it tries 14 times as often.
  Re-registering stays a manual step, because it changes the published address. The README has the
  procedure.
- `lwd_mixnet_client_establishment_seconds` runs from the moment the connection is accepted. It
  recorded the round that answered, not the wait it is named after. Rounds that came up empty first
  went missing, each of them a whole probe deadline, so a connection the wallet waited 11 s for could
  land in a 1 s bucket. Histograms scraped from earlier builds undercount, so don't compare them with
  later ones. The pair of counters beside it is unaffected.

### Measurement
- A published Nym address is only as reachable as the gateway inside it. The public testnet serving
  half lost its gateway on 2026-08-18 and was unreachable for 23 hours. The node stayed bonded in the
  contract while absent from the API's active entry list, and returned the following afternoon. From
  the dialling side that looks exactly like a bad afternoon on the transport: an operator's
  heartbeat that night read 19 of 19 dials unestablished and took it for the network. Both sides,
  with raw logs, in `docs/measurements/2026-08-18-gateway-gone.md` and
  `docs/measurements/2026-08-19-midnight-heartbeat.md`.
- Midnight drains the gateway allowance and the client refills it by itself. The public testnet
  serving half ran 96 hours on one process, crossing four midnights: the allowance emptied inside the
  first second of 00:00 UTC every time, refilled in 202 to 525 ms. An operator running the dialling
  half elsewhere hit the same window on one of those nights and recovered the same way, so the
  schedule is not a property of this registration. Report, counters and raw logs in
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

[Unreleased]: https://github.com/jpgonzalezra/lwd-mixnet-proxy/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/jpgonzalezra/lwd-mixnet-proxy/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/jpgonzalezra/lwd-mixnet-proxy/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/jpgonzalezra/lwd-mixnet-proxy/releases/tag/v0.1.0
