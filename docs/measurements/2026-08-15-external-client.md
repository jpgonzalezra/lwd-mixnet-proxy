# What an external dialling half saw against the public testnet

Date: 2026-08-15, with a longevity check on 2026-08-16. Not a same-machine loopback
run. This is one independent operator's client half pointed at the public testnet
serving address, with raw counters and a small interleaved bench in the same window.

Author: LaDale (forum user Lowo88). Written for
[the forum thread](https://forum.zcashcommunity.com/t/lwd-mixnet-proxy-light-wallet-grpc-over-the-nym-mixnet-and-what-three-days-of-measuring-it-found/57000/7)
as the dialling-half numbers behind the serving-half reading already correlated in
[The gateway allowance empties at 00:00 UTC](2026-08-15-daily-bandwidth-cliff.md).

## Method

`docker compose` client-only against the published public testnet `SERVER_ADDRESS`.
Image built from this repo at the time of the run; Nym SDK **1.21.5-rc.3**. Entry
gateway chosen by the client: labelled `entry-gateway-A` (one gateway served the
whole run; identity and IP omitted so a failure rate is not read as a claim about
that node). Host: Windows + Docker Desktop. Compose `restart: unless-stopped`.

Client probe flags were the 2026-08-15 defaults: `--probe-attempts 4`
`--probe-concurrency 3` `--probe-timeout-secs 10` (ladder: a round of three, then
a round of one). `main` later moved to 6 attempts (ADR 0012); a rerun today gets
a different discard count.

The host was in interactive use during the 15 Aug load window. The same container
then stayed up ~24h with Docker `RestartCount` 0 and continuous logs through the
16 Aug check. A Windows sleep log (`powercfg /sleepstudy`) was not captured at
the time of the bench. Zero opens were refused, which rules out a degrading
client but is not a substitute for a sleep log.

UTC timestamps for the load window:

| event | time |
|---|---|
| client up (`compose up -d client`) | 2026-08-15T16:07:56Z |
| health reported serving | 2026-08-15T16:11:22Z |
| `/metrics` snapshot after load | 2026-08-15T16:18:05Z |

Load behind `127.0.0.1:9068` was **TCP connect/close only** (no LWD gRPC bytes): 30
connections. Mixnet probe establish/discard counters still moved.

`lwd-mixnet-bench --trials 20` with interleaved rounds of 1 and 3 ran **alongside**
the client, not after it: the client was up from 16:07:56Z through the 16:18:05Z
snapshot; the bench started ~16:15:36Z in a one-off container against the same
serving address. The dialling half keeps no state directory, so each container is a
fresh SDK client: two of them ran at once, each with its own gateway registration
and its own bandwidth allowance, sharing the host's uplink and the one serving half
at the other end. The bench used `--attempts 6` (tool default); the client container
used 4. First-round rates stay comparable; wallet-visible rates do not, because they
come off different attempt budgets.

Raw files:

- [`raw/2026-08-15-external-client-metrics.txt`](raw/2026-08-15-external-client-metrics.txt) — rates, timestamps, `/metrics` block
- [`raw/2026-08-15-external-bench.txt`](raw/2026-08-15-external-bench.txt) — bench trial table and summary
- [`raw/2026-08-16-external-overnight.txt`](raw/2026-08-16-external-overnight.txt) — ~24h check and midnight log excerpt

The dialled Nym address in the raw files is replaced with the label
`public-testnet-server`. The entry gateway is `entry-gateway-A`.

## What the client counters said

At 2026-08-15T16:18:05Z:

| counter | value |
|---|---|
| `connections_total` | 30 |
| `first_round_failures_total` | 5 |
| `connections_unestablished_total` | 2 |
| `rounds_total` | 35 |
| `streams_opened_total` | 95 |
| `streams_discarded_total{reason="unanswered"}` | 17 |
| `establishment_seconds_count` | 28 |

Rates against `connections_total`:

- first-round failure: **16.67%** (5/30) — a **round-of-three** rate
- wallet-visible failure: **6.67%** (2/30)

The unanswered discard count matches a simple ladder of round sizes used that day:
5 × 3 + 2 × 1 = 17. Bytes to/from the mixnet stayed 0, which is expected for TCP
connect/close with no RPC payload.

`establishment_seconds` in that `/metrics` dump times the **answering round only**,
not the wait from accept. Three of the 28 establishments followed a timed-out first
round, so the wallet had already waited past 10 s; the histogram buckets (all 28
under 8.39 s) therefore understate what the wallet saw. The bench file prints both
quantities side by side.

## What the interleaved bench said

20 trials, 10 per arm, rounds of 1 and 3 interleaved, 10 s probe deadline,
`--attempts 6`. No opens refused.

| | rounds of 1 | rounds of 3 |
|---|---|---|
| first-round failure rate | **60.00%** (6/10) | **10.00%** (1/10) |
| wallet-visible failure rate | **0.00%** (0/10) | **0.00%** (0/10) |
| establishment p50 | 11,171 ms | 1,392 ms |

The **60.00%** is a **per-stream** rate (rounds of 1). It is not the same quantity
as the client's **16.67%** (round of three). The comparable pair is 16.67% against
the bench's round-of-3 rate of 10.00%. Independence at p=0.60 predicts 21.6% for a
round of three; the raw bench file already prints that.

Same shape as the 2026-08-06 harness report: retry clears what the wallet sees, and
rounds of three cut the establishment tail. Absolute rates are one short sample in
one window; they are not a claim that the transport sits at 60%.

## Longevity overnight

At 2026-08-16T16:25Z the same container was still healthy and serving, about 24h17m
after start. Docker `RestartCount` was 0. Connection counters were unchanged from the
16:18Z snapshot (idle overnight; no further dials).

At **2026-08-16T00:00:01Z** the client logged **14** `Not enough bandwidth` warnings
and then claimed testnet bandwidth successfully. That lines up with the serving-half
midnight cliff in
[2026-08-15-daily-bandwidth-cliff.md](2026-08-15-daily-bandwidth-cliff.md): two
independent clients, two gateways, **the same midnight window**. The serving half's
16 Aug burst ran `00:00:00.231Z`–`00:00:00.885Z`; this client logged starting
`00:00:01.040Z`. Under a second apart, not the same UTC second. The 14 WARN lines
plus the claim line are in the overnight raw file.

## Correlated on the serving half

The maintainer already noted on the forum and in the cliff report that the
deployment's `duplicate fragment received` warnings all fall between **16:11:29Z and
16:16:43Z** on 15 aug — inside this dialling window — and that the midnight reclaim
on 16 aug matches this client's log. Credit for the serving-half reading is theirs;
the dialling-half numbers behind that reading are here.

## Limitations

- TCP connect/close only on `:9068`, so this is not an end-to-end LWD gRPC call over
  the mixnet. Probe metrics moved; application bytes did not.
- Small samples: 30 local connections, 20 bench trials.
- Overnight was idle cover traffic. No dial was in flight inside the 00:00 UTC reclaim
  window on 16 Aug. That sample was taken later:
  [2026-08-19-midnight-heartbeat.md](2026-08-19-midnight-heartbeat.md).
- One machine, one entry gateway, one afternoon's weather.
- Two SDK clients shared the host during the overlapping bench window.
- The client only reported healthy at 16:11:22Z after coming up at 16:07:56Z; early
  traffic may still have been settling.
- Windows sleep log was not captured for the 15 Aug window.
