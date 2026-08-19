# Dialling through the 00:00 UTC reclaim window

Date: 2026-08-18 / 2026-08-19. Not a bench run. This is the public-testnet client
half, with one local TCP connect a minute for ten minutes either side of midnight
UTC — the sample the 15 Aug external report could not take because that overnight
was idle.

Author: LaDale (forum user Lowo88). Asked for on
[the forum thread](https://forum.zcashcommunity.com/t/lwd-mixnet-proxy-light-wallet-grpc-over-the-nym-mixnet-and-what-three-days-of-measuring-it-found/57000/7)
after the 15–16 Aug run: *“a heartbeat through the client half, one connection a
minute for ten minutes either side of midnight, would settle it. if streams sail
straight through, that’s just as useful and i stop pointing at the cliff.”*

## Method

Same `docker compose` client-only setup as
[2026-08-15-external-client.md](2026-08-15-external-client.md), pointed at the
published public testnet `SERVER_ADDRESS`. This is a **new process**, not the
15 Aug container: started `2026-08-18T00:22:19Z`, Docker `RestartCount` 0, still
`serving` through the window. Image from this repo; Nym SDK **1.21.5-rc.3**.
Entry gateway labelled `entry-gateway-A` (identity omitted for the same reason
as the earlier report). Host: Windows + Docker Desktop. Compose
`restart: unless-stopped`.

Probe flags were still the 2026-08-15 defaults: `--probe-attempts 4`
`--probe-concurrency 3` `--probe-timeout-secs 10` (ladder: a round of three,
then a round of one). That is confirmed by the discard count below, not assumed
from `main` (which later moved to 6 attempts in ADR 0012).

Load behind `127.0.0.1:9068` was **TCP connect/close only** (no LWD gRPC bytes),
one connect per 60 s. Local connect success only means the client accepted the
socket; mixnet establish/discard is what the `/metrics` counters record.

UTC timestamps:

| event | time |
|---|---|
| this client process up | 2026-08-18T00:22:19Z |
| first connect / BEFORE `/metrics` | 2026-08-18T23:50:00Z |
| midnight UTC | 2026-08-19T00:00:00Z |
| last connect / AFTER `/metrics` | 2026-08-19T00:09:05Z |

Raw files:

- [`raw/2026-08-19-midnight-heartbeat.txt`](raw/2026-08-19-midnight-heartbeat.txt) — BEFORE/AFTER `/metrics` and the 20 connect lines
- [`raw/2026-08-19-midnight-reclaim.log`](raw/2026-08-19-midnight-reclaim.log) — SDK reclaim burst, first `run out of bandwidth` through the last Bandwidth-type WARN

## What local TCP said

20/20 connects returned ok in ~210 ms, including `2026-08-19T00:00:03.497Z`,
three seconds after reclaim started. The local port did not drop. Health stayed
`{"state":"serving"}` on both sides of the window. The client did not restart.

That number is **not** the wallet-visible mixnet rate. A TCP accept can succeed
while every probe stream stays unanswered.

## What the client counters said

| counter | BEFORE 23:50:00Z | AFTER 00:09:05Z |
|---|---|---|
| `connections_total` | 0 | 20 |
| `connections_unestablished_total` | 0 | 19 |
| `first_round_failures_total` | 0 | 19 |
| `rounds_total` | 0 | 38 |
| `streams_opened_total` | 0 | 76 |
| `streams_discarded_total{reason="unanswered"}` | (absent / 0) | 76 |
| `establishment_seconds_count` | 0 | 0 |
| `connections_in_flight` | 0 | 1 |

Rates against the 19 connections that had finished by the AFTER snapshot:

- first-round failure: **19/19**
- wallet-visible unestablished: **19/19**
- streams that answered: **0**

Ladder reconstructs: 19 connections lost both rounds (3+1), 19 × 4 = 76
unanswered opens, 19 × 2 = 38 rounds. The 20th connect is the `00:09:05Z`
sample, still `in_flight` when AFTER was snapped. Zero opens refused, so the
SDK accepted every open it was asked for; the 19/19 is unanswered probes, not
a degrading client.

## What reclaim said

Same nightly cliff, on this client:

| | 19 aug (this process) |
|---|---|
| first `run out of bandwidth` | 00:00:01.242250Z |
| `managed to claim testnet bandwidth` | 00:00:01.540267Z |
| time at zero | **298 ms** |
| sphinx packets the gateway refused | 5 |
| claim attempts (credential-less) | 6 |
| last line of the burst | 00:00:01.680204Z |

~287–298 ms sits in the same band as the serving-half nights in
[2026-08-15-daily-bandwidth-cliff.md](2026-08-15-daily-bandwidth-cliff.md)
(202–525 ms). A client that re-claims rides through it. Cover traffic resumed;
the process stayed up.

## Read on the cliff

Streams did **not** sail through. Wallet-visible failure for finished dials in
this window was 19/19.

That is **not** explained by the 298 ms accounting window alone. Unanswered
probes started at `23:50Z`, ten minutes *before* 00:00, and stayed unanswered
after the claim at `00:00:01.540Z`. Reclaim happened on schedule in the middle
of a path that was already silent.

So: the heartbeat settles Joaco's "is the cliff eating live dials?" question
only in the negative for *this* night — the cliff is visible and short, and
the silence is the whole ±10 min, not the quarter-second. Serving-half
counters for `2026-08-18T23:50Z`–`2026-08-19T00:10Z` are the other half of
the correlation.

## Limitations

- TCP connect/close only on `:9068`. No LWD gRPC request reached upstream.
- 20 dials, one night, one machine, one entry gateway.
- AFTER snapshot caught the last connect in-flight, so 19 finished / 1 open.
- Probe budget was still 4 attempts. A rerun on current `main` (6 attempts)
  would produce a different discard count.
- This process started 22 minutes after the *previous* midnight (18 Aug
  00:00 UTC), so it does not extend the 15 Aug container's longevity record;
  it only covers 18/19 Aug.
- Windows sleep log was not captured.

## Forum

Thread #57000, heartbeat asked in post 7, this window run 18–19 Aug.
