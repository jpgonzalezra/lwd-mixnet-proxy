# The reply-block reserve costs nothing this could measure

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
are the next best thing: neither arm sits entirely in one window of an afternoon whose failure rate
moves by an order of magnitude between hours.

## What it found

| | threshold 0 | default 10 |
|---|---|---|
| trials | 100 | 100 |
| failures | 30% | 33% |
| p50 | 4,188 ms | 4,662 ms |
| p90 | 8,305 ms | 8,750 ms |

A forced round trip before the first reply would cost roughly a second and a half at this budget. The
gap is 474 ms and it does not hold its sign: the default arm was 490 ms slower in the first pair of
blocks and 380 ms faster in the second. Each arm moved more between its own two blocks than the two
arms differ from each other, 850 ms for threshold 0 and 1,720 ms for the default.

So this run found no consistent difference. That is not the same as finding the reserve free, and the
distinction matters here: in the default arm the replenishment certainly happens, since two stored
blocks never exceed a reserve of ten. Whatever it costs is smaller than an afternoon's drift.

## The finding nobody was looking for

A fresh client's first exchange at budget 1 fails about 30% of the time, in both arms. All four
blocks report `accepted 50 | read 50 | written 50`, so the far side answered every one of the 200
trials, and 62 of the 63 failures are replies that did not arrive before the dialler's 20 second
deadline.

An established client at the same budget fails almost never:
[6,237 trials](2026-08-25-six-thousand-trials.md) produced one timeout. So something about a
conversation's first exchange is fragile in a way its later ones are not.

The obvious guess is reply blocks again: a receiver holding two, or ten just requested, has very
little left to retransmit with when the first attempt goes missing, while an established client
carries thousands. The evidence available does not support it. An earlier fresh-client run across
budgets 1, 20, 100 and 400 failed 3, 3, 1 and 2 of 20, and a budget of 400 should have been far safer
than 1 if reply blocks were the whole story. Twenty trials a row settles nothing either way. The
observation stands; the explanation does not.

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
