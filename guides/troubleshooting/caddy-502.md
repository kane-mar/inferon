# 502 from Caddy, or one container healthy while the other is not

The dashboard or the API returns `502 Bad Gateway`, or the site is unreachable while
`docker compose ps` shows healthy containers.

## Symptom

```text
HTTP/2 502
via: 1.1 Caddy
```

or the web container is healthy and `https://…/healthz` fails (or the reverse).

## What it means

Caddy routes `/v1/*`, `/auth/*` and `/healthz` to the **proxy** and everything else to the
**web** container. A `502` means Caddy could not reach the container it chose:

| Failing path | Container | Meaning |
|---|---|---|
| `/healthz`, `/v1/*` | `proxy` | the Rust data plane is down, restarting, or crashed |
| `/`, `/dashboard`, `/auth/*` | `web` | the Next.js container is down or failed to start |

A crashing proxy is usually a configuration or schema mismatch: a missing secret, a
migration that did not apply, a `PROVIDER_ENCRYPTION_KEY` that changed (every provider then
fails to decrypt).

## Fix

1. **Read the container's log** before anything else:
   `docker compose logs --tail=50 proxy` (or `web`).
2. **Check the container is actually up**: `docker compose ps` , `Restarting` is not healthy,
   it is a crash loop.
3. **For the proxy**: check the secrets it needs are present (presence, not values):
   `SESSION_SECRET`, `PROVIDER_ENCRYPTION_KEY`, `OPERATOR_API_KEY` when the operator views
   are used.
4. **For migrations**: run `./scripts/migrate.sh` and read the output; a failed migration
   leaves the proxy unable to answer even though it started.
5. **Restart the container, not the stack**: `docker compose --profile prod up -d --no-build proxy`
   (or `web`) : a full `up` also recreates anything whose environment interpolation changed.

## How to confirm it worked

- `curl -s https://your-host/healthz` returns `ok`, and `curl -s https://your-host/api/health`
  returns the web container's health JSON.
- `docker compose ps` shows `healthy` for both.
- A real completion returns `200` with `x-inferon-trace-id`.

## When to stop and ask

- The container is healthy, its log is clean, and Caddy still returns `502` : a network or
  routing problem between them.
- The proxy crash-loops on the same line of its log.
- Providers all fail to resolve after a redeploy: the encryption key changed, and the stored
  credentials cannot be decrypted.

_Last verified 2026-11-06 against the live host (master acdaffc)._
