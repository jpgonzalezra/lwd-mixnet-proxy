# The establishment handshake, and what a fresh dialler does to a serving client

Date: 2026-09-01. Three runs against `nym-sdk` at `max/lwd-stream-patch`, pinned by commit rather
than by branch name: `f5e46b7d8287869221cd6632c63444d4b32da59c`.

The SDK's maintainer added an acknowledgement that a stream was accepted, and asked whether it works.
It does, and it answers the one failure this project could never detect from the dialling side. The
control needed to measure it also invalidates a claim in
[the reply-block reserve report](2026-08-27-surb-reserve-costs-nothing.md), which is amended.

## What is being tested

`accept()` now sends an `OpenAck` back through the dialler's reply blocks, and `MixnetStream`
gained `wait_established_with_timeout`. The acknowledgement is best-effort and is never
retransmitted; inbound data resolves the wait as well, which covers a lost one. The same commit
splits the reply-block count per frame type, 20 on the `Open` and 3 per `Data`, where both were 10
before.

## Method

The rig is `contrib/nym/` from `lightwalletd-rs`: two mixnet clients in one process, one accepting
and echoing 64 bytes, one dialling it, budgets rotated one per trial, everything in containers. It
gained `--wait-established`, which waits for the acknowledgement before writing the payload,
`--dead-peer-trials`, and `default` as a budget token so the SDK's own counts can be exercised. The
build takes `nym-sdk` from the git revision above instead of from crates.io, since none of that API
is in a published version.

Waiting before writing is what isolates the acknowledgement. The echo side writes only after it has
read, and the dialler writes only after the wait has finished, so nothing else can resolve the wait
and a lost `OpenAck` shows up as a lost `OpenAck`.

| run | dialler | trials | budgets |
|---|---|---|---|
| 1 | one, reused | 500 | `default` and 10, interleaved |
| 2a | rebuilt every trial | 134 captured | 1, 10 and `default`, interleaved |
| 2b | rebuilt every trial | 142 | same |

Run 1 also dials ten streams at an address whose client registered with a live gateway and then
disconnected.

Both fresh-client runs are cut at trial 100 for every statistic below. Each collapses a few trials
later, at 107 and at 105, and a hundred is the largest round number that is before both. Percentiles
are the rig's own: the value at index `floor((n - 1) * p + 0.5)` of the sorted sample, ties rounded
away from zero the way Rust's `f64::round` does. That is what printed every summary inside the raw
files, so a figure here can be checked against the one beside it.

## The acknowledgement predicts the outcome

Five hundred trials, each scored twice:

| | echo came back | echo failed |
|---|---|---|
| acknowledged | 500 | 0 |
| no acknowledgement | 0 | 0 |

Neither off-diagonal cell was ever reached. A layer that discards an unacknowledged stream would
have discarded nothing healthy here, and would have kept nothing dead.

## An address with nobody behind it

This is the failure named in the design note as the one nothing at the sender can see: a gateway that
is alive and in the topology, and a client that is not running. The gateway accepts and stores for an
absent recipient exactly as it would for a present one, and the SURB-ack layer forwards either way.

Ten dials at such an address, on a client that registered and then disconnected:

```
   1  dead  no ack       15002 ms  no establishment acknowledgement within timeout (peer state unknown)
   ...
  10  dead  no ack       15001 ms  no establishment acknowledgement within timeout (peer state unknown)
dead peer: 10 dials | opens refused 0 | acknowledged 0
```

`open_stream` succeeds, because the routing check has nothing to complain about. The caller learns
in fifteen seconds instead of waiting on its own deadline, and the error says what it does not know.

## What the acknowledgement costs

Half a round trip, which is what it has to be: it travels back over the mixnet on the dialler's
reply blocks.

| run 1 arm | n | total p50 | ack p50 | ack p90 | echo after the ack, p50 |
|---|---|---|---|---|---|
| 10 reply blocks | 250 | 3,035 ms | 1,499 ms | 1,801 ms | 1,509 ms |
| SDK counts, 20 and 3 | 250 | 2,985 ms | 1,601 ms | 2,037 ms | 1,352 ms |

Over all 500: p50 1,551 ms, p90 1,932 ms, p99 3,073 ms, longest 4,792 ms.

The two arms are the same to within the noise of the afternoon, in both directions, which is the
useful part: the new per-frame counts cost nothing measurable against the flat 10 they replace.

## One reply block on the `Open` delays the acknowledgement

The fresh-client runs carry a third arm at budget 1, and it separates cleanly from the other two.

| arm | run 2a ack p50 | run 2b ack p50 |
|---|---|---|
| 1 | 4,644 ms | 6,617 ms |
| 10 | 2,539 ms | 4,499 ms |
| SDK counts | 2,768 ms | 3,287 ms |

Budget 1 costs about two seconds against the flat 10, twice: 2,105 ms in the first run and 2,118 ms
in the second, in windows whose absolute latencies differ by half again. Against the SDK's split
counts the same penalty is 1,876 ms and 3,330 ms, which is the same story told by a noisier pair of
numbers. Between 10 and the SDK's 20 there is nothing to resolve: 229 ms one way in the first run,
1,212 ms the other way in the second.

Through those first hundred trials the log says why, on every budget-1 trial and on no other:

```
failed to request more surbs to clear pending queue of size 1 (attempted to request: 10):
not enough reply SURBs to send the message, available: 0 required: 1
```

The reading, and it is a reading rather than a trace: the acceptor is handed one reply block, spends
it acknowledging, and cannot ask for more because the request needs one of its own. It is unstuck by
the dialler's next message, which carries another. Twenty on the `Open` never gets into that state.

## A serving client degrades under fresh diallers

The runs that rebuild the dialling client every trial do not hold up. Both collapse, at trial 107 and
at trial 105, and neither recovers. What precedes the collapse is a slope rather than a cliff, and a
noisy one: run 2b climbs in every window, run 2a dips once before it climbs.

| trials | run 2a ack p50 | run 2b ack p50 |
|---|---|---|
| 1-20 | 3,031 ms | 3,240 ms |
| 21-40 | 2,768 ms | 3,517 ms |
| 41-60 | 3,248 ms | 5,275 ms |
| 61-80 | 4,188 ms | 5,956 ms |
| 81-100 | 4,051 ms | 7,403 ms |
| 101-120 | 14 of 20 failed | 18 of 20 failed |
| 121 onward | all failed | all failed |

Run 1 is the control, and it is flat. Five hundred trials through one reused dialler, in ten windows
of fifty: 1,575, 1,547, 1,527, 1,607, 1,553, 1,545, 1,547, 1,553, 1,557 and 1,537 ms, and not one
failure.

So the collapse belongs to the configuration that rebuilds the dialling client, and not to the number
of streams, the process's age, or the acknowledgement, which run 1 also used five hundred times. The
[27 August run](2026-08-27-surb-reserve-costs-nothing.md) rules the acknowledgement out from the
other side: it rotated diallers without one, and collapsed the same way in all four of its blocks.

That is an association and not an isolated variable. Rebuilding a dialler carries a registration, a
teardown, a different gateway and route, a topology fetch, and twenty seconds of extra wall clock
with it, and this pair of runs separates none of those from each other.

Every failure before and through the start of each collapse is in the return direction: the echo side
reads and writes, and neither the acknowledgement nor the reply arrives. Deep in run 2a's tail two
are not, a `LOST-OUT` at trial 125 and a `NO-ACCEPT` at 127. The accepting client is the only thing
that lives across a whole run, and what accumulates in it is one anonymous sender tag per trial, each
with its own reply-block store. That is where suspicion falls, on elimination rather than on
evidence.

It matters beyond the rig. A public serving half meets exactly this workload, one short-lived
dialling client per wallet, so a serving client that stops replying after a hundred of them is an
operational problem and not only a measurement artifact.

### One thing seen on the way out

Run 2b ended at trial 142 rather than 150. A fresh dialler could not be built at all, because
`nym-api` was serving a topology whose rewarded set and node details came from different epochs, and
tearing that client down panicked a worker:

```
panicked at common/client-libs/gateway-client/src/packet_router.rs:70:13
```

That line is a deliberate `panic!` for an ack that cannot be routed when no shutdown is in progress,
guarded by a comment saying it should never happen the way the code is currently used. Building and
dropping 140 clients in one process is not the way it is currently used, so this is a corner rather
than a defect in the path anyone travels. It is in the raw log either way.

### What this corrects

The 27 August report read the same collapse as a property of first exchanges and put a 30% failure
rate on it. That number records where each of its four processes fell over. Before those collapses
its rate is 5 failures in 142 trials, and the two runs here fail 0 and 1 in their first hundred, with
two more in run 2b at trials 102 and 103, immediately before it goes.

## Limitations

- One machine, one afternoon, one pair of gateways per client. Absolute latencies do not reproduce;
  the arms are interleaved inside each run because of it.
- The collapse has been seen six times, at trials 30, 35, 40, 41, 105 and 107. The knee is not a
  constant, so nothing here predicts when a serving client gives out, only that it does.
- Where the return path breaks is not established, and neither is what about a rebuilt dialler
  matters. Registration, client teardown, gateway and route selection, topology fetches and the
  extra wall clock all move together here, and the accepting client is a suspect by elimination.
- Both fresh-client runs saw DNS and `nym-api` failures while building diallers. In each run the
  first of those lands after the collapse has already started, so they do not explain it, but they
  are in the same logs and they are not nothing.
- The acknowledgement was measured against a peer that always accepts. A listener that is slow to
  call `accept()`, or that never does, is a different question and is not tested here.
- Run 1 and both fresh-client runs share the host with an idle `lwd-mixnet-proxy` client from
  another deployment. It was present throughout, so it does not favour an arm.
- Run 2a's capture is truncated at trial 134, and its own summary is lost: the container was removed
  when the process that was reading its output died. Everything past trial 107 was already
  discarded, so the loss is inside the invalid tail.
