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
| [The gateway left the topology and took the address with it](2026-08-18-gateway-gone.md) | what a published address does when its gateway leaves the network, and why the dialling side cannot tell that from a bad afternoon | 2026-08-18 / 2026-08-19 |
| [The gateway bounced and took the process with it](2026-08-20-gateway-restart.md) | what a gateway restart of about a minute costs a client that is registered with it | 2026-08-20 |
| [The silent stream failures were reordering](2026-08-24-reordering-not-loss.md) | where the unexplained residue goes: the first payload overtakes the `Open` that registers its stream, and the pinned SDK discards it | 2026-08-24 |
| [Six thousand trials on the fixed tree, and nothing lost](2026-08-25-six-thousand-trials.md) | whether anything is lost once the race is handled, at a resolution 400 trials could not reach | 2026-08-25 |
| [A thousand trials on the branch that answers two of these reports](2026-08-26-branch-under-test.md) | what the rig sees once the SDK returns an error for an unroutable recipient and for reorder-buffer loss | 2026-08-26 |
| [The reply-block reserve costs nothing this could measure](2026-08-27-surb-reserve-costs-nothing.md) | whether two SDK defaults landing on the same number costs a round trip before every first reply | 2026-08-27 |

The first is a bench run against a purpose-built harness ([ADR 0005](../decisions/0005-what-the-measurement-has-to-show.md)
sets what such a run has to show). The second reads the public testnet deployment in place, which is
the only way to see what happens over days. The third is the dialling half of that same public
deployment, run by someone else on another machine and gateway. The fourth is that dialling half
again, with connections in flight through 00:00 UTC, the sample the overnight in the third report
could not take because it was idle. The fifth is the serving half on that same night, which turns out
to be what the fourth was measuring. The sixth is the same half two days later, losing the same kind
of node for a minute instead of a day. The seventh goes back to the bench rig and finds that the
failure mode all of this was built around is a race the SDK has since fixed upstream. The eighth
runs that tree fifteen times longer and finds nothing left to catch. The ninth tests the branch that
turns two of these findings into errors a caller can see. The tenth answers a question asked upstream
about two constants, and finds an unrelated one about first exchanges on the way.

A rate measured here is one machine and one pair of gateways in one window. The transport's failure
rate moves by an order of magnitude between one hour and the next, so absolute numbers do not
reproduce and comparisons have to be interleaved inside the same window.
