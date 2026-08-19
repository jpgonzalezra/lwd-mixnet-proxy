# 0014. Say degraded rather than exit when nothing answers

## Context

[ADR 0010](0010-exit-when-the-local-client-degrades.md) exits the dialling half after five
connections in which the SDK refused every open, and deliberately leaves out the case where streams
open and nothing comes back, on the grounds that it is the ordinary bad day this proxy rides out.

That reading held until the far side went away. On 2026-08-18 the public serving half spent 23 hours
unable to reach the mixnet
([2026-08-18-gateway-gone.md](../measurements/2026-08-18-gateway-gone.md)). An operator dialling it
took 19 connections in that window, opened 76 streams, and had none of them answered. Their half sat
at `serving` throughout and would have sat there for a week. Nothing it exposed said otherwise: the
counters are totals over a process lifetime, so a run of failures barely moves them, and the log line
each connection leaves is the one a bad hour leaves too.

The two causes look identical from the dialling side, and that is not a gap to be closed here.
Whether the destination is gone or the transport is losing, the wallet gets the same nothing, and no
restart mends either.

## Decision

**Count connections that end with no stream, consecutively, and report at three.** The half sets
`"degraded": true` in `/health`, logs once at error level, and carries the streak in
`lwd_mixnet_client_connections_failed_in_a_row`. Three because each of those connections has already
spent the whole attempt budget, six streams by default: three in a row is minutes of a wallet getting
closed sockets.

**Do not exit.** ADR 0010's exit answers a client that only a restart brings back. Nothing here is
that: the local port works, the SDK is opening streams, and the next connection may be fine.

**Do not change the status code.** `/health` stays 200 while serving. A 503 tells a supervisor to
stop sending work or to restart, and both are wrong answers to a transport that is merely losing.
What is wanted here is a reader, not a reaction.

**Clear on the first connection that gets through**, which is why this is a flag beside the state
rather than a state. The three states only ever advance; this one has to come back.

**Count the dial, not the connection.** A dial resolves in seconds and says whether the far side is
answering. The connection it carries can last as long as the wallet keeps it, so folding its outcome
in when it closes reports a fact from ten minutes ago: a long-lived success that ends after a
degradation began would clear a flag that is still true, and with no further traffic the half would
say it is healthy indefinitely.

**Fold under one lock, in the library.** The report is three pieces of state, the streak, the gauge
and the flag, and they have to agree. Held in separate atomics they do not: two connections finishing
together can leave a degraded flag with a streak of zero, and the next success then reads a streak
below the limit and never clears it. `Streaks::fold` takes a `Mutex`, updates all three inside it,
and never awaits while holding it. Living in `src/streaks.rs` rather than in the binary is what makes
the interleavings testable at all.

## Consequences

- An operator can tell "nothing is getting through right now" from "this process has seen failures",
  which the counters alone cannot say. The gauge is the alertable form.
- A genuinely bad hour will trip it. That is not a false positive: three connections in a row with
  nothing is what the wallet is living through, whatever the cause.
- Like ADR 0010 it needs traffic. An idle half is never degraded, and a wallet that has given up
  makes this quiet exactly when it matters, which is an argument for scraping the gauge rather than
  waiting to be told.
- ADR 0010's refusal run now resets when a dial opens a stream rather than when the connection it
  carried ends, which is sooner and for the same reason. The exit itself is unchanged.
- **`--no-probe` turns the signal off**, and quietly, which is the sharpest edge here. With no probe
  a dial reports success as soon as the announcement is written, so against a destination that is
  gone every connection would look like an answer and clear the run. Rather than let it lie, an
  unprobed dial breaks a refusal run and touches nothing else: the flag can still be raised by dials
  that opened nothing, and can no longer be cleared by a success nobody witnessed.
- One lock is taken per dial. Dials take seconds and the critical section is a handful of stores, so
  the contention is not worth measuring, but it is a lock on a hot-ish path and worth knowing about.
- It does not say which of the two causes is at work. The one question that would separate them, is
  the destination's gateway still in the topology, belongs to the API and is not asked here.
- The serving half keeps the flag unused. It has no dials to count, and its own failure is the one
  [ADR 0013](0013-bound-the-wait-for-a-registered-gateway.md) bounds.
