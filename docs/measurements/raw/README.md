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
