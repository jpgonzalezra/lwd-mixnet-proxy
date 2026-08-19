# 0013. Bound the wait for a registered gateway

## Context

The serving half's address is `identity.encryption@gateway`. The gateway is part of what clients
write down, so this half registers once and keeps that registration for the life of the deployment.

It builds its client with `with_wait_for_gateway(true)`, so a gateway briefly out of the topology
delays startup instead of failing it. Registration itself was seen to fail on 2 of 15 attempts, and
a half that gives up on the first bad answer would need a supervisor to paper over it. The SDK takes
a bool there and waits 4200 seconds on it.

On 2026-08-18 the gateway of the public testnet instance stopped answering. At 14:12:48Z the SDK
declared it dead and the process exited. The node stayed bonded in the contract, so nothing about it
looked wrong from the chain, but it left the API's described set and the active entry gateway list,
which is what the client builds its topology from. Every restart after that waited the full 70
minutes, exited, and was restarted into the same wait: 20 restarts, 23 hours, `/health` answering 503
throughout while `docker ps` reported a container that was up. Another operator measured against the
address inside that window and read the silence as a bad night on the transport, which is exactly
what it looks like from the dialling side.

The node then came back. It reappeared in both lists the next afternoon, roughly a day after it went,
which is the fact this decision has to be built around: gone is not the same as gone for good, and
neither is knowable while it is happening.

A dead registration and a slow one are the same event to a supervisor. What separates them is how
long they last, and 70 minutes is long enough to read as neither.

## Decision

**Bound the wait in the caller: `--gateway-wait-secs`, 300 by default.** The SDK's knob is a bool, so
the deadline has to come from outside it. On expiry this half logs what it was waiting for and exits
non-zero, which puts a container into a visible restart loop within minutes instead of hours.

**Exit rather than re-register.** Re-registering would restore service in seconds and change the
published address while doing it. An address that rotates itself on a restart nobody watched is worse
than an outage: clients that wrote the old one down get the same silence, and now there is no way
back to it. Rotating is the operator's call, and the README says how.

**The dialling half is left alone.** It connects with `connect_new()`, so it holds no registration
and picks a gateway at every start. It has nothing to be stuck on.

## Consequences

- A gateway that goes costs minutes of confusion instead of a day. The outage is not shortened by
  this. What changes is that it looks like one.
- When the gateway does come back, the restart loop is what notices, so a shorter wait also
  reconnects sooner: up to 70 minutes of an attempt were spent waiting on a gateway that had already
  returned.
- Five minutes is a guess, not a measurement. It has to sit above a slow registration and below an
  operator's patience. If a legitimately slow start ever trips it, the cost is a restart, and the
  flag is there to raise.
- The wait is bounded, not removed. A gateway that comes back within five minutes is still ridden
  out, which is the case the SDK flag was turned on for.
- An operator who wants the old behaviour sets `--gateway-wait-secs 4200` and gets it back exactly.
- This does not detect the condition; it only stops hiding it. Whether the registered gateway is
  still in the topology is a question for the API, and nothing here asks it.
