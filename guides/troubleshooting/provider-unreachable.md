# A provider that tests as unreachable

**Test** on a provider returns **Cannot reach it**, or a request fails with `502`
`upstream_error`. Nothing reached the provider, so nothing was billed.

## Symptom

```json
{"ok":false,"status":null,"models":0,"error":"upstream request failed"}
```

or, on a request:

```json
{"error":{"message":"upstream request failed","type":"upstream_error"}}
```

The trace records `error_class: transport`.

## What it means

The gateway could not open a connection to the base URL. In order of likelihood:

1. **The URL is not what you think.** A `local` provider at `http://127.0.0.1:8080/v1` is
   the *gateway container's* loopback, not your machine.
2. **The path prefix is wrong.** The base URL must end at `/v1`; the gateway appends
   `/chat/completions` (or `/messages`).
3. **TLS.** A self-signed certificate is rejected. Either trust it in the gateway or put a
   real one in front.
4. **Firewall or private network.** The gateway's host cannot route to yours: no VPN, no
   port forward, a security group that only allows your office.
5. **The provider is down.** Rare, but a `503` is the provider's, not ours.

## Fix

1. **Check the base URL** against what the provider documents. `https://api.deepseek.com/v1`
   has no trailing slash and no `/chat/completions`.
2. **Press Test** after each change; it is the same code path a request takes.
3. **For a local model**, prove reachability from the gateway's side, not your laptop:
   `docker exec inferon-proxy-1 wget -qO- http://your-host:11434/v1/models`.
4. **For TLS**, use a certificate the gateway trusts, or terminate TLS at a proxy you
   control.
5. **If it is a `local` model on a laptop**, run the self-hosted stack instead
   ([Inferon on your own network](../inferon-self-hosted.md)) — the cloud cannot reach your
   loopback.

## How to confirm it worked

- **Test** returns **Connected — N models**, and the provider row shows the check time.
- A completion returns `200`; its trace has no `transport` class.
- **Quality → Errors by cause** stops growing `transport`.

## When to stop and ask

- The URL answers from the gateway's container but **Test** fails.
- Test succeeds and completions still `502` — that is a different path (the request itself),
  not connectivity.
- The provider is only reachable over a tunnel you do not control.

_Last verified 2026-11-06 against the live host (master acdaffc)._
