## ITEM-121: Troubleshooting — operations (answers, dashboards, sign-in, deploys, data)

**As a** self-hoster or owner who has changed something
**I want** a page per symptom that tells me what changed and how to confirm the fix
**So that** I stop guessing between a config mistake, a deploy mistake and a bug

**Slices from ITEM-113.** Nine pages under `guides/troubleshooting/`, same inherited
shape (Symptom → What it means → Fix → How to confirm it worked → When to stop and
ask) and the same `Last verified` footer.

1. `empty-refused-truncated.md` — `max_tokens`, reasoning tokens, the `finish_reason`
   values and the truncation marker.
2. `quality-dropped.md` — read the comparison group, then roll back the downgrade.
3. `provider-unreachable.md` — local model, TLS, firewall, path prefix, the probe.
4. `dashboard-empty.md` — wrong key, capture off, wrong window.
5. `audit-shows-zero.md` — a model with no price in the table, and how to add one.
6. `sign-in-loop.md` — `SESSION_SECRET` mismatch, cookie domain, OAuth callback URL.
7. `caddy-502.md` — web container healthy while the proxy is not (or the reverse).
8. `migrations-not-applied.md` — the order that avoids it, and how to confirm the
   schema version.
9. `payloads-purged.md` — retention and purge removed what the user wanted to diff.

**Acceptance Criteria**
1. Each page is written from a failure we have actually had (the ITEM log and the live
   host's logs) and quotes the real message.
2. Each page states the check that proves it — a command, a log line, a table, a
   dashboard page — and where to stop and ask.
3. Cross-links both ways with the configuration page that causes the symptom.
4. `python3 scripts/check-guides.py` passes; `Last verified` carries the date.
5. Live on the durable host.

**Source:** ITEM-113 breakdown, 2026-11-05.
