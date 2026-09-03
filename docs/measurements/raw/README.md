# Raw output

What the reports in [`../`](../README.md) were written from, kept so their conclusions can be checked
against the lines they came from rather than taken on trust.

## Bench runs

Unedited stdout from `lwd-mixnet-bench`, one file per run, behind
[`../2026-08-06-probe-and-retry.md`](../2026-08-06-probe-and-retry.md). Numbered in the order the runs
happened.

| file | what it was | verdict |
|---|---|---|
| `2026-08-06-run1-sequential.txt` | 300 trials, rounds of 1 | **invalid past trial ~200**: the host began suspending and the client started refusing opens |
| `2026-08-06-run2-hedged-only.txt` | 200 trials, rounds of 3 alone | **unattributable**: no arm to compare against in the same window, and a per-stream rate is not measurable at this round size |
| `2026-08-06-run3-interleaved.txt` | 300 trials, rounds of 1 and 3 interleaved | **the result**: no refused opens, no host suspension |
| `2026-08-06-run4-warm-traffic.txt` | 200 trials, gaps of 0/2/8/20 s, stopped at 148 | **valid to trial ~137**, where the host began suspending |

Each line is one trial: its number, the arm, whether any stream answered, how many rounds it took,
how many opened streams went unanswered, how many opens the local client refused, and how long the
whole trial took. The summary at the foot of each file is what the tool printed at the time, so runs
1 and 2 still show the headline numbers that turned out to be misleading; what was wrong with them,
and how the tool was changed so it says so itself, is in the report.

**One edit was made to these files**: the Nym address each run dialled has been replaced with a
stable label, so `server-A`, `server-B` and `server-C` stand for three different serving halves and
runs 2 and 3 can be seen to have shared one. The addresses themselves were of no use here. They
appeared once per file, in the header, and no number depends on them; the identities were ephemeral
and died with their processes; and the last component of such an address names a **gateway**, a
public node run by someone else. Publishing a failure rate next to a stranger's node would imply
something about it that these runs never measured.

Not kept: the SDK's own stderr. It is large, ANSI-coloured and mostly routine, and the two findings
drawn from it are quoted in the report (`Not enough bandwidth` six minutes into run 1, and 754
`sending_delay_controller` warnings during it).

Reproducing any of these needs the same release binaries, a running `lwd-mixnet-server`, and a host
kept awake. The absolute rates will not reproduce; the underlying distribution spans more than an
order of magnitude, which is why every comparison here is interleaved rather than run as separate
sessions.

## Deployment log

Three files sit behind [`../2026-08-15-daily-bandwidth-cliff.md`](../2026-08-15-daily-bandwidth-cliff.md),
all of them the container log of the public testnet serving half. Almost every line comes from the SDK
rather than from this project.

| file | what it is |
|---|---|
| `2026-08-15-bandwidth-cliff.log` | the 106 lines of the first two nights the bandwidth allowance emptied, 14 and 15 aug |
| `2026-08-17-bandwidth-cliff.log` | the 98 lines of the next two, 16 and 17 aug |
| `2026-08-15-inbound-traffic.log` | every reassembly warning the deployment logged in 96 hours, all 13 of them inside one five-minute window |

Each window runs from the first `run out of bandwidth` to the last line of the burst, in the order and
at the resolution the client logged them. Nothing inside a window was cut.

Terminal colour codes are gone, and so is the container's own timestamp prefix, which leaves the
timestamp the client wrote. The redaction that applies to the bench files, replacing the serving
address and the gateway it registered with, had nothing to catch here: no line in any of these windows
names an address, a gateway or a host.

## External dialling half (forum user Lowo88)

Three files sit behind [`../2026-08-15-external-client.md`](../2026-08-15-external-client.md):
client `/metrics` from one afternoon against the public testnet serving address, a small
`lwd-mixnet-bench` run in the same window, and an overnight check the next day.

| file | what it is |
|---|---|
| `2026-08-15-external-client-metrics.txt` | timestamps, computed rates, and the Prometheus `/metrics` block at 2026-08-15T16:18:05Z |
| `2026-08-15-external-bench.txt` | 20 interleaved r1/r3 trials and the tool summary (ANSI stripped) |
| `2026-08-16-external-overnight.txt` | ~24h health check, flat counters, and the 16 Aug midnight WARN/claim lines |

The dialled Nym address is replaced with `public-testnet-server`. The entry gateway is
labelled `entry-gateway-A`. Publishing a failure rate next to a named public gateway
would imply something about that node that these runs never measured.

## The gateway that left

One file sits behind [`../2026-08-18-gateway-gone.md`](../2026-08-18-gateway-gone.md), again the
container log of the public testnet serving half.

| file | what it is |
|---|---|
| `2026-08-18-gateway-gone.log` | three windows: the allowance emptying for the fifth night, the gateway going and the process exiting, and one of the twenty starts that waited on it afterwards |

Unlike the windows above, this one is filtered rather than whole, and heavily. The 45 seconds in
which the gateway went carry 17,789 `loop cover message` warnings, 724 `delay multipler` warnings and
18 `Client is not authenticated` errors, every one of them a repeat; the 41 lines kept are what is
left. The 70 minute wait keeps 27 of its 54 lines, dropping the same cover traffic. Lines that were
kept are verbatim and in order.

That ratio is worth seeing rather than hiding. The moment this deployment died is 20-odd lines inside
18,500, which is part of why it went unnoticed for a day.

The redaction that applies elsewhere is not applied here. Every wait line names the gateway this half
was registered with, and that identity was the last component of an address published on the forum on
2026-08-13, so there is nothing left to protect. The report says the same.

The three reproductions in that report ran in throwaway containers, and their logs went with them.
What is quoted there is what was kept.

## The gateway that bounced

One file sits behind [`../2026-08-20-gateway-restart.md`](../2026-08-20-gateway-restart.md), the
container log of the same serving half two days later.

| file | what it is |
|---|---|
| `2026-08-20-gateway-restart.log` | four windows: the socket dying and the process exiting, the first of four starts that could not authenticate, the fifth start that could, and the topology refresh afterwards that came back without the gateway in it |

Filtered like the window above and for the same reason. The 47 seconds of reconnection carry 19
repeats of `Client is not authenticated`, of which one is kept, and the sixteen minutes of the whole
window carry 16,531 cover traffic warnings, of which the first and the last are kept. Three of the five
starts are identical to the one kept and are not repeated. Everything else is verbatim and in order.

The gateway identity is not redacted here either: it is the last component of the address published
on 2026-08-19.

## Reordering, not loss

Three files sit behind [`../2026-08-24-reordering-not-loss.md`](../2026-08-24-reordering-not-loss.md),
all of them stdout from the `repro` binary in `contrib/nym/` (the `lightwalletd-rs` repository),
one file per run.

| file | what it is |
|---|---|
| `2026-08-24-develop-orphans.txt` | run A, 400 trials against `nym-sdk` at `ece291d`: every trial line, the summary, and all 90 `buffering seq 0 until registration` traces |
| `2026-08-24-pinned-control.txt` | run B, the pinned release, stopped at trial 272: every trial line and every packet-backlog line, which is what invalidates the tail |
| `2026-08-24-pinned-fresh-client.txt` | run C, a fresh client, 80 trials: trial lines, backlog lines and the summary |

Colour codes are gone and the SDK's own `info` and `warn` noise is dropped, including 1,110
`duplicate fragment received` warnings in run A alone, which are the layer below doing its job and
say nothing about the question. Trial lines, orphan traces, backlog lines and the tool's own summary
are verbatim and in order. Run B has no summary because it was killed before printing one.

No address or gateway identity appears in any of the three: the rig's clients are ephemeral and die
with the container.

## Six thousand trials

Three files sit behind [`../2026-08-25-six-thousand-trials.md`](../2026-08-25-six-thousand-trials.md),
seven batches of the `repro` binary rolled together rather than one file per batch, since the batches
differ only in which client pair ran them.

| file | what it is |
|---|---|
| `2026-08-25-long-run-trials.txt` | every trial line of all seven batches, batch headers in between |
| `2026-08-25-long-run-traces.txt` | every orphan-buffer and orphan-cleanup trace, 1,416 of the first and one of the second |
| `2026-08-25-long-run-summaries.txt` | the tool's own summary per batch, and the run log carrying the note that invalidates batch 3 past trial 237 |

Batch 3 is kept whole in the trial file, including the 763 trials after the host suspended, because
what a suspended host looks like from inside is worth being able to recognise. The report counts only
the first 237.

Colour codes are gone. The SDK's `info` and `warn` noise is dropped, which here is mostly duplicate
fragment warnings from the layer below doing its job. No address or gateway identity appears in any
of the three: the rig's clients are ephemeral and die with their containers.

## The branch under test

Two files sit behind [`../2026-08-26-branch-under-test.md`](../2026-08-26-branch-under-test.md).

| file | what it is |
|---|---|
| `2026-08-26-branch-run.txt` | 1,000 trial lines, the tool's summary, and every orphan-buffer and orphan-cleanup trace of the batch |
| `2026-08-26-unroutable-both-builds.txt` | both dial windows, one per build, from container start to the line that ends each |

The dial windows are whole rather than filtered, which is why DNS retry warnings from an unrelated
resolver sit in them. They are two dozen lines and cutting them would leave less to check than to
read past.

## The reply-block reserve

One file sits behind
[`../2026-08-27-surb-reserve-costs-nothing.md`](../2026-08-27-surb-reserve-costs-nothing.md).

| file | what it is |
|---|---|
| `2026-08-27-surb-threshold-abab.txt` | all four blocks in the order they ran, 50 trial lines and a summary each, with the arm named in the block header |

Colour codes and the SDK's own logging are gone; the trial lines and the tool's summaries are
verbatim. The two abandoned attempts at this experiment are not here: one ended when the host ran out
of disk and the other when Docker was pruned underneath it, and neither produced a number worth
keeping.

## The establishment handshake

Three files sit behind
[`../2026-09-01-establishment-handshake.md`](../2026-09-01-establishment-handshake.md), all of them
stdout from the `repro` binary in `contrib/nym/` built against `nym-sdk` at commit `f5e46b7d`.

| file | what it is |
|---|---|
| `2026-09-01-establishment-500-trials.txt` | run 1, one reused dialler: the ten dead-peer dials, 500 trial lines with their acknowledgement times, and the summary |
| `2026-09-01-fresh-client-run-a.txt` | run 2a, a dialler per trial, **truncated at trial 134** and with no summary |
| `2026-09-01-fresh-client-run-b.txt` | run 2b, the same again, through the collapse to the topology failure that ended it at trial 142 |

Run 1 is kept whole, 618 lines, because at that size filtering hides more than it saves. The two
fresh-client runs are filtered to trial lines, the tool's own output, and every `WARN` and `ERROR`,
which drops about 2,000 lines of client startup per file: each of those runs registers a new client
every trial and each registration announces itself in twenty-odd `INFO` lines.

Run 2a is short two ways, and both are the same accident. The process reading the container's output
was killed, and the container was set to delete itself on exit, so the last trials and the summary
are gone with it. Everything missing is past trial 107, which the report discards anyway.

The 150 repetitions of the `packet_router` panic at the foot of run 2b are the cancellation manager
reporting the same event once per task. Nothing is redacted in any of the three: the rig's clients
are ephemeral and die with their containers.

## Keepalive

One file sits behind [`../2026-09-02-keepalive.md`](../2026-09-02-keepalive.md), stdout from the
`keepalive` binary in `contrib/nym/`, built against `nym-sdk` at commit `693201bf`.

| file | what it is |
|---|---|
| `2026-09-02-keepalive-arms.txt` | four containers in the order they ran: one rig check, then the three arms of three trials each, each with its own summary |
| `2026-09-03-keepalive-watched.txt` | the second pass, two containers, with `last_peer_activity()` polled every five seconds while each trial waits |

Trial lines carry the arm, the outcome and the seconds between the kill and the error. The SDK's
`INFO` and `DEBUG` noise is dropped, which here is client startup and shutdown. Every `WARN` and
`ERROR` is kept: 35 warnings, and one error, at the foot of the first container. Successful pings and pongs are not in
here because the SDK does not log them at any level.

The first container is the check run before the batch. Its single trial is in the file and in no
table.

## Midnight heartbeat (forum user Lowo88)

Two files sit behind [`../2026-08-19-midnight-heartbeat.md`](../2026-08-19-midnight-heartbeat.md):
the dialling-half BEFORE/AFTER `/metrics` plus connect lines, and the SDK reclaim burst
on the same process at 00:00 UTC.

| file | what it is |
|---|---|
| `2026-08-19-midnight-heartbeat.txt` | window header, BEFORE/AFTER Prometheus blocks, 20 TCP connect lines, summary |
| `2026-08-19-midnight-reclaim.log` | ANSI-stripped SDK lines from first `run out of bandwidth` through the last Bandwidth-type WARN of the 19 Aug burst |

No serving address or gateway identity appears in either file. The heartbeat script's
`SUMMARY connects=20 ok=20 fail=0` is local TCP accept only; mixnet establish/discard
is the `/metrics` delta in the same file.
