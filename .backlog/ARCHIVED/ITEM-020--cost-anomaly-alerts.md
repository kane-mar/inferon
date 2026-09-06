## ITEM-020: Cost Anomaly Alerts

**As an** Inferon operator (and a tenant)
**I want** to be told when spend or waste behavior departs from its own baseline
**So that** a runaway integration is caught in hours, not at the next dashboard visit

**Acceptance Criteria:**
1. Two alert classes evaluated on a schedule: (a) spend velocity — a tenant's spend in a rolling window exceeds N× its own trailing baseline (or an absolute floor), and (b) waste spikes — per-request waste classes (ITEM-006 linter) that already exist, aggregated past a daily threshold
2. Alerts are persisted (table) with class, tenant, window values, threshold, and first/last-fired timestamps; an operator view lists them
3. Each (tenant, class) fires at most once per day until the condition clears (no spam); a clear event is recorded
4. Delivery reaches an operator without new infra: MVP = webhook POST to a configurable URL (compose env); email/Slack are later options
5. Thresholds are per-tenant configurable (extend `tenant_configurations`); defaults are conservative enough not to fire on normal dev traffic

**Technical Notes:**
- Reuses the telemetry worker's existing Postgres flush cadence for the evaluation schedule — no new daemon in the MVP
- Cost math stays integer-cents on the flush path (ITEM-016 convention); baselines read from the traces table, not a new counter
- Decision needed at refinement: alert classes + default multipliers (this item captures the shape; the numbers are tuned after ITEM-018 brings real traffic)

## Verification record (2026-09-06)

- **AC1 (two classes, scheduled)**: `src/alerts.rs` evaluates spend velocity
  (rolling 1h window vs max(5× trailing-24h hourly baseline, floor)) and waste
  spikes (flagged requests in 24h past the daily threshold). Runs on an
  in-process 60s interval task in main — same binary, no new daemon/service.
  Pure decision functions unit-tested (11 tests).
- **AC2 (persisted + operator view)**: `alerts` rows carry class, tenant,
  window values (`detail`), threshold, first/last-fired + cleared timestamps
  (migration 003). Operator view `/v1/dashboard/api/operator/alerts?days=7`
  lists firing + cleared episodes (gate tests 401/403).
- **AC3 (no spam + clear event)**: fire/clear lifecycle unit tests — Start on
  a new episode, silent while firing (<24h), ReFire at ≥24h, Clear records
  `cleared_at`. Live-verified on the durable host: fire → clear → re-fire.
- **AC4 (webhook, no new infra)**: best-effort `ALERT_WEBHOOK_URL` POST from
  compose env (verified live — netcat sink on the compose network received
  `{"event":"alert","class":"spend_velocity",...}`).
- **AC5 (per-tenant thresholds)**: `alert_spend_multiplier`,
  `alert_spend_floor_cents`, `alert_waste_daily_threshold` on
  `tenant_configurations`, conservative defaults (5× / $1 floor / 50 per day);
  live test set floor to 1 cent to prove a $3 window spike fires.
- **Bug caught at the live seam**: aggregate FILTER placement was invalid SQL
  (syntax error at FILTER) — unit tests can't catch SQL syntax; the
  deploy-seam verification did. Fixed + re-verified.
