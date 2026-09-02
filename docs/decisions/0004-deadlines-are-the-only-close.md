# 0004. Deadlines are the only close

> **Amended 2026-09-02.** Still true of every released SDK, and the decision stands. What changed is
> that one direction of the far side's silence is no longer unreadable: on the SDK's keepalive
> branch a dialler whose acceptor dropped the stream is told so in band, after about four and a half
> minutes, in three trials. The reverse direction, an acceptor whose dialler walked away, follows
> from how arming and pinging are written but was not measured. That is not a close, and it is far
> too slow for the dialling half's stall deadline, but what this decision calls a conversation that
> will never continue turned out to end, in minutes rather than in the reaper's half hour. Worth
> revisiting the listening half's reaper when a release carries it.
> Evidence: [the keepalive](../measurements/2026-09-02-keepalive.md).

## Context

A mixnet stream cannot be closed. Dropping one deregisters it locally and tells the far side nothing;
there is no FIN, no reset, and no frame that means "I am done". An end that walks away leaves the
other holding a conversation that will never continue, and no error is ever delivered to either.

This is not a corner case. It is the ordinary outcome of the failure in
[0003](0003-probe-every-stream-before-the-wallet-uses-it.md), of a wallet that quits, of an upstream
that sends `GOAWAY`, and of a dialler that discards a stream after a failed probe.

The probe only covers establishment. Anything that breaks once bytes are moving is left, and the
project's guarantee has to say something about it.

## Decision

**Never hang. Recovering an in-flight request is not promised; ending the connection is.**

Nothing is retried once the wallet's bytes are moving. Resuming would mean rebuilding HTTP/2 state
and replaying requests already in flight, and a request delivered twice is worse than one that failed
cleanly. Instead, each half runs the deadline that suits what it is watching:

- **The dialling half watches for a stall**: the far side was written to and has answered nothing
  since. That is a request without a response. When it fires, the local connection is closed, which
  turns an invisible hang into an ordinary error the wallet's gRPC library reconnects from.
- **The listening half watches for idleness**: nothing has moved in either direction. An idle
  connection is legitimate, so this is a reaper rather than a failure detector. It is how a stream
  whose dialler is gone is eventually let go, along with its upstream connection.

Applying either deadline to the other half would be wrong. A completed response leaves a connection
legitimately quiet, so a stall deadline on the listening half would fire on healthy streams; and an
idle deadline is far too slow to be what a waiting wallet relies on.

One implementation detail is worth recording: **neither direction is ever shut down individually.** A
mixnet stream's `shutdown` deregisters the whole stream rather than half-closing it, so shutting down
the write side would silently kill the read side.

## Consequences

- The failure a wallet sees is a closed connection, which is a case every gRPC library handles, and
  never a request that hangs forever.
- An upstream that closes its side is invisible to the wallet until the stall deadline fires. There
  is no way to propagate the close, so the deadline is the mechanism, and its default has to be low
  enough to be tolerable and high enough not to fire on a slow response.
- A request that was in flight when a stream died is lost, and may or may not have reached the
  upstream. Callers that cannot tolerate that must be idempotent or must check; this is why bulk
  streaming is a poor fit for this transport and single small calls are a good one.
- Both deadlines are configurable, and both can be switched off, which is only ever appropriate for
  debugging.
