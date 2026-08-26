# A thousand trials on the branch that answers two of these reports

Date: 2026-08-26. One batch of 1,000 trials against `nym-sdk` at `max/lwd-stream-patch`, plus one
dial each against that branch and the pinned release at an address whose gateway is not in the
topology.

Two of the things reported here have been fixed upstream by the SDK's maintainer, in a branch he
asked to have tested: a stream that cannot be routed now fails at `open_stream` instead of hanging,
and a reorder-buffer overflow now raises an `io::Error` instead of skipping past the missing frames
in silence. This is what the rig sees with those in place.

## Method

The rig is `contrib/nym/` from `lightwalletd-rs`, unchanged, the same one behind
[the six thousand trial run](2026-08-25-six-thousand-trials.md): two mixnet clients in one process,
one echoing 64 bytes and one dialling it, budgets rotated one per trial, everything in containers on
a release build. `nym-sdk` comes from git at the branch head rather than from crates.io.

The second test is smaller and has nothing to do with throughput. A dialler is pointed at a
well-formed address whose gateway identity is absent from the topology, and the question is only what
the caller is told. The address reuses this deployment's own identity with its encryption key
substituted for the gateway component, so it is syntactically valid and routes nowhere.

## The batch

| | |
|---|---|
| trials | 1,000 |
| ok | 997 |
| `stream data lost` | 0 |
| frames rescued by the orphan buffer | 207 |
| streams whose `Open` never arrived | 3 |

| budget | trials | ok | p50 |
|---|---|---|---|
| 1 | 250 | 249 | 1,178 ms |
| 20 | 250 | 250 | 1,544 ms |
| 100 | 250 | 249 | 2,738 ms |
| 400 | 250 | 249 | 6,684 ms |

`echo stages: accepted 1000 | read 1000 | written 1000`. Every stream was accepted, read and answered
by the far side, so nothing failed on the way out.

## The three failures are one event

They are consecutive: trials 671, 672 and 673, at 13:31:20, 13:31:45 and 13:32:18, each hitting the
rig's 20 second deadline exactly. In the minute that followed, three orphan frames arrived for three
distinct stream ids and were swept: buffered at 13:32:12, 13:32:18 and 13:32:20, cleaned at 13:32:20
and 13:32:30.

The reading is that those are the three replies, arriving somewhere between 22 and 72 seconds after
their requests, for streams the dialler had already given up on and deregistered. Each orphan cannot
be matched to a particular trial from the log, so that pairing is inference; the timestamps are in
the raw file for anyone who wants to argue with it.

Either way nothing was lost. It is the same shape as the single failure in the six thousand trial
run, at a scale that makes the point better: a transport whose tail reaches past a minute, and a
deadline that cannot tell that from a stream that will never answer.

## The unroutable dial

| build | what the caller got |
|---|---|
| 1.21.5-rc.3 | nothing. Two `no node with identity` warnings a layer below, and a connection that sits there |
| `max/lwd-stream-patch` | `opening the mixnet stream failed ... cannot route to HDfv77...`, returned by `open_stream` |

That is the failure mode behind [the gateway that left](2026-08-18-gateway-gone.md), from the dialling
side, turned into an error the caller can act on. The serving side of that outage was registration,
which this branch does not touch.

## What this does not test

- **The reorder error never fired.** No trial reached the byte cap, so the new `io::Error` path is
  unexercised here. It is the other half of the branch and this run says nothing about it.
- **A gateway that leaves mid-conversation.** The routing check runs in `open_stream`, so a
  conversation already under way when its gateway goes still loses writes with the warning a layer
  below. That is the shape of 18 to 20 August and it is untouched.
- **Teardown.** There is still no close on the wire, so a discarded stream is one the far side holds
  until its own deadline.
- **The orphan buffer's own bounds**, 5 seconds swept periodically, 64 streams, 32 frames each. This
  workload never approached them.

## Limitations

- One batch, one client pair, one gateway, one afternoon. 1,000 trials bound loss loosely, and the
  six thousand trial run is the tighter bound.
- The pairing of the three late frames to the three failed trials is a reading of adjacent
  timestamps, not an identity carried in the log.
- The branch is a work in progress on someone else's tree and will change. The numbers belong to its
  head as of 2026-08-26.
- The unroutable dial is one dial per build, which is enough for a categorical difference (an error
  or no error) and not enough for anything about timing.
