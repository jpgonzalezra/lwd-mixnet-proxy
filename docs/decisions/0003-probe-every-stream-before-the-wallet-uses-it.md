# 0003. Probe every stream before the wallet is allowed near it

> **Amended 2026-08-26.** The rate this decision is calibrated against is real and is what every
> `nym-sdk` release still produces, so the decision stands as written. What changed is why: the
> failures are not the transport losing payloads, they are a first `Data` frame overtaking the `Open`
> that registers its stream, which the pinned SDK discards without a log. Nym fixed that on `develop`
> in August 2026. On that tree the rate collapses, and 6,237 trials found nothing lost at all. Revisit
> this budget when a **release** carries the fix, not before. Evidence:
> [reordering, not loss](../measurements/2026-08-24-reordering-not-loss.md) and
> [six thousand trials](../measurements/2026-08-25-six-thousand-trials.md).

## Context

The transport loses a stream's first payload, often and silently. A stream opens, the far side's
`accept()` fires, the sender's `write_all` and `flush` both return `Ok`, and the payload never
arrives. Neither end errors and neither times out: **both hang until something external gives up.**

Measured over three days against a minimal reproduction using nothing but the SDK, the rate moved
between 2% and 51%, and is not stationary: within one 400-trial run, failures nearly tripled between
the first and second halves. Raising the reply-block budget shifts it monotonically, from 51% at one
block to 26% at four hundred, and costs 5.3x the latency to do so. No setting observed makes it
reliable.

The failure is overwhelmingly one of **establishment**: the stream opens and the first payload is
lost. Under degraded conditions loss also appeared on the return path.

That last fact is what makes this tractable. A failure that happens before the wallet has sent
anything can be filtered without the wallet ever knowing. A failure in the middle of a conversation
cannot: retrying there would mean rebuilding HTTP/2 state and replaying requests already in flight.

## Decision

**Every stream is probed before it carries anything, and a stream that does not answer is discarded
and replaced.**

Both halves are ours, so they speak a 14-byte header of their own: a magic, a version, a flag, and a
token. The dialling half sends it and waits for it back within a deadline. If the echo arrives, the
stream has demonstrated a full round trip and the wallet's connection is spliced onto it. If it does
not, the stream is dropped and another is opened, up to a configurable number of attempts. The wallet
sees none of this: it is holding an ordinary TCP connection that has not been answered yet.

**The probe reproduces the measured failure mode exactly.** This is not a generic retry hoping for
better luck; it is a test for one known defect.

Three details follow from the same reasoning:

- **The header is mandatory even when the probe is off.** The listening half needs it to recognise a
  stream as one of ours, so a stream that opens with anything else is dropped before it can reach an
  upstream. Switching the probe off removes the round trip, not the header.
- **The listening half puts its own deadline on the handshake.** The failure mode is precisely a
  stream that is accepted and never speaks, and without a deadline the SDK holds one for half an
  hour.
- **The upstream is not touched until a request arrives.** A stream that was probed and then
  discarded, or probed and never used, never becomes a connection to the node.

**Attempts are grouped into rounds, and a round opens three streams by default.** A failure is only
ever discovered when the probe deadline expires, so a round of one pays that deadline once per
failure, in series. Opening several at once turns that sum into a minimum, at the cost of reply
blocks and of exposing every stream in a round to the same moment.

The default is three because that is what
[measurement](../measurements/2026-08-06-probe-and-retry.md) supports. Interleaved against sequential
retry under a 34.67% per-stream failure rate, both configurations reduced the wallet-visible failure
rate to zero, so reliability did not separate them. Establishment did: p90 of 1.4 s and p99 of 6.3 s
for rounds of three, against 11.4 s and 31.3 s for retrying in series, because a third of connections
spent a full deadline waiting before their retry began.

## Consequences

- Establishing a connection costs one extra round trip, which is seconds on this transport, not
  milliseconds. That is the price of not hanging, and it is paid once per connection rather than per
  request.
- **The probe deadline cannot be set to a good value, only to a trade.** Healthy round trips measured
  p50 2.4 s and p99 between 3.4 s and 17 s depending on the session, and the failure mode is silence,
  so the two distributions overlap: a deadline short enough to keep establishment quick also discards
  healthy streams, which inflates the apparent failure rate and spends attempts. This is the reason
  rounds exist, and the reason the deadline is configurable rather than tuned to a constant.
- The probe can be switched off by configuration. If the SDK stops losing first payloads, that is the
  change required to take the cost back, and nothing else has to move.
- Discarded streams are not free for the listening half: it holds each one until its handshake
  deadline expires. The transport carries no close, so there is no way to tell it sooner.
- Retry is bounded. When every attempt fails, the dialling half closes the local connection, which is
  an error the wallet's gRPC library already knows how to handle. See
  [0004](0004-deadlines-are-the-only-close.md).
- Both the deadline and the round size are configuration, so the same build can be run as a
  sequential retry or as a hedged open. That is what makes them comparable in one measurement run.
- **Rounds of three leave two introduced but silent streams per connection on the listening half**,
  one measured run leaving 300 of them behind across 150 connections. They are held under their own
  short deadline rather than the idle one that governs a connection in use, since carrying two dead
  streams per connection for ten minutes is a cost the dialling half would be imposing on every
  operator.
