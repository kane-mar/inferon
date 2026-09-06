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
