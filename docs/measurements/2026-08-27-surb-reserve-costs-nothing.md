# The reply-block reserve costs nothing this could measure

> **Amended 2026-09-02.** The section below on first exchanges was wrong, and is retracted and
> replaced in place. The 30% is not a failure rate for anything: every block ends in one unbroken
> run of failures, each block was its own process, and each process began with working trials, so the
> number records where each process fell over. The comparison between the two arms survives. Two
> numbers in it did not: the p90 row gave each arm's slower block, 8,305 ms and 8,750 ms, rather than
> the arm's own figure, and three of the block-to-block differences below were wrong. Both are
> corrected in place, at the percentile definition the rig has always used. What degrades, and the run that isolates it, is in
> [the establishment handshake](2026-09-01-establishment-handshake.md).

Date: 2026-08-27. Two arms of 100 trials each against `nym-sdk` at `develop`, built to answer a
question asked upstream: whether it matters that two SDK defaults are the same number.

A sender attaches `DEFAULT_NUMBER_OF_SURBS` reply blocks to each message, 10 by default. A receiver
holds back `minimum_reply_surb_storage_threshold`, also 10, before it will spend any on a reply. At
equality a receiver whose whole stock is one default message cannot answer until it has asked for
more. The question is what that costs.

## Method

The rig is `contrib/nym/` from `lightwalletd-rs`, with one addition: `--surb-threshold`, which sets
the receiver's reserve through `MixnetClientBuilder::debug_config`. Unset, it reproduces
`connect_new()` exactly rather than approximately.

Two choices make this the worst case the mechanism can have.

**Budget 1**, so each message carries one reply block. A stream sends two messages before the far
side answers, the `Open` and the first write, so the receiver holds two against a reserve of ten and
has to replenish before it can reply at all.

**A fresh dialling client for every trial.** Reply blocks accumulate per sender tag, and a tag lives
as long as the client, so a persistent client stops being short after a few exchanges. Rotating the
dialler makes every trial a first exchange. It costs one registration per trial, which is why the
runs are 100 trials rather than thousands.

**Alternating blocks of 50**, arm A then B then A then B. The reserve is client configuration and
lives in the process, so two arms cannot be interleaved trial by trial the way budgets are. Blocks
are the next best thing: neither arm sits entirely in one window of an afternoon whose latencies move
by more between hours than anything being measured here.

## What it found

| | threshold 0 | default 10 |
|---|---|---|
| trials | 100 | 100 |
| failures | 30% | 33% |
| p50 | 4,192 ms | 4,662 ms |
| p90 | 7,688 ms | 8,456 ms |

The failure row does not mean what it looks like, and the section below replaces it. The percentiles
are over successful trials, so they are unaffected by that: every success in this run falls before
its block's collapse either way.

A forced round trip before the first reply would cost roughly a second and a half at this budget. The
gap is 470 ms and it does not hold its sign: the default arm was 489 ms slower in the first pair of
blocks and 451 ms faster in the second. Each arm moved more between its own two blocks than the two
arms differ from each other, 882 ms for threshold 0 and 1,822 ms for the default.

So this run found no consistent difference. That is not the same as finding the reserve free, and the
distinction matters here: in the default arm the replenishment certainly happens, since two stored
blocks never exceed a reserve of ten. Whatever it costs is smaller than an afternoon's drift.

## The finding nobody was looking for, and what it really was

*This section reported a 30% failure rate for a fresh client's first exchange. Retracted
2026-09-02: there is no such rate.*

The 63 failures are not spread through the run. Each block ends in one unbroken run of them.

| block | arm | failures | trailing run |
|---|---|---|---|
| 1 | threshold 0 | 18/50 | 16, trials 35 to 50 |
| 2 | default 10 | 22/50 | 21, trials 30 to 50 |
| 3 | threshold 0 | 12/50 | 10, trials 41 to 50 |
| 4 | default 10 | 11/50 | 11, trials 40 to 50 |

58 of the 63 are those four runs. Each block was a separate invocation of the tool, and each one
worked for its first 29 to 40 trials before entering its terminal run. Blocks 1 to 3 do carry a
failure or two before that point, five across the 142 trials that ran before a collapse, which is
the sort of rate the rest of these reports see. The per-arm figures are two numbers recording where a
process stopped working, added together. Two fresh-client runs on 2026-09-01, close in shape rather
than identical, failed 0 and 1 in their first hundred trials.

What does happen is a degradation that has only ever been seen in this configuration, one dialling
client built per trial, and never in 500 trials through a reused one.
[The establishment handshake](2026-09-01-establishment-handshake.md) has the runs and what they do
and do not settle.

So the explanation this section reached for, a receiver too short of reply blocks to retransmit with,
was answering a question that was never posed. The budget sweep that refused to support it was right.

## Limitations

- One machine, one gateway, one afternoon, and 100 trials per arm. This bounds the reserve's cost at
  roughly the size of the drift, not below it.
- Alternating blocks spread each arm across two windows. They do not make arbitrary drift fall
  equally, and the default arm always ran second within a pair.
- The percentiles are over successful trials only, so a shift that turns slow replies into timeouts
  would move the failure rate rather than the latency.
- Only streams were tested. A single anonymous message with a reply owed is the case where the
  reserve binds hardest, and nothing here measures it.
- Two earlier attempts at this experiment were discarded, one when the host ran out of disk mid-run
  and one when Docker was pruned underneath it. Neither is in the numbers above.
