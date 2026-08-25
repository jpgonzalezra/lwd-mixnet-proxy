# Six thousand trials on the fixed tree, and nothing lost

Date: 2026-08-25. Seven batches against the live mixnet, 6,237 valid trials, on `nym-sdk` from
`develop` at `ece291d`.

[Yesterday's report](2026-08-24-reordering-not-loss.md) found that the silent stream failures this
project was built around are an `Open`/`Data` race whose losing frame the SDK discards without a log,
fixed upstream and not in any release. It left one thing open: whether real loss exists at a rate 400
trials could not resolve. This is that measurement, and the answer is no, down to the resolution
6,237 trials buy.

## Method

Same rig and same rotation of budgets as yesterday. What changed is the size and the client
rotation: seven batches of 1,000 trials, each in its own container, so each batch gets a fresh client
pair.

That is not tidiness. Two runs have already been spoiled by the host rather than the network: a
client that saturated after 200 trials on 2026-08-24, and a laptop lid closed mid-run today. Rotating
per batch bounds the damage to the batch it happens in, which is what it did.

Trace was on for `nym_sdk::mixnet::stream` throughout, so both events of interest are counted:
`buffering seq N until registration`, a frame that arrived before its stream, and
`Cleaning up orphan frames for never-registered stream`, a frame whose `Open` never arrived inside
the orphan TTL.

**Batch 3 is truncated at trial 237.** The host suspended: its log has a 2,196 second gap between
16:03:32 and 16:40:08, everything after it is the machine coming back, and none of it counts. The 237
trials before the gap do.

## What it found

| budget | trials | frames rescued | rate |
|---|---|---|---|
| 1 | 1,560 | 736 | 47.2% |
| 20 | 1,559 | 439 | 28.2% |
| 100 | 1,559 | 236 | 15.1% |
| 400 | 1,559 | 5 | 0.3% |
| **all** | **6,237** | **1,416** | **22.7%** |

One failure in 6,237 trials, and one `never-registered` cleanup. They are the same event, and it is
ours rather than the transport's.

Trial 460 of batch 4 was a budget 400 dial that timed out at 20,002 ms. The last line its process
logged before recording that timeout is stamped 17:14:33.293, and the far side's reply landed at
17:14:33.348, **55 milliseconds later**. The trial line carries no timestamp of its own, so the
deadline fell between those two. By then the dialler had dropped the stream, so the reply arrived for
a stream id that no longer existed, went into the orphan buffer, and was swept 14.9 seconds later.
That interval is the 5 second TTL plus the 10 second sweep, exactly as the SDK documents it.

So no frame went missing on this tree in 6,237 trials. What looked like the one candidate is a slow
success our own deadline turned into a failure.

## What that bounds

Zero events in 6,237 trials puts real loss under roughly 0.05% with 95% confidence, and the point
estimate is 0. Yesterday's 400 trials could only bound it under about 1%.

That is the number the reliability layer's retry half was waiting on. On this tree there is no
residue for it to catch: attempts, rounds and hedging are answers to a failure mode that the orphan
buffer already handles. What survives the finding is the other half: verified establishment with a
deadline, and the counter pair. This run argues for both better than the old failure rate did. The
only trial that failed here failed against a deadline 2.7 times the median for its budget. That is a
slow stream read as a dead one, which is what counting opened streams separately from established
connections is there to expose.

## The budget curve, at ten times the resolution

Yesterday the same curve ran 38 / 24 / 27 / 1 percent on 100 trials per row. With 1,560 per row it is
47.2 / 28.2 / 15.1 / 0.3, and monotonic, which the earlier sample was not.

It supports the same reading and no more: every message carries the budget, the `Open` and the first
`Data` go into the same lane in that order, and a larger budget puts a larger `Open` in front of a
larger `Data`. Somewhere between enqueueing, transmission and reassembly that separation is what
decides the race. These logs still carry no packet departure times, so which of the three does the
work is not settled here.

## Limitations

- One machine, one client pair per batch, one gateway, one evening. The rate is this window's.
- Batch 3 contributes 237 trials rather than 1,000, and its exclusion was decided after seeing the
  gap rather than by a rule set in advance. The gap itself is not a judgement call.
- The bound on real loss is a bound on this tree under this traffic pattern: 64 byte echoes, one
  stream at a time, no concurrency. Nothing here says what happens under load.
- The single failure is attributed to our own 20 second deadline by timestamps 55 ms apart. That is a
  tight margin, so read it as an inference rather than a fact.
- `develop` at `ece291d` is not a release and will change. The numbers belong to that revision.
