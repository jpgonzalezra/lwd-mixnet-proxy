# The keepalive finds a stream nobody closed

Date: 2026-09-02, with a second pass on 2026-09-03 and a mixed-version pass on 2026-09-04. Three
arms of three trials each against `nym-sdk` at `max/stream-keepalive`, commit
`693201bf14092f4445f1417e86051d3b4ebdb20a`.

The day after the establishment acknowledgement landed, its author put ping/pong keepalive on top of
it and asked whether that covers what this project needs. It does, and one arm answers a question
open here since [ADR 0004](../decisions/0004-deadlines-are-the-only-close.md): what a dialler can
know about a stream the far side has thrown away.

## What is being tested

An armed stream that has heard nothing for 60 seconds gets a `Ping`. After three consecutive
unanswered pings the stream fails in band with `StreamFailure::PeerUnresponsive`, which reaches a
caller as `io::Error` of kind `TimedOut` and the text `stream peer unresponsive: keepalive pings
unanswered`. A stream arms only once the peer has sent a liveness frame of its own, so a peer on an
older SDK is never pinged and never fails.

## Method

The rig gained a `keepalive` binary next to `repro`. Every trial builds an accepting client, opens
one stream to it, waits for the establishment acknowledgement, which is also what arms keepalive,
and then writes nothing ever again. The dialling client is reused across trials; a fresh acceptor
per trial keeps the arms comparable and gives `peer-gone` one to spend.

The accepting side holds what it accepts. A dropped stream deregisters, and that is the third arm's
whole subject, so it must not happen by accident in the other two.

| arm | what happens after the acknowledgement | what it stands for |
|---|---|---|
| `alive` | nothing | a quiet stream between two live peers |
| `peer-gone` | the accepting client disconnects | a process that stopped, its gateway still accepting for it |
| `stream-gone` | the accepting client stays up and drops the stream | teardown: no close frame exists to send |

The clock starts at the kill and stops at the error the caller reads.

## What happened

| arm | trials | reported unresponsive | time to report |
|---|---|---|---|
| `alive` | 3 | 0, all survived 420 s | |
| `peer-gone` | 3 | 3 | 252, 258, 260 s |
| `stream-gone` | 3 | 3 | 257, 258, 262 s |

A single earlier trial of `stream-gone`, run to check the rig before the batch, reported at 267 s.
It is in the raw file, and it is not in the table or in any range quoted below.

Seven ping intervals of silence did not cost the live arm its stream, in any of its three trials.
The two ways of dying are not distinguishable by the clock, which is what you would expect: in both,
the peer stops answering.

## Why four intervals and not three

The sweep runs on the router's cleanup tick. The first pass after 60 seconds of silence sends a ping
without counting a miss, because nothing is outstanding yet. The misses land on the passes at 120,
180 and 240 seconds, so three unanswered pings cost four intervals rather than three.

240 seconds is therefore the earliest this can fire, and it is a floor rather than a prediction.
Each of those four waits ends on a tick rather than on the second, and a tick is up to ten seconds,
so the schedule can slip by as much as forty seconds before anything else is counted. The six
failures measured 252 to 262 seconds, which sits inside that. Nothing here separates the slip from
whatever delivering the error to the caller costs on top.

Four and a half minutes covers every failure seen here, which is not the same as bounding one. The
model above allows 240 seconds plus as much as forty of slip before anything is counted for
delivering the error, so a budget meant to hold rather than to describe starts above 280 seconds.

## The stream nobody closed

`stream-gone` is the arm this project has been waiting for. There is no close frame in this
transport, so a stream discarded by one side stays registered on the other until its idle reaper
fires half an hour later. Both halves of this proxy discard streams on their failure path, which
means each of them has been leaving work on the far side for thirty minutes at a time.

Keepalive does not close anything, and it is not a teardown. What it does is make the far side's
silence legible: the peer's router answers pings only for streams it still holds, so a stream that
was dropped stops answering exactly like a peer that died. Four and a half minutes is a long time
next to a wallet's deadline and a short one next to thirty.

## Watching the exchange from outside

The runs above measured outcomes and not the pings behind them, because no ping or pong is logged at
any level: the code logs only the exceptional paths, a stale nonce, a full channel, an unknown
stream. Reading the logs was the wrong place to look. `MixnetStream::last_peer_activity()` returns
when the peer was last heard from, and reads local state without sending anything, so a second pass
polled it every five seconds.

| arm | trials | oldest the last inbound frame got | times that age fell back |
|---|---|---|---|
| `alive` | 2 | 71.4 s, 76.9 s | 6, 5 |
| `stream-gone` | 1 | 265.1 s | 0 |

The getter reports that the peer was heard from, not what it said. What makes this readable is the
setup rather than the getter: after the acknowledgement neither side writes, and an acceptor never
pings, so on the live arm the only frame that can arrive is a pong. Under those conditions a fall in
the age is a pong landing. None of them was identified as one.

The live arm's peak sits a little above the 60 second interval, which is that interval plus the tick
it waits for plus the trip out and back, and the age falls five or six times across the hold. The
dying arm climbs to its failure without falling once. That is consistent with the schedule the
section above derives from the constants, and it is as close to watching the exchange as this gets.

The two figures per trial are the rig's own arithmetic over its polls, and the polls themselves are
not in the raw file, so they can be read but not recomputed from it.

## One direction of the version question, from the deployment

The public testnet serving half moved to this commit on 2026-09-03, keeping its state directory and
therefore its published address. Its dialling half did not move: the released `1.21.5-rc.3` build
was pointed at it for twelve trials, and all twelve answered on the first round, p50 1,418 ms.

That is twelve trials in one deployment on one afternoon, and the bench prints its own warning that
a rate this low cannot separate a design property from a quiet afternoon. What it says is that the
released dialler worked against this upgraded acceptor every time it tried.

The code says why, which is a separate argument from the run: the `OpenAck` an old peer cannot parse
is dropped on the unknown discriminant, and only the dialling side pings, so keepalive never enters
a conversation an old dialler starts. Twelve trials are consistent with that. They do not establish
it.

## The other direction, where a false positive would cost something

A new dialler against an acceptor that has never heard of keepalive is the case that decides whether
this can fail a stream it should not. It needs two builds, so it got them: an acceptor from the
released `=1.21.5-rc.3` that accepts streams and holds them without ever writing, and a dialler from
`693201bf` that opens one, waits, and watches.

| trials | establishment | outcome | oldest the last inbound frame got | times that age fell back |
|---|---|---|---|---|
| 3 | timed out, 3 of 3 | survived 420 s, 3 of 3 | 450.2 s | 0 |

Nothing arrived, ever. The age of the last inbound frame climbed from the moment the stream was
registered to the end of the hold and never once fell, which is what a peer that cannot send a
liveness frame looks like from here.

That is what makes three trials enough. Arming needs a frame whose discriminant a released peer
cannot produce, and a stream that armed anyway would be pinged and fail about four and a half
minutes after the last thing it heard. These sat silent for 450 seconds and none of them failed, so
they never armed. The property is not a rate to be estimated; one trial that survives already
contradicts it, and three did not.

The same run puts a number on the first half of the advice this project would otherwise be giving
from the source: against a peer with no acknowledgement to send, `wait_established` runs out, three
times out of three. The stream was then held for 420 seconds past that without reporting an error,
which is not the same as having used it. Nothing was read or written after the timeout, so whether
it still carries bytes is still the SDK's claim rather than this run's.

## Two things from the same runs

The branch renamed `wait_established_with_timeout` to `wait_established`, which now takes the
timeout as its argument, between the commit measured in [the establishment
handshake](2026-09-01-establishment-handshake.md) and this one. Nothing about that report changes,
since it pins the commit it ran against, but code written against the older name does not build
here.

One run, the check before the batch, ends with `failed to send mixnet packet due to closed channel
(outside of shutdown!)` as the process exits. The three batch runs do not, so exiting is not on its
own enough to produce it. It reads like the same shape as the panic seen on 1 September, teardown
against traffic still arriving, but one occurrence is a note rather than a finding.

## Limitations

- Three trials an arm, one machine, one afternoon. Six of the nine produced a failure time and those
  six agree to within ten seconds. The other three are right-censored at 420 seconds: their failure
  times, if any, are longer than the hold.
- The three arms all ran between two peers on the same branch. Both mixed-version directions were
  measured separately and by different means: the old dialler against a new acceptor by the
  deployment, twelve trials, and the new dialler against an old acceptor by two images, three
  trials.
- The dialler is the only side that pings. Nothing here measures what an accepting client sees when
  the dialler is the one that dies.
- The clock starts when the rig kills the far side, which is instant. A gateway that fades rather
  than stops would start the count somewhere less definite.
