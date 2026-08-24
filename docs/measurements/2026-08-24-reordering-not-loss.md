# The silent stream failures were reordering

Date: 2026-08-24. Three runs of the reproduction rig against the live mixnet, in one afternoon.

Three weeks of reports here describe a stream that opens, gets accepted by the far side, and then
never delivers its first payload, with no error and no timeout at either end. This measures where
that goes. The payload is not going missing in the network. It arrives before the `Open` that
registers its stream, and the version of `nym-sdk` these reports were written against discards
whatever arrives before the stream exists.

## Method

The rig is `contrib/nym/` from the `lightwalletd-rs` repository, unchanged: two mixnet clients in one
process, one echoing a 64 byte payload and one dialling it, no gRPC and no proxies in the path. Reply
block budgets rotate one per trial so a drifting network hits every value equally.

Two builds of the same rig, differing only in where `nym-sdk` comes from:

| build | `nym-sdk` |
|---|---|
| develop | git, `nymtech/nym` at `ece291df435cb023ced12aace8b87ec4b4384ffd` |
| pinned | crates.io, `=1.21.5-rc.3`, which is what this project pins |

The rig's own sources are identical between the two. The builds differ in one more place than the
dependency: the pinned copy carries and enforces its `Cargo.lock`, while the develop copy has none
and resolves fresh, since changing the dependency invalidated it. The git rev is what pins that
build.

Everything ran in containers, release profile, host under `caffeinate`.

**The comparison is not between failure rates.** Two SDK versions cannot be interleaved inside one
process, so a rate against a rate would be two clients in two windows, which this project's own
methodology rejects: the transport's failure rate moves by an order of magnitude between one hour and
the next.

What makes the afternoon conclusive instead is that `develop` counts the mechanism itself. Its
`StreamMap` buffers frames that arrive before their stream is registered, and logs each one at
`trace`:

```
Stream {id}: buffering seq {seq} until registration
```

Every one of those lines is, on the pinned release, a frame handed to `send_to_stream`, not found in
the map, and dropped by an `else` branch that logs nothing. So one run on `develop` measures how
often the race happens, without needing the other build at all.

## Run A: develop, 400 trials

Zero failures. 400 accepted, 400 read, 400 answered.

| | run A, 2026-08-24 | the 2026-08-04 run on the pinned release |
|---|---|---|
| trials | 400 | 400 |
| failures | **0** | 26% to 51% by budget |
| p50, budget 1 | 1,416 ms | 1,412 ms |
| p50, budget 400 | 7,401 ms | 7,529 ms |

The latency distribution is within noise of the run that failed a third of its trials, so the network
was not being unusually kind.

The trace tells the rest:

| line | count |
|---|---|
| `buffering seq N until registration` | **90**, all `seq 0`, in 90 distinct streams |
| `Cleaning up orphan frames for never-registered stream` | **0** |
| `orphan buffer full` | 0 |

90 of 400 is 22.5%. Every one was the first payload of its stream, which is the frame sent
immediately after the `Open` and therefore the one racing it. No orphan ever aged out: every `Open`
arrived, and inside the 5 second TTL.

That bounds outright loss in this run rather than measuring it. The layer below retransmits, and run
A logged 1,110 `duplicate fragment received` warnings while it did so, so nothing here says what
happened packet by packet.

## Runs B and C: the pinned release, the same afternoon

Run B started 4 minutes after run A ended and was **stopped at trial 272**. Its first 200 trials are
the measurement; the rest is a client falling over:

| trials | failures |
|---|---|
| 1 to 50 | 26% |
| 51 to 100 | 28% |
| 101 to 150 | 16% |
| 151 to 200 | 14% |
| 201 to 250 | 98% |
| 251 to 272 | 100% |

The cut is post-hoc, so it is worth being exact about what supports it. Not the packet backlog: it
does climb and sit between 170 and 190 through the tail, but runs A and C peak at 217 and 182 while
failing nothing, so the number alone diagnoses nothing. What supports it is the shape, 14% and then
98% between one block of 50 and the next, and run C below. Two `Not enough bandwidth` warnings appear
in the first seconds of the run, nowhere near the cliff.

Run C settles which it was. A fresh client, 80 trials, started **14 seconds** after run B was
stopped, which is what makes it an answer: the same minute, the same network, a different client.

```
trials 80 | ok 63 | failure rate 21.2%
echo stages: accepted 80 | read 63 | written 63
```

All 17 failures are outbound, and the echo side accepted every stream while reading only 63 of them.
The backlog stayed healthy. So the cliff in run B belongs to that client, not to the network, and the
21% of its first 200 trials stands.

## The two curves track each other

Orphan frames on `develop`, attributed to the trial that produced them, against failures on the
pinned release in run C:

| budget | orphans, run A | failures, run C |
|---|---|---|
| 1 | 38% | 40% |
| 20 | 24% | 25% |
| 100 | 27% | 20% |
| 400 | 1% | 0% |

Different builds, different clients, adjacent windows, 20 trials per row in run C. That is strongly
consistent, not a demonstration that the two are one curve. What the code does establish is the
direction: a frame that arrives before its stream is discarded on the pinned release, so the trials
`develop` rescues are trials the pinned release had no way to complete.

It also points at a curve this project has been reporting since 2026-08-04 without a mechanism, the
one where a larger reply-block budget lowers the failure rate. Every message carries the budget, and
the `Open` and the first `Data` enter the same transmission lane in that order, so a larger budget
puts a larger `Open` ahead of a larger `Data`. Somewhere in enqueueing, transmission and reassembly
that buys separation, and by 400 it buys enough that the race is over before it starts.

Which of those three steps does the work is not settled here. These logs carry no packet departure
times, so "the `Open` finishes leaving before the `Data` starts" is a plausible reading and not a
measured one. Isolating it would take a run that pads the messages without changing the budget.

## What this corrects

- **Not loss, for the failures this run can account for.** The reading carried by earlier reports
  here, and in `contrib/bench/results/mixnet-transport-2026-08.md` in the `lightwalletd-rs`
  repository, is that the transport drops payloads silently. Every frame this run can point at
  arrived. The ones behind a failure arrived early and were discarded above the transport.
- **Not the reply-block budget either.** The budget curve is a race being won or lost, not
  replenishment running dry.
- **Already fixed upstream.** The orphan buffer landed in `nymtech/nym` on 2026-08-14 with #7057,
  bounded at 64 streams, 32 frames each and a 5 second TTL. It is on `develop` and in no release, so
  everything this project ships still meets the old behaviour.
- **The probe was doing real work.** Discarding a stream whose probe goes unanswered, and opening
  another, is exactly the right response to a race that is lost per stream and independent per
  attempt. It compensated for this without naming it.

## Limitations

- One afternoon, one client pair, one gateway, one host. The rates are this window's.
- Runs A and B are adjacent, not interleaved. Only run A carries the mechanism count, which is why
  the conclusion does not rest on comparing them.
- Run B's tail is discarded on a judgement made after seeing it, not on a rule declared before the
  run. The first 200 trials are arithmetically what they are; what went wrong in that client after
  them is not explained here.
- Run C is 20 trials per budget. The 100 row differing by 7 points from run A is within what that
  sample supports.
- Nothing here measures packet-level delivery in either direction. Zero never-registered streams in
  400 trials bounds outright loss of a whole message; the layer below was retransmitting throughout,
  and what it recovered is not counted.
- The develop build is one commit, `ece291d`, resolved as a git dependency rather than from its own
  workspace. It carried the orphan buffer, which is what the run needed; nothing else about that tree
  was verified.
