# The dashboard is empty although traffic is flowing

Your application is calling the gateway and getting answers, but **Dashboard**, **Spend**
and **Quality** all read zero.

## Symptom

Pages render, KPIs read `0` or `—`, `No data for this window yet.`

## What it means

Four causes, in order of likelihood:

1. **You are looking at a different workspace.** The dashboard shows the workspace your
   *session* belongs to; the traffic may belong to another one. Open **Settings → Workspace
   ID** and compare it with the workspace the key belongs to.
2. **The key is an operator-seeded key for another tenant.** A key minted with
   `seed-tenant.sh` belongs to the workspace it was created for, not to the workspace you
   signed in to.
3. **The window is too short.** The chip defaults to 30 days; a `?days=7` bookmark can hide
   a spike from last month.
4. **Traces were dropped.** The telemetry channel is bounded: under extreme load a trace is
   dropped and counted, never retried. The operator view shows the counters
   (`/operator/metrics`: `traces_queued`, `traces_persisted`, `dropped_full`).

Capture being off does **not** empty the dashboard: costs and outcomes are recorded either
way. It only empties **Datasets**.

## Fix

1. **Confirm the request reached us**: the response carried `x-inferon-trace-id`. If it did
   not, the request never reached the gateway.
2. **Find the trace by id** : the operator view can look it up directly; the workspace view
   can filter **Quality → Traffic** by model and time.
3. **Compare workspace ids**: **Settings → Workspace ID** versus the workspace the key was
   minted in.
4. **Widen the window** to 90 days before concluding anything.
5. **Check the counters** if you are the platform operator: `dropped_full` growing means the
   telemetry queue is saturated, which is a capacity problem, not a settings one.

## How to confirm it worked

- The next request's trace id appears in **Quality → Traffic** within a second or two.
- **Overview → Requests** moves.
- The workspace id on the page matches the one on the key's row
  (**Settings → API keys → Last used** updates for the right key).

## When to stop and ask

- The trace id exists on the response and cannot be found anywhere in the dashboard.
- Two workspaces show each other's traffic.
- The counters show drops at normal traffic levels.

_Last verified 2026-11-06 against the live host (master acdaffc)._
