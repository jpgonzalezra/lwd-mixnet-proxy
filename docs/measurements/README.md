# Measurements

What was measured, what it showed, and what invalidated the runs that had to be thrown out. Each
report says how it was taken, so the numbers can be argued with. Raw output for all of them is in
[`raw/`](raw/README.md).

| report | question | date |
|---|---|---|
| [Does probing and retrying work, and how should attempts be grouped?](2026-08-06-probe-and-retry.md) | whether a probe plus a round of three streams turns a one-in-three transport failure into nothing a wallet sees | 2026-08-06 |
| [The gateway allowance empties at 00:00 UTC](2026-08-15-daily-bandwidth-cliff.md) | what a serving half does when left running for days, and what a bandwidth reset costs when it lands | 2026-08-15, extended 2026-08-17 |
| [What an external dialling half saw against the public testnet](2026-08-15-external-client.md) | client-half counters and a small interleaved bench from an independent operator against the public serving address | 2026-08-15, overnight 2026-08-16 |
| [Dialling through the 00:00 UTC reclaim window](2026-08-19-midnight-heartbeat.md) | one TCP connect a minute for ten minutes either side of midnight, so a live dial sits in the reclaim window | 2026-08-18 / 2026-08-19 |

The first is a bench run against a purpose-built harness ([ADR 0005](../decisions/0005-what-the-measurement-has-to-show.md)
sets what such a run has to show). The second reads the public testnet deployment in place, which is
the only way to see what happens over days. The third is the dialling half of that same public
deployment, run by someone else on another machine and gateway. The fourth is that dialling half
again, with connections in flight through 00:00 UTC — the sample the overnight in the third report
could not take because it was idle.

A rate measured here is one machine and one pair of gateways in one window. The transport's failure
rate moves by an order of magnitude between one hour and the next, so absolute numbers do not
reproduce and comparisons have to be interleaved inside the same window.
