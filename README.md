# lwd-mixnet-proxy

[![ci](https://github.com/jpgonzalezra/lwd-mixnet-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/jpgonzalezra/lwd-mixnet-proxy/actions/workflows/ci.yml)

Carry a Zcash light wallet's gRPC connection over the [Nym](https://nym.com) mixnet, without changing
the wallet or the server.

Two processes, each a byte pipe:

```
  wallet  --TCP-->  [lwd-mixnet-client]  --mixnet-->  [lwd-mixnet-server]  --TCP-->  lightwalletd
```

Neither half understands gRPC. A mixnet stream implements `AsyncRead + AsyncWrite`, so the connection
travels through unmodified: the wallet is pointed at a local port, and the server sees an ordinary
client. The serving half takes its upstream from configuration, so it works in front of any
implementation of the light-client protocol.

> **Status: early.** The core mechanism is implemented, unit-tested, and measured against the live
> mixnet: under a 34.67% per-stream failure rate it reduced what a wallet sees to 0 of 300
> connections, with a 6.3 s p99 to establish. That is one afternoon on one network path.

## Why

A light wallet reveals more to a server than the protocol suggests, and the leak is roughly inverse to
the bandwidth: bulk block download is the heaviest call and the least revealing, because the client
fetches everything and trial-decrypts locally, while the cheapest calls are the ones worth protecting.
Submitting a transaction links it to a network identity. Transparent-address queries hand over the
addresses directly.

A mixnet routes each packet separately through several relays that delay and reorder traffic, so it
resists the timing correlation a low-latency overlay does not. That makes it a good fit for exactly
the calls that leak the most and cost the fewest bytes.

## What this is really for

One defect decides the whole design: **a stream can open, be accepted by the far side, and never
deliver its first payload.** Neither end errors and neither times out; both hang. The rate measured
between 2% and 51% over three days and is not stationary.

Nothing is lost when that happens. The first `Data` frame overtakes the `Open` that registers its
stream, and the `nym-sdk` releases this project pins discard whatever arrives for a stream they do
not yet know about. Nym fixed it on `develop` in August 2026, and it is in no release yet, so every
release still behaves this way.
[The measurement](docs/measurements/2026-08-24-reordering-not-loss.md) has the evidence, and
[the one after it](docs/measurements/2026-08-25-six-thousand-trials.md) found nothing lost in 6,237
trials on the fixed tree.

gRPC libraries recover from errors. They do not recover from silence. So this proxy exists to convert
that hang into an invisible retry, and in the worst case into a fast error:

- Before the wallet sends a byte, the dialling half **probes** each stream and discards any that does
  not answer within a deadline. The probe is the same round trip that gets dropped, so a stream
  that would have swallowed the wallet's first request swallows the probe instead.
- Streams are opened **three at a time**, keeping the first to answer. Since a silent failure is only
  discovered when the deadline expires, retrying one at a time pays that deadline per failure and
  drags the tail out: measured side by side, that difference was a p99 of 31.3 s against 6.3 s.
- Once bytes are moving, a **watchdog** closes a connection whose far side stopped answering. The
  request in flight is lost, and the wallet gets a closed socket, which its gRPC library already
  knows how to retry.

## Use it for the small calls, not for syncing

At the measured throughput a full historical sync would move tens of gigabytes over a transport whose
median round trip is seconds, which is on the order of days. This is not a drop-in replacement for an
ordinary connection.

**Worth carrying:** transaction submission, transparent-address queries, single transaction lookups.
High leak, few bytes, latency-insensitive.

**Not worth carrying:** bulk block download, unless the wallet's birthday is recent.

**Do not sync and submit through the same server.** An operator that sees an address synchronising and
moments later receives an anonymous transaction can correlate the two by timing, and with few
concurrent users the anonymity set is negligible. Using a different instance for each costs nothing.

## Running

Both halves need to reach the mixnet, and the order is fixed: the serving half prints the address the
dialling half is configured with, and it only knows it once it has registered with a gateway.

### With containers

Both halves ship from one image. The compose file runs each as its own service and needs no edit for
the common case; `.env` carries what changes.

```
cp .env.example .env

docker compose up -d server
docker compose logs -f server        # NYM_ADDRESS=<identity>.<encryption>@<gateway>, after ~5 s
```

Put that address in `.env` as `SERVER_ADDRESS`, and set `UPSTREAM` if the light-client server is not
on the container host at `:9067`. Then:

```
docker compose up -d client
```

If the light-client server is itself a container, the serving half can join its network and reach it
by service name, so nothing has to be published to the host at all:

```yaml
services:
  server:
    environment:
      LWD_MIXNET_UPSTREAM: "lwd-rs-testnet:9070"
    networks: [upstream]

networks:
  upstream:
    name: <that stack's network>
    external: true
```

Point the wallet at `127.0.0.1:9068`. Both halves answer `/health` and are healthy once serving:

```
docker compose ps                    # both `healthy`
curl -s localhost:9070/metrics       # the dialling half; the serving half is on 9069
```

The processes run as **uid 10001**, unprivileged. An empty named volume inherits the ownership of
`/state` from the image, so the default compose file works as it stands. Two cases need a hand:

- **A bind mount instead of the named volume** starts as whatever the host directory is owned by, and
  the serving half cannot write to it: `chown 10001:10001` that directory first.
- **A volume from an older image that ran as root** stays owned by root. `docker compose down -v`
  discards it, at the cost of the identity in it: the Nym address changes and whoever wrote the old
  one down can no longer reach this half.

### As binaries

```
# next to the server
lwd-mixnet-server --upstream 127.0.0.1:9067 --state-dir /var/lib/lwd-mixnet
# NYM_ADDRESS=<identity>.<encryption>@<gateway>

# next to the wallet
lwd-mixnet-client --server <that address> --bind 127.0.0.1:9068
```

Then point the wallet at `127.0.0.1:9068`.

Every flag has an environment variable, listed in `--help`. The ones that matter:

| flag | what it trades |
|---|---|
| `--probe-timeout-secs` | Healthy round trips have a long tail, so a short deadline discards working streams; a long one makes each failure expensive. There is no value that is good at both. |
| `--probe-attempts` | Total streams one connection may open before giving up, six by default. Failures between rounds are independent, so this is the exponent on the rate a wallet sees. That rate moves by an order of magnitude between afternoons. Keep it a multiple of `--probe-concurrency`: a budget that ends on a short round retries in series just when the transport is worst. |
| `--probe-concurrency` | Streams opened at once, three by default. One retries in series and pays a deadline per failure; three keeps the tail short at the cost of three streams and three reply-block budgets per connection. |
| `--reply-surbs` | Raising it lowers the failure rate and costs latency. It will not get you to zero. |
| `--stall-timeout-secs` | How long the wallet waits on an answer that is not coming before the connection is closed. |
| `--metrics-bind` | Where to serve `/metrics` and `/health`. No default on either half: see below. |

### All of it

Every setting is a flag with a matching environment variable, and the flag wins. There is no
configuration file: everything here is a scalar, and a file would mostly add a precedence order to get
wrong. [`.env.example`](.env.example) is a template with these names and defaults already in it.

**Both halves:**

| variable | flag | default |
|---|---|---|
| `LWD_MIXNET_METRICS_BIND` | `--metrics-bind` | unset, so nothing is served |
| `LWD_MIXNET_SHUTDOWN_GRACE_SECS` | `--shutdown-grace-secs` | `10` |

Those two names are **shared by both binaries**. Running both halves on one machine with
`LWD_MIXNET_METRICS_BIND` exported means the second to start exits immediately with `Address already
in use`; pass `--metrics-bind` per process instead.

**`lwd-mixnet-client`:**

| variable | flag | default |
|---|---|---|
| `LWD_MIXNET_SERVER` | `--server` | **required** |
| `LWD_MIXNET_BIND` | `--bind` | `127.0.0.1:9068` |
| `LWD_MIXNET_PROBE_TIMEOUT_SECS` | `--probe-timeout-secs` | `10` |
| `LWD_MIXNET_PROBE_ATTEMPTS` | `--probe-attempts` | `6` |
| `LWD_MIXNET_PROBE_CONCURRENCY` | `--probe-concurrency` | `3` |
| `LWD_MIXNET_NO_PROBE` | `--no-probe` | off |
| `LWD_MIXNET_REPLY_SURBS` | `--reply-surbs` | unset, leaving the SDK's 10 |
| `LWD_MIXNET_STALL_TIMEOUT_SECS` | `--stall-timeout-secs` | `60` |

**`lwd-mixnet-server`:**

| variable | flag | default |
|---|---|---|
| `LWD_MIXNET_UPSTREAM` | `--upstream` | `127.0.0.1:9067` |
| `LWD_MIXNET_STATE_DIR` | `--state-dir` | unset, so the identity is ephemeral |
| `LWD_MIXNET_HANDSHAKE_TIMEOUT_SECS` | `--handshake-timeout-secs` | `30` |
| `LWD_MIXNET_FIRST_REQUEST_TIMEOUT_SECS` | `--first-request-timeout-secs` | `60` |
| `LWD_MIXNET_IDLE_TIMEOUT_SECS` | `--idle-timeout-secs` | `600` |
| `LWD_MIXNET_MAX_STREAMS` | `--max-streams` | `256` |
| `LWD_MIXNET_GATEWAY_WAIT_SECS` | `--gateway-wait-secs` | `300` |

An unset `--state-dir` means the Nym address changes on every restart, so nobody who wrote it down
can reach this half again. Anything long-lived wants one.

`RUST_LOG` sets the log filter on both. `info` is quiet; `debug` shows per-stream detail, including
which streams the probe discarded.

### Watching it run

Neither half opens a metrics port unless told to, so a deployment that sets nothing is flying blind.
On this transport that is worse than it sounds, and the reason is the next paragraph.

```
lwd-mixnet-server --upstream 127.0.0.1:9067 --metrics-bind 127.0.0.1:9069
curl -s localhost:9069/metrics
curl -s localhost:9069/health     # {"state":"serving"}, 200 only once it is
```

**Read two numbers, never one.** The rate at which streams fail to establish swung by more than an
order of magnitude across three days, so any single rate mostly records which afternoon it was
measured on.
What separates a bad afternoon from a broken deployment is the pair, both taken from the same dial:

```
lwd_mixnet_client_first_round_failures_total     / lwd_mixnet_client_connections_total
lwd_mixnet_client_connections_unestablished_total / lwd_mixnet_client_connections_total
```

The first is the transport's own rate. The second is what a wallet actually experiences, and keeping
it near zero is the whole job. The first rising on its own means retry is doing what it is there for;
both rising together is worth a page.

What the pair leaves out is the price. `lwd_mixnet_client_establishment_seconds` runs from the moment
a wallet's connection is accepted, so a dial that needed a second round carries the deadline the
first one spent waiting on streams that never answered. Retry buys the second rate down and pays for
it in that histogram, which is the only place the cost appears.

`/health` reports `starting`, `registered` or `serving`, answers 200 only for the last, and may add
`"degraded": true` beside it. The port
is bound before the mixnet client connects, so it can be asked about the slow, unreliable part of
startup: registering with a gateway takes seconds and was seen to fail outright on 2 of 15 attempts.

Both halves drain on `SIGINT` and on `SIGTERM`, letting connections in flight finish within
`--shutdown-grace-secs` (10 by default) before closing the rest. `SIGTERM` is what a container
runtime sends, so the grace period has to be shorter than the runtime's own: `docker stop` escalates
to `SIGKILL` after 10 seconds unless `-t` says otherwise.

The dialling half also exits with an error when its local mixnet client refuses every stream open
for several connections in a row, since a restart is the only thing known to bring one back — run it
under something that restarts it, as the compose file does.

**Three connections in a row that carry nothing turn `/health` into
`{"state":"serving","degraded":true}`**, and put the streak in
`lwd_mixnet_client_connections_failed_in_a_row`. The status code stays 200 and the process keeps
going, because neither a restart nor sending the wallet elsewhere fixes a far side that is gone or a
transport that is losing (ADR 0014). It clears on the first connection that gets through. Read the
gauge rather than the counters when the question is whether anything works *now*: the counters are
totals over a process lifetime, and a serving half that vanished barely moves them.

`--no-probe` takes this signal away with it. Nothing comes back from the far side before the wallet's
own bytes, so a dial can only report that a stream opened, and a destination that is gone looks like
one that is fine.

Nothing exported or logged identifies a client. The endpoint is unauthenticated, so keep it on
loopback or a private network: it reveals that the machine runs this proxy and how busy it is.

### When the gateway goes away

The serving half registers with one gateway and keeps it, because the gateway is the last component
of the address clients dial. A gateway that leaves the network takes the address with it. That
happened to the public testnet instance on 2026-08-18, and the shape is worth knowing before it
happens to yours.

From the dialling side it looks like a bad night on the transport. Streams open, none answer, and the
SDK says why at `WARN` if the filter is at `info`:

```
failed to send a repliable message - Failed to prepare packets -
no node with identity <gateway> is known. 0 reply surbs will be returned
```

From the serving side, startup never finishes. The log fills with `is still not online` for that
gateway, `/health` answers 503, and the process exits once `--gateway-wait-secs` runs out (ADR 0013).
Check whether the gateway is still routable before doing anything else, because being bonded is not
enough to be usable:

```
curl -s https://validator.nymtech.net/api/v1/unstable/nym-nodes/skimmed/entry-gateways/all \
  | grep -c <gateway identity>
```

If it is gone, re-register. **This changes the published address**, and there is no way back to the
old one, so tell whoever dials it. The keys stay, so only the last component changes:

```
docker compose stop <service>
# keep a copy of the whole state directory first, keys included
mv /state/gateways_registrations.sqlite* /somewhere/safe/
docker compose up -d <service>          # logs print the new NYM_ADDRESS=
```

### The state directory is private key material

The serving half's Nym address is derived from the keys in `--state-dir`. Losing it changes the
address, so clients can no longer find it. Copying it allows impersonation. It is gitignored here and
belongs in a volume with restricted permissions.

### Cost of being connected

A connected mixnet client generates continuous cover traffic, on the order of 2 Mbps sustained, for as
long as it is running. That is what the traffic analysis resistance is made of, but it is a real bill
on a metered connection.

## Building

```
make build     # cargo build
make test      # unit tests, no network
make lint      # clippy, warnings denied
make fmt       # rustfmt check
make verify    # all of the above
make image     # release binaries in a container
```

The SDK is pinned exactly and `Cargo.lock` is committed. It resolves to roughly 750 packages and
cannot be trimmed; upgrading it is a deliberate change, and whoever does it should re-run the
measurement rather than trust the numbers recorded here.

## Measuring

`lwd-mixnet-bench` drives the same dialling code the client half uses, against a running serving half,
and reports the raw failure rate and the wallet-visible one from the same attempts, along with what
establishing costs and whether failures are independent enough for retrying to help at all.

```
lwd-mixnet-bench --server <address> --trials 300 --attempts 6 --concurrency 1,3
```

Round sizes given as a list are rotated one per trial. The transport's failure rate moves by an order
of magnitude between one hour and the next, so two configurations measured back to back cannot be
told apart from the weather.

What a run has to show, and why a single threshold would not have been enough, is in
[ADR 0005](docs/decisions/0005-what-the-measurement-has-to-show.md). The results so far, including two
runs that had to be thrown out and what they teach, are in
[`docs/measurements/`](docs/measurements/README.md).

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — what the pieces are and how bytes move.
- [`docs/decisions/`](docs/decisions/README.md) — why it looks like this.
- [`docs/measurements/`](docs/measurements/README.md) — what was measured, and what it cost to measure it properly. Raw output from every run is kept alongside it.
- [SECURITY.md](SECURITY.md) — how to report a vulnerability, and how advisories against the pinned dependency tree are watched.

## Acknowledgments

The evaluation that produced this design was carried out in
[`lightwalletd-rs`](https://github.com/jpgonzalezra/lightwalletd-rs), where the measurements and the
decision to keep the transport out of that crate are recorded. Thanks to the Zcash community, and to
Nym for the SDK this is built on.

## License

MIT. See [LICENSE](LICENSE).
