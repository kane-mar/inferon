# Retention or a purge removed the payloads you wanted

You wanted to diff two prompts, build a dataset or re-pin a gold set, and the payloads are
gone.

## Symptom

**Datasets** lists a prompt with requests but no examples, or a prompt that existed last
week is missing entirely. **Payloads** shows the capture switch on.

## What it means

Three mechanisms delete payload text, and they are deliberate:

1. **Retention.** Payloads older than the window (**Keep payloads for (days)**, 30 by
   default) are deleted by the periodic clean-up. Costs, tokens and counts survive.
2. **A purge.** **Datasets → Delete stored payloads** removes every captured payload for the
   workspace immediately.
3. **Capture was off for a period.** Nothing is recovered by turning it back on; there is
   nothing to recover.

Retention is about the *text*. The trace row (model, tokens, cost, outcome) stays, which is
why the dashboard still shows the traffic.

## Fix

1. **Check the window**: **Payloads** shows the retention days and the capture switch.
2. **Check the dates** of the requests you care about against the retention window.
3. **For a dataset you need long-term**, export it (JSONL) or pin it as a gold set: a pinned
   gold set copies the inputs and references out of `traces`, so retention cannot change it
   afterwards.
4. **Raise retention** (`0` keeps forever) *before* the data you need ages out.
5. **If you purged by mistake**, it is not recoverable from the app. A database backup is the
   only route.

## How to confirm it worked

- A new request appears in **Datasets** with its payload, and its trace has `raw_prompt`.
- **Payloads** shows the retention window you set.
- A gold set pinned before the purge still runs — its cases are independent of `traces`.

## When to stop and ask

- Payloads are missing inside the retention window with capture on.
- The purge reports a row count but the datasets list is unchanged.
- You need the deleted text for an audit: that is a backup question, and after a purge the
  answer may be "it is gone".

_Last verified 2026-11-06 against the live host (master acdaffc)._
