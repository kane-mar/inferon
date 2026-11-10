# Storing payloads, keeping them, and hiding personal data

Inferon stores costs and outcomes for every request either way. Whether it stores the
**prompt and the answer** is your choice, because that is what a dataset is made of.

## The capture switch

**Routing → Controls → Store prompts and answers**. Off by default in the API sense:
nothing is stored until it is on. With it off:

- costs, tokens, latency, outcomes and error classes are still recorded;
- no prompt or answer is kept, so **Datasets** stays empty and no gold set can be pinned;
- turning it on later does not recover anything from before.

Datasets, evals and the cost audit all read captured payloads, so this switch governs all
three.

## Retention

**Keep payloads for (days)** : 30 by default, `0` keeps them forever. A periodic clean-up
deletes older payloads. Costs and counts survive; only the text goes.

Payloads are also removed the moment you ask: **Datasets → Delete stored payloads** purges
everything captured for the workspace:

```json
{"purged": 1284}
```

Retention is not a compliance guarantee : see the PII section below for what detection
can and cannot see.

## Personal data detection

Detection runs on every captured payload by default. It recognises **shapes**, and it
never records what it found : only the class and a count:

| Class | Shape |
|---|---|
| `email` | `name@example.com` |
| `phone` | international and national formats |
| `card` | 13–19 digits passing the Luhn check |
| `ssn` | `123-45-6789` shape |
| `api_key` | `sk-…`, `ghp_…`, `AKIA…` and similar |

**It is not a compliance control.** A name, an address or a free-form identifier is
invisible to it, and an obfuscated address defeats it. Treat a clean scan as "no shapes
found", never as "no personal data".

## The two redaction modes

| Mode | What it does | Where the effect shows |
|---|---|---|
| **Hide personal data in storage** | Replaces matches with a placeholder (`<email>`) before the payload is stored, so datasets, exports and audit shares never carry them | Datasets, exports |
| **Hide personal data in transit** | Replaces matches in the request that goes to the provider, on 90% of requests: a 10% holdout is sent unredacted so you can see what redaction costs in answer quality | Answers from the provider |

Both are off until you turn them on. Redaction is idempotent: a second pass finds nothing
left to replace, so a stored redacted payload stays stable.

The transit mode can change answers : the model no longer sees the real value. That is why
it is partial and why the holdout exists; compare the two groups before relying on it
everywhere.

## How to confirm it worked

- **Payloads** shows the capture switch, the retention window, and counts by PII class for
  the window.
- A captured trace shows `pii` with per-class counts and which modes were in force.
- After a purge, **Datasets** is empty and the models' spend is unchanged.

## When to stop and ask

- Capture is on and **Datasets** is empty : check the window and whether purge or
  retention removed the payloads ([payloads were purged](../troubleshooting/payloads-purged.md)).
- PII counts are zero for traffic you know contains email addresses : the shapes may be
  obfuscated, or the payloads are not being captured.
- You need redaction for compliance, not hygiene: this is shape matching, and a compliance
  control needs a different tool.

_Last verified 2026-11-06 against the live host (master acdaffc)._
