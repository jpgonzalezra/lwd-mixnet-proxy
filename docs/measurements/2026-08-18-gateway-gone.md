# The gateway left the topology and took the address with it

Date: 2026-08-19. Not a bench run. This is the public testnet serving half failing in place, read
from its own container log, from the API its client builds a topology from, and from three
reproductions against the address it left behind.

## Method

The deployment is the one in [2026-08-15-daily-bandwidth-cliff.md](2026-08-15-daily-bandwidth-cliff.md):
a single `lwd-mixnet-server` container in front of a testnet `lightwalletd-rs`, `restart:
unless-stopped`, its identity on a bind mount. Nothing was staged. The container log survives
restarts, so the whole timeline below was read back from it after the fact, which is the only reason
there is one: the counters do not survive a restart and every one of them read zero.

The topology questions were asked of `validator.nymtech.net` on 2026-08-19 at 13:00 UTC, roughly 23
hours after the event, and asked again at 16:00, by which time the answer had changed. The three
reproductions were run on purpose, from this host, between those two.

Three windows of that log are in
[`raw/2026-08-18-gateway-gone.log`](raw/2026-08-18-gateway-gone.log): the last ordinary night, the
gateway going, and one of the twenty starts that followed. The reproductions ran in throwaway
containers that were removed afterwards, so what is quoted from them here, one line and two counter
sets, is all that was kept.

## What happened

| time (UTC) | event |
|---|---|
| 2026-08-18 00:00:00.069 | the gateway allowance empties for the fifth night and is reclaimed 413 ms later, 7 sphinx packets refused. Ordinary, and the last ordinary thing in this log |
| 2026-08-18 13:29:32 | one topology fetch fails. The client says so and keeps the topology it already has |
| 2026-08-18 14:04:31 | cover traffic starts warning that it has nothing to send |
| 2026-08-18 14:12:03 | the connection to the gateway times out, `os error 110`. Ten reconnection attempts follow, one every five seconds |
| 2026-08-18 14:12:48 | `failed to reconnect after 10 attempts`, then `Failed to send sphinx packet to the gateway 20 times in a row - assuming the gateway is dead`, then `Signalling shutdown from the MixTrafficController`. The process exits |
| 2026-08-18 14:12:49 | restarted. The SDK is asked to wait for the same gateway and waits 4200 s for it |
| ... | 18 more of those, one every 70 minutes |
| 2026-08-19 12:26:45 | the twentieth exit |
| 2026-08-19 13:19 | the registration is moved out of `/state` by hand and the container restarted |
| 2026-08-19 13:20:03 | registered with another gateway in 4 seconds, serving again on a new address |

23 hours. `RestartCount` 20: the first exit because the gateway died under the process, the other 19
because the wait for it ran out.

`/health` answered `{"state":"starting"}` with 503 for all of it, and the compose health check marked
the container unhealthy after the first minute. Neither was being read. `docker ps` showed a
container that was up, because for 70 minutes at a time it was.

## Bonded and gone at the same time

The node stayed in the contract the whole time. What it left was the set of nodes a client can route
through:

| asked of the API on 2026-08-19 | the registered gateway, node 3184 | node 2762, for contrast |
|---|---|---|
| described, 838 nodes at 13:00 | absent | present |
| active entry gateways, 595 at 13:00 | absent | present |
| bonded, 1095 nodes at 13:00 | present, `is_unbonding: false` | present |
| ws port 9000, mix port 1789, at 13:00 | neither answers | not asked |

Node 2762 is the entry gateway the external operator's dialling half was registered with that night,
which is how this rules out the API itself and their machine in one step: the same query, in the same
minute, returned one of the two.

So a bonded node is not a usable one, and the check that matters is the active entry list. Whatever
happened to node 3184 is between its operator and the network. Nothing here measures that node's
quality, only that a client could no longer route to it.

**It came back.** At 16:00 the same two queries found it in both lists, performance 1, roughly a day
after it went. Nothing was watching closely enough to say when in those three hours it returned. This
is the fact worth carrying out of the whole event: while it is happening, a gateway that is gone for
an hour and one that is gone for good are the same observation.

## What it looked like from the dialling side

An external operator ran a heartbeat against the published address inside this window, and their
report is [2026-08-19-midnight-heartbeat.md](2026-08-19-midnight-heartbeat.md). It was meant to catch
a dial inside the 00:00 UTC reclaim. It caught this instead: 19 of 19 finished dials unestablished,
76 opens, 76 unanswered, `establishment_seconds_count` 0, and their own reclaim firing normally in
the middle of it.

Nothing in those numbers says the destination was gone. The ladder reconstructs exactly, which says
their client was healthy, and healthy is what it was: it opened every stream it was asked to open,
into a mixnet that had nowhere to put them.

## Two silences that count the same

Pointing a dialling half at the dead address reproduces the signature, and running it three times on
2026-08-19 split it in two. The node returned between the first run and the second, which is the only
reason the split is visible at all:

| run | time (UTC) | node 3184 in the topology | driven by | `no node with identity` lines |
|---|---|---|---|---|
| A | 13:51 | no | two gRPC calls | 56 |
| B | 15:54 to 16:03 | yes | ten TCP connects and one gRPC call | 0 |
| C | 16:00 to 16:03 | yes | one gRPC call | 0 |

Counters, where they were captured:

| counter | run A, at 13:54 | run B, at 15:56 |
|---|---|---|
| `connections_total` | 7 | 4 |
| `connections_unestablished_total` | 6 | 3 |
| `streams_opened_total` | 24 | 12 |
| `streams_discarded_total{reason="unanswered"}` | 24 | 12 |
| `establishment_seconds_count` | 0 | 0 |

Four opens per connection and nothing answered, in both. From the counters the two runs are the same
event. They are not:

- **The destination gateway is not in the topology.** The client cannot build a packet for it and
  says so at `WARN`, with the filter at `info`, several times per connection:

  ```
  failed to send a repliable message - Failed to prepare packets - no node with identity
  3xLD3rp... is known. 0 reply surbs will be returned
  ```

- **The gateway is there and the client behind that address is not.** The packet is built and sent
  and nothing is waiting for it. There is nothing to log, and nothing at the sender could know.

The first is what the external operator's client hit that night, and their logs carry 154 of those
lines inside the heartbeat window, the first at 23:49:58Z. The second is what any stale address looks
like once its gateway is healthy again, which is the case this deployment left behind.

So the diagnosis exists for one of the two, one layer below where anything can act on it: it never
reaches the stream API, so both look like a deadline expiring. Worth knowing when debugging this
silence, in either direction: the answer may already be in the log at `info`, and its absence does
not mean the transport is at fault.

## What was wrong on our side

Not the outage, which belongs to the gateway. The 23 hours.

This half was built with `with_wait_for_gateway(true)`, so that a gateway briefly out of the topology
delays startup instead of failing it. The SDK's deadline for that is 4200 s, which turned a
registration nobody could use into a restart loop slow enough to look like a process still coming up.
Startup is now bounded by `--gateway-wait-secs`, 300 by default
([ADR 0013](../decisions/0013-bound-the-wait-for-a-registered-gateway.md)).

That shortens nothing about the outage itself. It does two things: the failure looks like one within
minutes, and since the gateway did come back, the loop is also the recovery path. A start that lands
after the node returns simply connects, so bounding the wait would have found it up to 70 minutes
sooner.

Re-registering is still by hand, because it changes the published address and that is not a thing to
do to an unattended deployment. The README has the procedure.

## Limitations

- One gateway, one deployment, one event. Nothing here says how often this happens, and one event is
  not a rate.
- The timeline is reconstructed from the container log after the fact. The counters were wiped by
  every restart and read zero throughout, so nothing independent corroborates the log.
- The API was queried a day after the event, at 13:00 and again at 16:00. What the topology looked
  like at 14:12 on 18 aug is inferred from the client's own `is still not online` lines, not from a
  snapshot taken then, and the node's return is placed inside a two hour window, not timed.
- The reproduction ran a day later and from this host rather than the external operator's. It
  reproduces the signature, not the conditions.
- Runs B and C are one afternoon of a stale address, half an hour apart. That two of them logged
  nothing is enough to show the log line is not guaranteed, and not enough to say what decides it.
- Why node 3184 stopped answering, and what brought it back, is not known here. Its operator was not
  asked.

The gateway identity appears in this report and in its raw log because it was the last component of
the address published on 2026-08-13, so it is already public and cannot be usefully redacted. The
[raw README](raw/README.md) says what is redacted elsewhere and why.
