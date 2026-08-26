# 0012. Spend the attempt budget in whole rounds

> **Amended 2026-08-26.** The rate this decision is calibrated against is real and is what every
> `nym-sdk` release still produces, so the decision stands as written. What changed is why: the
> failures are not the transport losing payloads, they are a first `Data` frame overtaking the `Open`
> that registers its stream, which the pinned SDK discards without a log. Nym fixed that on `develop`
> in August 2026. On that tree the rate collapses, and 6,237 trials found nothing lost at all. Revisit
> this budget when a **release** carries the fix, not before. Evidence:
> [reordering, not loss](../measurements/2026-08-24-reordering-not-loss.md) and
> [six thousand trials](../measurements/2026-08-25-six-thousand-trials.md).

## Context

Two settings govern dialling: how many streams one connection may open in total
(`--probe-attempts`), and how many of those go out together (`--probe-concurrency`). Concurrency has
been three since [0006](0006-no-pool-of-pre-probed-streams.md), on measurement. The total was four,
which was never measured and never written down.

Four does not divide by three. `dial` opens `min(concurrency, remaining)` per round, so the ladder was
a round of three and then a round of **one**. That second round is sequential retry: it pays a full
probe deadline for a single stream, which is the shape the 2026-08-06 run compared against and
rejected, at a p99 of 31.3 s against 6.3 s. That sequential round only ever ran on connections the
transport had already failed once.

[0005](0005-what-the-measurement-has-to-show.md) sets the arithmetic: failures between rounds were
measured to be independent, so k attempts turn a per-stream rate p into roughly p^k. The budget is the
exponent, and p is not a constant. Here it has ranged from 2% to 51% across four days, and an
[operator elsewhere](../measurements/2026-08-15-external-client.md) measured 0.60 on an
afternoon of their own, so a budget calibrated for a good window is not calibrated at all:

| p | k=4 | k=6 | k=9 |
|---|---|---|---|
| 0.35 | 1.5% | 0.18% | 0.008% |
| 0.55 | 9.2% | 2.8% | 0.46% |
| 0.60 | 13.0% | 4.7% | 1.0% |

That operator's client half, on the old default, left 2 of 30 connections with nothing. Independence
predicts 9.2% at the rate their own bench measured that hour. The ladder behaved as designed. The
design was short.

## Decision

**The attempt budget is spent in whole rounds, and the default is six: two rounds of three.**

Six buys the square of what one round buys, and it costs nothing in the tail. The round count is
unchanged at two, so the worst case is still two probe deadlines. Only connections whose first round
came up empty open the extra streams, one in six at p=0.55. For the connections that do retry, the
second round gets faster rather than slower: three streams racing instead of one waiting out its
deadline alone.

Nine was considered and rejected as a default. It reaches 1% even at p=0.60, but it adds a third
deadline, and a 30 s tail is the number [0006](0006-no-pool-of-pre-probed-streams.md) was written to
avoid. An operator who knows their window is bad can set it.

## Consequences

- A wallet-visible failure rate roughly p^6 instead of p^4. On the worst afternoon on record that is
  4.7% instead of 13%; on an ordinary one it is under a fifth of a percent.
- Up to three more streams and three more reply-block budgets per connection, spent only when the
  first round found nothing. Average cost at p=0.55 is about half a stream per connection.
- No change to establishment latency for a connection that works first time, which is most of them.
- `--probe-attempts` should stay a multiple of `--probe-concurrency`. Nothing enforces it, since a
  short final round is a reasonable thing to ask for deliberately; the README carries the warning
  instead.
- This rests on independence between rounds, measured once, on one afternoon, at one round size. A
  transport whose failures became correlated would make the exponent a fiction and the extra streams
  pure cost. `lwd_mixnet_client_first_round_failures_total` against
  `lwd_mixnet_client_connections_unestablished_total` is the pair that would show it.
