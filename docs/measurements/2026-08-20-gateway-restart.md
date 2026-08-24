# The gateway bounced and took the process with it

Date: 2026-08-20, written up on 2026-08-24. Not a bench run. This is the public testnet serving half
losing its entry gateway for about a minute, read from its own container log.

Same failure class as [2026-08-18-gateway-gone.md](2026-08-18-gateway-gone.md), which cost 23 hours.
This one cost 73 seconds, and the reason for the difference is not the fix that came out of it.

## Method

The deployment is unchanged: one `lwd-mixnet-server` container in front of a testnet
`lightwalletd-rs`, `restart: unless-stopped`, its identity on a bind mount, registered with gateway
2719 since 2026-08-19 13:20 UTC. Nothing was staged and nothing was watching. The timeline was read
back from the container log four days later, which is again the only record: the counters live in
memory and every restart zeroes them.

Four windows of that log are in
[`raw/2026-08-20-gateway-restart.log`](raw/2026-08-20-gateway-restart.log).

## What happened

| time (UTC) | event |
|---|---|
| 13:05:10.114 | the gateway socket dies mid-send: broken pipe, then connection reset by peer. Reconnection starts in the same millisecond |
| 13:05:10 to 13:05:57 | ten reconnection attempts, one every five seconds. The hostname is unreachable (`os error 101`), the published fallback address refuses (`os error 111`) |
| 13:05:57.176 | `failed to reconnect after 10 attempts`, then `assuming the gateway is dead`, then shutdown. The reply store is flushed and both sqlite pools are closed |
| 13:05:58 to 13:06:09 | four starts. Each one reaches gateway authentication in about 2.5 s, is refused, and exits. Compose restarts it each time |
| 13:06:20 | the fifth start authenticates |
| 13:06:23.103 | accepting mixnet streams again, same address |
| 13:11:23 to 13:16:23 | the client's own gateway is absent from the topology it fetched. 16,531 warnings in five minutes, then it is back |

73 seconds between the last stream the old process could have served and the first the new one could.
`RestartCount` 5. The published address never changed, so nothing had to be announced.

Since then the same process has been up four days, to 2026-08-24 13:06 UTC, with no reconnection
attempt logged at all. Across two registrations this half has now spent eleven days connected, and a
websocket ageing out has never been what ended a run.

## Why this cost a minute and the last one cost a day

Both events are the same sentence, the entry gateway stopped being reachable, and they take different
branches inside the SDK:

| | 2026-08-18 | 2026-08-20 |
|---|---|---|
| what the node did | left the topology and stayed out for ~23 h | stayed in the topology, refused TCP for ~70 s |
| what startup did | waited for it, 4200 s per attempt | failed authentication in ~2.5 s |
| what recovered it | re-registering by hand, on a new address | the fifth restart, on the same address |

So the bounded startup wait from
[ADR 0013](../decisions/0013-bound-the-wait-for-a-registered-gateway.md) never fired here. What
recovered this outage is `restart: unless-stopped`, and it worked for the ordinary reason that the
failure was fast, loud and repeated. The bounded wait is for the other branch, and this event says
nothing about whether it helps there.

Worth keeping both in mind before reading anything into a restart count. Five restarts in 22 seconds
is a healthy recovery. Twenty restarts in 23 hours is an outage nobody was reading.

## The reconnection budget is shorter than a gateway restart

Ten attempts five seconds apart is 47 seconds, and both numbers are
`nym-gateway-client` defaults: `DEFAULT_RECONNECTION_ATTEMPTS` is 10, `DEFAULT_RECONNECTION_BACKOFF`
is 5 s. The node was accepting connections again about 70 seconds after it went. A client with a live
registration and a working process was killed by a bounce it would have ridden out on a slightly
longer budget.

`GatewayClientConfig` has `with_reconnection_attempts` and `with_reconnection_backoff`, so the knobs
exist. What is missing is the plumbing: `nym-client-core` builds its gateway client with
`GatewayClientConfig::new_default()` and sets only the credentials mode, and neither the SDK builder
nor `DebugConfig` carries either field. An application on `nym-sdk` cannot change this today. Raised
with Nym, and small enough to be worth offering a patch for.

## The reply store is discarded on every start

All five starts logged the same three lines: load the existing surb database, close it, then

```
setup_fs_reply_surb_backend: Failed to setup persistent storage backend for our reply needs: The
loaded data is inconsistent - it seems that on the last shutdown the client hasn't finished the data
flush. We're going to create a fresh database instead
```

The shutdown that preceded them had flushed cleanly, in the log four lines above:
`PersistentReplyStorage is flushing all reply-related data to underlying storage`, then
`Closing sqlite pool: /state/persistent_reply_store.sqlite`. So the store is thrown away after a
shutdown that did finish its flush, and the reply blocks accumulated over days go with it.

That is not free. The SDK attaches 10 reply blocks per message by default
(`DEFAULT_NUMBER_OF_SURBS`), and a receiver holds back `minimum_reply_surb_storage_threshold`, also
10, before it will spend any on a reply. Ten is not greater than ten, so on a store that starts empty
the first reply of a conversation cannot be sent at all until the receiver has asked the sender for
more and been answered. Every restart puts both halves back in that state.

## Five minutes without its own gateway

One topology refresh after the recovery, at 13:11:23, the client's fetched topology came back without
node 2719 in it. It was connected to that node over a working websocket at the time. What it could
not do was build a cover traffic path through it, and it said so 16,531 times in five minutes, 55 a
second, until the next refresh had the node back. The refresh interval is 5 minutes, so this is one
cycle exactly.

Nothing above the SDK could see it. No dial arrived in that window, so nothing here says what a wallet
would have got if one had.

## Limitations

- One deployment, one gateway, one event. Not a rate, and not evidence about how often nodes bounce.
- Reconstructed from the container log after the fact. The counters were zeroed by the restarts and
  read zero, so nothing corroborates the log independently.
- The API was not queried during the event. Whether node 2719 actually left the active entry list at
  13:11 or merely failed to appear in one refresh is not known here. The client's own warnings are
  the only evidence either way.
- Why the node stopped answering is between it and its operator. Nothing here measures that node's
  quality, only that a client could not reach it for about seventy seconds.
- The 73 seconds is process downtime. No dial was in flight, so it is not a measured wallet-visible
  failure.
- The gateway identity appears in this report and its raw log because it is the last component of the
  address published on 2026-08-19, so there is nothing left to protect.
