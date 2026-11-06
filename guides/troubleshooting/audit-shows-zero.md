# The audit shows $0.00

A model's requests are counted, and its cost is zero — or the whole audit reads `$0.00`.

## Symptom

**Spend → by model** shows a model with requests and `$0.00`. Or **Cost audit** shows six
findings and every dollar figure is zero.

## What it means

Two very different cases:

- **A locally hosted model is free, by design.** A provider of type `local` is your own
  hardware: inference costs nothing at the margin, so its traces are costed at exactly
  zero. Its *savings* are still attributed (the counterfactual is reported), and it does not
  appear as "unpriced".
- **A model with no price in the table is unpriced.** Its cost falls back to the default
  row, and the gateway logs a warning once per model. This is a gap in our price table, not
  a free model.

## Fix

1. **Identify the provider kind** of the model's provider (**Providers**): `local` explains
   `$0` completely.
2. **If it is not local**, the model id is not in the price table. Check the exact id — a
   provider may return a different id than you asked for (a router or tier alias, a dated
   variant, a reasoning suffix).
3. **Ask for the id to be priced** if it is a real model on a real provider: this is a
   one-line change to the table, and until then every cost for it is the default row.
4. **If everything is zero**, check the window and the workspace first
   ([the dashboard is empty](../troubleshooting/dashboard-empty.md)) before assuming prices.

## How to confirm it worked

- The model's rows show a non-zero cost after the price table is updated (new requests
  only — existing traces keep the cost they were written with).
- A `local` model keeps `$0` and shows a non-zero **savings** figure.
- The operator's `unpriced` counter stops rising for that model.

## When to stop and ask

- The model is on a paid provider, its id looks standard, and its cost is zero.
- The audit's totals are zero although **Spend** shows real costs — a report problem, not a
  pricing one.
- A cost changed for traffic that predates a price-table change.

_Last verified 2026-11-06 against the live host (master acdaffc)._
