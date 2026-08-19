# The gateway allowance empties at 00:00 UTC

Date: 2026-08-15, extended 2026-08-17 with two further events and a correction. Not a bench run. This
is the public testnet serving half in ordinary operation, read from its own log and `/metrics`.

## Method

One `lwd-mixnet-server` container in front of a testnet `lightwalletd-rs`, started 2026-08-13 14:04:29
UTC and still on its first process 96 hours later. Nothing was staged or driven: the numbers below
are whatever the deployment did while it sat there.

All four events happened with nothing connected. Over the whole 96 hours the half opened one upstream
connection, during a smoke test on 13 aug at 15:03:11, and `lwd_mixnet_server_connections_in_flight`
was 0 at every midnight. So what the gateway refused to send was cover traffic, not anyone's
request.

The lines the four events produced are in
[`raw/2026-08-15-bandwidth-cliff.log`](raw/2026-08-15-bandwidth-cliff.log) for the first two and
[`raw/2026-08-17-bandwidth-cliff.log`](raw/2026-08-17-bandwidth-cliff.log) for the rest.

## What happens

Four times in 96 hours the gateway reported the client's bandwidth allowance at zero. Once a night,
always inside the first second of 00:00 UTC:

| | 14 aug | 15 aug | 16 aug | 17 aug |
|---|---|---|---|---|
| first `run out of bandwidth` | 00:00:00.078109Z | 00:00:00.079051Z | 00:00:00.231279Z | 00:00:00.086186Z |
| `managed to claim testnet bandwidth` | 00:00:00.298429Z | 00:00:00.357774Z | 00:00:00.755900Z | 00:00:00.288394Z |
| time at zero | **220 ms** | **279 ms** | **525 ms** | **202 ms** |
| sphinx packets the gateway refused | 11 | 9 | 15 | 5 |
| claim attempts | 12 | 10 | 16 | 6 |
| last line of the window | 00:00:00.442611Z | 00:00:00.591272Z | 00:00:00.885293Z | 00:00:00.474376Z |

The shape is the same every night:

```
00:00:00.078109  WARN  run out of bandwidth when attempting to send the message! we got 0.00 B
                       available, but needed at least 2.36 kiB to send the previous message
00:00:00.087865  WARN  Not enough bandwidth. Trying to get more bandwidth, this might take a while
00:00:00.112058 ERROR  Failed to send sphinx packet(s) to the gateway: gateway returned an error
                       response: insufficient bandwidth available to process the request.
                       required: 2413B, available: 0B
00:00:00.298429  INFO  managed to claim testnet bandwidth
```

Three details are worth keeping.

**The claims go out in parallel and only one is answered.** Every `Not enough bandwidth` starts its
own attempt, one succeeds, and the replies to the others arrive as `received illegal message of type
'Bandwidth' in an authenticated client`. That count is attempts minus one on all four nights: 11 of
12, 9 of 10, 15 of 16, 5 of 6. So the warning is the price of firing them together rather than a
fault.

**The cost is not a constant.** With two events it looked like a quarter of a second and about ten
refused packets. Four span 202 ms to 525 ms and 5 to 15 packets. Nothing in the log says what sets
the size, and nothing in a deployment should be sized against these numbers.

**This is free testnet bandwidth.** The client runs in what the SDK calls disabled credentials mode
and claims its allowance without a credential, which is also why recovery is immediate. A mainnet
client pays with ticketbooks (`Ticketbooks stored: 0` in this log, every three hours), so nothing here
says what the same second looks like when the allowance has to be bought.

## Why a fraction of a second is worth writing down

The refusal is returned to the sender, logged, and that is the end of it. Nothing above that layer is
told: no closed stream, no error on a write, no timeout that fires early. A stream whose first payload
sits in one of those refused packets ends up in exactly the state
[ADR 0003](../decisions/0003-probe-every-stream-before-the-wallet-uses-it.md) and
[ADR 0004](../decisions/0004-deadlines-are-the-only-close.md) are built around: `accept()` fires on the
far side and the `read` never returns.

That link is inference. All four events landed on an idle server, so no stream was observed dying in
one, and it stays that way until someone catches a dial inside the window.

The window is also far too small to explain the failure rate. A few hundred milliseconds a day is
between two and six parts per million; per-stream failure rates between 2% and 51% are not made of
this. What it gives is one confirmed way for the transport to turn a payload into silence, on a
schedule you can point at.

## Correction: the streams below are not mostly local testing

The counters in the next section were first published on 2026-08-15 with the note that most of the
sample was one machine's smoke testing. That is wrong, and the log says so.

The nym client logged 13 `duplicate fragment received` warnings in 96 hours. All 13 fall between
16:11:29Z and 16:16:43Z on 15 aug, inside a window that an independent operator ran against this
deployment and
[reported on the forum](https://forum.zcashcommunity.com/t/lwd-mixnet-proxy-light-wallet-grpc-over-the-nym-mixnet-and-what-three-days-of-measuring-it-found/57000/6).
Nothing else in the log is inbound traffic. The one upstream connection is dated 13 aug, two days
before that window.

Arrival times cannot be read off the log directly, which is why this took a second look. Both frequent
outcomes, a stream that never introduces itself and a stream that carries no request, are logged at
`debug`, and the container runs at `info`. Reassembly warnings are the only inbound signal that
survives at that level, so they are the evidence. The 13 lines are in
[`raw/2026-08-15-inbound-traffic.log`](raw/2026-08-15-inbound-traffic.log).

How the 150 splits between 13 aug and that window is still not visible from here. The 45% below is
not one machine talking to itself, though, and the dialling side of it belongs to the operator who
ran it. Credit for the window, and for the client-side numbers that go with it, is theirs (forum user
Lowo88).

## The rest of the 96 hours

Counters at 2026-08-17 13:53 UTC, over the whole process lifetime, unchanged from the 2026-08-15
reading:

| | |
|---|---|
| streams that arrived from the mixnet | 150 |
| never delivered a handshake inside the 30 s deadline (`streams_rejected_total{reason="unanswered"}`) | 67, 45% |
| introduced themselves but carried no request (`streams_without_request_total`) | 82 |
| reached the upstream | 1 |

Nothing has arrived since 15 aug. Read the 45% as an observation over one afternoon's traffic rather
than a rate: the sample is small. It is the same first-payload loss the bench measured from the
dialling side, seen now from the receiving end.

82 of 83 carrying no request is the expected shape here, not a second finding. A probe never sends
one, a hedged dial keeps one stream of three, and the bench sends no request at all, so with a single
real call in 96 hours almost every accepted stream should be idle.

The log is quiet otherwise: 44 `Not enough bandwidth` warnings in 96 hours, every one of them inside
the four seconds above, the 13 reassembly warnings, and two failed topology refreshes that kept the
previous topology and moved on.

## What this settles, and what it does not

The [2026-08-06 report](2026-08-06-probe-and-retry.md) ends by saying that nothing in it shows what a
client does over a day, and that `Not enough bandwidth` six minutes into a run makes that a real
question. Part of the answer: a gateway registration survived 96 hours without a restart, and each of
the four times the allowance emptied, the client refilled it by itself in under 600 ms. Whatever ended
those two-hour runs, it was not this.

The 2026-08-15 version of this report could not say whether the reset belonged to this gateway, to
testnet accounting, or to the network. It does now, at least in part. A dialling half run by someone
else, on another machine and another gateway, hit the same reset on the night of 16 aug and recovered
the same way without restarting. Two independent clients emptying and refilling inside the same window
is hard to explain as a property of either registration.

## A fifth night, and the end of this process

The process kept going one more night. On 18 aug the allowance emptied at 00:00:00.069Z and was
claimed back at 00:00:00.482Z: 413 ms, 7 sphinx packets refused, which sits inside the range of the
other four and adds nothing to the argument except another instance of it.

Fourteen hours later the same process ended, and not because of any of this. Its gateway stopped
answering, the SDK declared it dead, and what followed took the public address down for 23 hours.
That is [2026-08-18-gateway-gone.md](2026-08-18-gateway-gone.md). It also closes the third limitation
below: this registration lasted 120 hours in the end, and what ended it was not the allowance.

## Limitations

- Testnet and free claims. The mainnet path through ticketbooks is unmeasured.
- Every event landed with nothing connected, so the cost to a live stream is reasoned, not observed.
  Catching one needs a dial in flight at 00:00:00, which nobody has done yet.
- 96 hours is not weeks, and the question was about weeks.
- The window edges are the first and last lines the client logged. How long the gateway had been at
  zero before the client tried to send is not in the log.
- The second client is one report from one operator, taken from their own logs rather than measured
  here. It settles that the window is not local to this registration; it does not establish who owns
  the schedule.
