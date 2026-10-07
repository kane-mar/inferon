//! Cost anomaly alerts (ITEM-020).
//!
//! Pure decision logic only — no DB, no HTTP. The evaluation schedule lives
//! in `evaluate_alerts` (alerts.rs DB layer / main.rs task); these functions
//! decide *whether* a tenant's spend or waste behavior is anomalous and what
//! the fire/clear lifecycle should do, so the rules are unit-testable.
//!
//! Two classes (per AC1):
//!   (a) spend_velocity — rolling-window spend exceeds N× the tenant's own
//!       trailing hourly baseline, or an absolute floor (whichever is higher)
//!   (b) waste_spike — waste-flagged requests (ITEM-006 linter) in a day pass
//!       a per-tenant daily threshold
//!
//! Lifecycle (per AC3): a (tenant, class) fires when an episode starts and
//! re-fires at most once per 24h while the condition persists (no spam); a
//! clear event is recorded when the condition drops. Money math is integer
//! cents throughout (ITEM-016 convention).

use rust_decimal::Decimal;

/// Per-tenant alert thresholds. `None` values mean the DB default applies
/// (see migration 003) — the SQL layer fills them before calling the logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertThresholds {
    /// N in "spend exceeds N× its own baseline".
    pub spend_multiplier: Decimal,
    /// Absolute hourly spend floor in cents (fires even with no baseline).
    pub spend_floor_cents: i64,
    /// Waste-flagged requests per day that count as a spike.
    pub waste_daily_threshold: i64,
}

impl AlertThresholds {
    /// Conservative defaults (migration 003 column defaults): 5× baseline or
    /// $1/hr floor; 50 waste-flagged requests/day. Tuned after ITEM-018
    /// brings real traffic (decision note in the item file).
    pub fn defaults() -> Self {
        Self {
            spend_multiplier: Decimal::new(5, 0),
            spend_floor_cents: 100,
            waste_daily_threshold: 50,
        }
    }
}

/// Spend velocity signal over a rolling window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpendSignal {
    /// Total spend in the rolling window (cents, integer).
    pub window_cents: i64,
    /// Tenant's own trailing baseline: average spend per hour over the last
    /// 24h (cents, integer). 0 when there is no history yet.
    pub baseline_per_hour_cents: i64,
}

impl SpendSignal {
    /// Threshold = max(floor, multiplier × hourly baseline), rounded up to
    /// whole cents (ceil — never under-flag spend, matching cost_to_cents).
    pub fn threshold_cents(&self, t: &AlertThresholds) -> i64 {
        let baseline = i64::try_from(
            (t.spend_multiplier * Decimal::from(self.baseline_per_hour_cents)).ceil(),
        )
        .unwrap_or(i64::MAX);
        baseline.max(t.spend_floor_cents)
    }

    /// Anomalous when the window strictly exceeds the threshold.
    pub fn is_anomalous(&self, t: &AlertThresholds) -> bool {
        self.window_cents > self.threshold_cents(t)
    }
}

/// Waste spike signal: waste-flagged requests in the trailing day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasteSignal {
    pub flagged_requests_24h: i64,
}

impl WasteSignal {
    /// Anomalous when the daily flagged count passes the threshold (strictly
    /// greater than — "past a daily threshold" per AC1b).
    pub fn is_anomalous(&self, t: &AlertThresholds) -> bool {
        self.flagged_requests_24h > t.waste_daily_threshold
    }
}

/// What the lifecycle should do for one (tenant, class) on this evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireAction {
    /// Condition is true and no episode is open — start one and deliver.
    Start,
    /// Condition is true and the open episode already delivered recently —
    /// stay silent (no spam, AC3).
    KeepSilent,
    /// Condition is true, the episode is open, and ≥24h passed since the last
    /// delivery — re-fire (a long-running condition reminds daily).
    ReFire,
    /// Condition is false but an episode is open — record the clear.
    Clear,
    /// Condition is false and nothing is open — nothing to do.
    Idle,
}

/// Decide the lifecycle action for one (tenant, class).
///
/// `is_open` = an episode is currently firing; `last_fired_at_ms` = the last
/// delivery time (None when open but never delivered, which cannot happen —
/// Start always delivers). `now_ms` = evaluation time.
pub fn decide_fire(
    is_open: bool,
    last_fired_at_ms: Option<i64>,
    now_ms: i64,
    anomalous: bool,
    re_fire_after_ms: i64,
) -> FireAction {
    match (anomalous, is_open) {
        (true, false) => FireAction::Start,
        (true, true) => match last_fired_at_ms {
            Some(t) if now_ms - t >= re_fire_after_ms => FireAction::ReFire,
            _ => FireAction::KeepSilent,
        },
        (false, true) => FireAction::Clear,
        (false, false) => FireAction::Idle,
    }
}

/// Re-fire period: at most one delivery per (tenant, class) per day (AC3).
pub const REFIRE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// Alert class identifiers persisted in `alerts.class`.
pub const SPEND_VELOCITY: &str = "spend_velocity";
pub const WASTE_SPIKE: &str = "waste_spike";
/// ITEM-047: per-entity monthly-limit classes; the episode class is
/// `key_spend_limit:<uuid>` / `provider_spend_limit:<uuid>` so each entity has
/// its own lifecycle.
pub const KEY_SPEND_LIMIT: &str = "key_spend_limit";
pub const PROVIDER_SPEND_LIMIT: &str = "provider_spend_limit";

/// The per-entity episode class (kept long — `alerts.class` is TEXT).
pub fn limit_class(kind: &str, id: uuid::Uuid) -> String {
    format!("{kind}:{id}")
}

/// Evaluation window sizes (SQL interval text, kept with the class constants
/// so the DB layer and this module cannot drift).
pub const SPEND_WINDOW_HOURS: i64 = 1;
pub const SPEND_BASELINE_HOURS: i64 = 24;
pub const WASTE_WINDOW_HOURS: i64 = 24;

/// A persisted alert episode as stored in the `alerts` table.
#[derive(Debug, Clone)]
pub struct AlertRow {
    pub tenant_id: uuid::Uuid,
    pub class: String,
    pub status: String,
    pub first_fired_at_ms: i64,
    pub last_fired_at_ms: i64,
}

/// Read the current open (firing) episodes for every tenant/class. The
/// evaluation loop turns each row into the `is_open` input of `decide_fire`.
pub async fn open_episodes(
    pool: &sqlx::PgPool,
) -> Result<Vec<AlertRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows: Vec<(uuid::Uuid, String, String, i64, i64)> = sqlx::query_as(
        "SELECT tenant_id, class, status,
                EXTRACT(EPOCH FROM first_fired_at)::bigint * 1000,
                EXTRACT(EPOCH FROM last_fired_at)::bigint * 1000
         FROM alerts WHERE status = 'firing'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(tenant_id, class, status, first_fired_at_ms, last_fired_at_ms)| AlertRow {
            tenant_id,
            class,
            status,
            first_fired_at_ms,
            last_fired_at_ms,
        })
        .collect())
}

/// Load a tenant's thresholds from `tenant_configurations`, falling back to
/// the conservative defaults when the tenant has no configuration row.
pub async fn thresholds_for(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
) -> Result<AlertThresholds, Box<dyn std::error::Error + Send + Sync>> {
    let row: Option<(rust_decimal::Decimal, i64, i32)> = sqlx::query_as(
        "SELECT alert_spend_multiplier, alert_spend_floor_cents, alert_waste_daily_threshold
         FROM tenant_configurations WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|(multiplier, floor_cents, waste)| AlertThresholds {
            spend_multiplier: multiplier,
            spend_floor_cents: floor_cents,
            waste_daily_threshold: i64::from(waste),
        })
        .unwrap_or_else(AlertThresholds::defaults))
}

/// Aggregate a tenant's signals in one query over `traces` (baselines read
/// from the traces table, not a new counter — AC1 + tech note). Cost math is
/// done in SQL (numeric) and rounded to integer cents at the boundary — the
/// Rust layer never sees a float.
struct TenantSignals {
    window_cents: i64,
    baseline_per_hour_cents: i64,
    flagged_requests_24h: i64,
}

async fn aggregate_signals(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
) -> Result<TenantSignals, Box<dyn std::error::Error + Send + Sync>> {
    // Window = last SPEND_WINDOW_HOURS; baseline = the SPEND_BASELINE_HOURS
    // *before* the window (a runaway does not dilute its own baseline); waste
    // = flagged requests in the last WASTE_WINDOW_HOURS. The WHERE clamps the
    // scan to the widest window; FILTERs pick the subsets.
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT
            COALESCE(ROUND(SUM(upstream_cost_usd) FILTER
                (WHERE created_at >= NOW() - $2 * INTERVAL '1 hour') * 100)::bigint, 0),
            COALESCE(ROUND(SUM(upstream_cost_usd) FILTER
                (WHERE created_at >= NOW() - $3 * INTERVAL '1 hour'
                  AND created_at < NOW() - $2 * INTERVAL '1 hour') * 100)::bigint, 0),
            COUNT(*) FILTER
                (WHERE created_at >= NOW() - $4 * INTERVAL '1 hour'
                  AND waste_detected IS NOT NULL AND waste_detected <> '{}'::jsonb)
         FROM traces WHERE tenant_id = $1 AND created_at >= NOW() - $3 * INTERVAL '1 hour'",
    )
    .bind(tenant_id)
    .bind(SPEND_WINDOW_HOURS)
    .bind(SPEND_BASELINE_HOURS + SPEND_WINDOW_HOURS)
    .bind(WASTE_WINDOW_HOURS)
    .fetch_one(pool)
    .await?;
    Ok(TenantSignals {
        window_cents: row.0,
        baseline_per_hour_cents: row.1 / SPEND_BASELINE_HOURS,
        flagged_requests_24h: row.2,
    })
}

/// Evaluate every tenant and deliver webhooks for episodes that started or
/// re-fired. One full pass of the alert schedule (runs on a timer in main).
pub async fn run_evaluation(
    pool: &sqlx::PgPool,
    webhook_url: Option<&str>,
    client: &reqwest::Client,
) {
    let tenant_ids: Vec<uuid::Uuid> = match sqlx::query_as::<_, (uuid::Uuid,)>("SELECT id FROM tenants")
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows.into_iter().map(|(id,)| id).collect(),
        Err(e) => {
            tracing::error!("alerts: tenant list failed: {e}");
            return;
        }
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    for tenant_id in tenant_ids {
        let deliveries = match evaluate_tenant(pool, tenant_id, now_ms).await {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("alerts: evaluation failed for {tenant_id}: {e}");
                continue;
            }
        };
        for (class, fired_at_ms) in deliveries {
            deliver_webhook(client, webhook_url, tenant_id, &class, fired_at_ms).await;
        }
    }
}

/// Best-effort webhook POST (AC4). Failures are logged — the alert row is the
/// source of truth and the operator view lists it regardless.
async fn deliver_webhook(
    client: &reqwest::Client,
    webhook_url: Option<&str>,
    tenant_id: uuid::Uuid,
    class: &str,
    fired_at_ms: i64,
) {
    let Some(url) = webhook_url else {
        tracing::debug!("alerts: no ALERT_WEBHOOK_URL configured; skipping delivery");
        return;
    };
    let body = serde_json::json!({
        "event": "alert",
        "tenant_id": tenant_id.to_string(),
        "class": class,
        "fired_at_unix_ms": fired_at_ms,
    });
    match client.post(url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!("alerts: delivered {class} for {tenant_id}");
        }
        Ok(resp) => tracing::error!("alerts: webhook returned {}: {}", resp.status(), url),
        Err(e) => tracing::error!("alerts: webhook delivery failed: {e}"),
    }
}

/// Operator view data: every episode with its lifecycle timestamps, newest
/// first. Status is included so the view can show firing and cleared states.
pub struct AlertViewRow {
    pub tenant_id: uuid::Uuid,
    pub class: String,
    pub status: String,
    pub detail: serde_json::Value,
    pub threshold: serde_json::Value,
    pub first_fired_at: String,
    pub last_fired_at: String,
    pub cleared_at: Option<String>,
}

pub async fn list_alerts(
    pool: &sqlx::PgPool,
    window_days: i64,
) -> Result<Vec<AlertViewRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows: Vec<(
        uuid::Uuid,
        String,
        String,
        serde_json::Value,
        serde_json::Value,
        String,
        String,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT tenant_id, class, status, detail, threshold,
                first_fired_at::text, last_fired_at::text, cleared_at::text
         FROM alerts
         WHERE created_at >= NOW() - ($1 * INTERVAL '1 day')
         ORDER BY last_fired_at DESC",
    )
    .bind(window_days)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(tenant_id, class, status, detail, threshold, first_fired_at, last_fired_at, cleared_at)| {
                AlertViewRow {
                    tenant_id,
                    class,
                    status,
                    detail,
                    threshold,
                    first_fired_at,
                    last_fired_at,
                    cleared_at,
                }
            },
        )
        .collect())
}

/// Evaluate one tenant: read its signals, apply the pure fire/clear lifecycle
/// for both classes, and persist. Returns the episodes that started or
/// re-fired (each needs a delivery attempt, AC4).
pub async fn evaluate_tenant(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    now_ms: i64,
) -> Result<Vec<(String, i64)>, Box<dyn std::error::Error + Send + Sync>> {
    let thresholds = thresholds_for(pool, tenant_id).await?;
    let signals = aggregate_signals(pool, tenant_id).await?;
    let spend = SpendSignal {
        window_cents: signals.window_cents,
        baseline_per_hour_cents: signals.baseline_per_hour_cents,
    };
    let waste = WasteSignal {
        flagged_requests_24h: signals.flagged_requests_24h,
    };

    let mut deliveries = Vec::new();
    for (class, anomalous, detail) in [
        (
            SPEND_VELOCITY.to_string(),
            spend.is_anomalous(&thresholds),
            serde_json::json!({
                "window_cents": signals.window_cents,
                "baseline_per_hour_cents": signals.baseline_per_hour_cents,
            }),
        ),
        (
            WASTE_SPIKE.to_string(),
            waste.is_anomalous(&thresholds),
            serde_json::json!({"flagged_requests_24h": signals.flagged_requests_24h}),
        ),
    ] {
        let open = find_open(pool, tenant_id, &class).await?;
        match decide_fire(
            open.is_some(),
            open.as_ref().map(|r| r.last_fired_at_ms),
            now_ms,
            anomalous,
            REFIRE_AFTER_MS,
        ) {
            FireAction::Start => {
                insert_episode(pool, tenant_id, &class, now_ms, &detail, &thresholds).await?;
                deliveries.push((class, now_ms));
            }
            FireAction::ReFire => {
                touch_episode(pool, tenant_id, &class, now_ms).await?;
                deliveries.push((class, now_ms));
            }
            FireAction::Clear => clear_episode(pool, tenant_id, &class, now_ms).await?,
            FireAction::KeepSilent | FireAction::Idle => {}
        }
    }
    evaluate_limits(pool, tenant_id, now_ms, &thresholds, &mut deliveries).await?;
    Ok(deliveries)
}

/// ITEM-047: entities over their monthly cap (month-to-date spend >= limit).
struct LimitBreach {
    class: String,
    detail: serde_json::Value,
}

async fn limit_breaches(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
) -> Result<Vec<LimitBreach>, Box<dyn std::error::Error + Send + Sync>> {
    let mut breaches = Vec::new();

    let keys: Vec<(uuid::Uuid, String, i64, i64)> = sqlx::query_as(
        "SELECT k.id, k.label, k.spend_limit_cents,
                COALESCE(ROUND(SUM(t.upstream_cost_usd) * 100)::bigint, 0)
         FROM api_keys k
         LEFT JOIN traces t ON t.api_key_id = k.id
              AND t.created_at >= date_trunc('month', NOW())
         WHERE k.tenant_id = $1 AND k.spend_limit_cents > 0
         GROUP BY k.id, k.label, k.spend_limit_cents
         HAVING COALESCE(ROUND(SUM(t.upstream_cost_usd) * 100)::bigint, 0) >= k.spend_limit_cents",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    for (id, label, limit_cents, spent_cents) in keys {
        breaches.push(LimitBreach {
            class: limit_class(KEY_SPEND_LIMIT, id),
            detail: serde_json::json!({
                "api_key_id": id.to_string(),
                "label": label,
                "spent_cents": spent_cents,
                "limit_cents": limit_cents,
            }),
        });
    }

    let providers: Vec<(uuid::Uuid, String, i64, i64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.spend_limit_cents,
                COALESCE(ROUND(SUM(t.upstream_cost_usd) * 100)::bigint, 0)
         FROM providers p
         LEFT JOIN traces t ON t.provider_id = p.id
              AND t.created_at >= date_trunc('month', NOW())
         WHERE p.tenant_id = $1 AND p.spend_limit_cents > 0
         GROUP BY p.id, p.name, p.spend_limit_cents
         HAVING COALESCE(ROUND(SUM(t.upstream_cost_usd) * 100)::bigint, 0) >= p.spend_limit_cents",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    for (id, name, limit_cents, spent_cents) in providers {
        breaches.push(LimitBreach {
            class: limit_class(PROVIDER_SPEND_LIMIT, id),
            detail: serde_json::json!({
                "provider_id": id.to_string(),
                "name": name,
                "spent_cents": spent_cents,
                "limit_cents": limit_cents,
            }),
        });
    }

    Ok(breaches)
}

/// Fire/re-fire an episode per breaching key/provider and clear episodes whose
/// entity is back under its cap (ITEM-047 AC2).
async fn evaluate_limits(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    now_ms: i64,
    thresholds: &AlertThresholds,
    deliveries: &mut Vec<(String, i64)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let breaches = limit_breaches(pool, tenant_id).await?;
    let active: std::collections::HashSet<String> =
        breaches.iter().map(|b| b.class.clone()).collect();

    for breach in &breaches {
        let open = find_open(pool, tenant_id, &breach.class).await?;
        match decide_fire(
            open.is_some(),
            open.as_ref().map(|r| r.last_fired_at_ms),
            now_ms,
            true,
            REFIRE_AFTER_MS,
        ) {
            FireAction::Start => {
                insert_episode(pool, tenant_id, &breach.class, now_ms, &breach.detail, thresholds)
                    .await?;
                deliveries.push((breach.class.clone(), now_ms));
            }
            FireAction::ReFire => {
                touch_episode(pool, tenant_id, &breach.class, now_ms).await?;
                deliveries.push((breach.class.clone(), now_ms));
            }
            FireAction::Clear | FireAction::KeepSilent | FireAction::Idle => {}
        }
    }

    // Clear any open limit episode whose entity is back under its cap.
    let open: Vec<(String,)> = sqlx::query_as(
        "SELECT class FROM alerts
         WHERE tenant_id = $1 AND status = 'firing'
           AND (class LIKE 'key_spend_limit:%' OR class LIKE 'provider_spend_limit:%')",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    for (class,) in open {
        if !active.contains(&class) {
            clear_episode(pool, tenant_id, &class, now_ms).await?;
        }
    }
    Ok(())
}

async fn find_open(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    class: &str,
) -> Result<Option<AlertRow>, Box<dyn std::error::Error + Send + Sync>> {
    let row: Option<(uuid::Uuid, String, String, i64, i64)> = sqlx::query_as(
        "SELECT tenant_id, class, status,
                EXTRACT(EPOCH FROM first_fired_at)::bigint * 1000,
                EXTRACT(EPOCH FROM last_fired_at)::bigint * 1000
         FROM alerts WHERE tenant_id = $1 AND class = $2 AND status = 'firing'",
    )
    .bind(tenant_id)
    .bind(class)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(tenant_id, class, status, first_fired_at_ms, last_fired_at_ms)| AlertRow {
            tenant_id,
            class,
            status,
            first_fired_at_ms,
            last_fired_at_ms,
        },
    ))
}

/// Persist the episode's signal values + thresholds at fire time (AC2: window
/// values and threshold live on the row) and open it.
async fn insert_episode(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    class: &str,
    now_ms: i64,
    detail: &serde_json::Value,
    thresholds: &AlertThresholds,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        "INSERT INTO alerts (tenant_id, class, detail, threshold, first_fired_at, last_fired_at)
         VALUES ($1, $2, $3::jsonb, $4::jsonb, to_timestamp($5 / 1000.0), to_timestamp($5 / 1000.0))",
    )
    .bind(tenant_id)
    .bind(class)
    .bind(detail)
    .bind(serde_json::json!({
        "multiplier": thresholds.spend_multiplier.to_string(),
        "spend_floor_cents": thresholds.spend_floor_cents,
        "waste_daily_threshold": thresholds.waste_daily_threshold,
    }))
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

async fn touch_episode(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    class: &str,
    now_ms: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        "UPDATE alerts SET last_fired_at = to_timestamp($3 / 1000.0)
         WHERE tenant_id = $1 AND class = $2 AND status = 'firing'",
    )
    .bind(tenant_id)
    .bind(class)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

async fn clear_episode(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    class: &str,
    now_ms: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        "UPDATE alerts SET status = 'cleared', cleared_at = to_timestamp($3 / 1000.0)
         WHERE tenant_id = $1 AND class = $2 AND status = 'firing'",
    )
    .bind(tenant_id)
    .bind(class)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn th(mult: &str, floor: i64, waste: i64) -> AlertThresholds {
        AlertThresholds {
            spend_multiplier: Decimal::from_str_exact(mult).unwrap(),
            spend_floor_cents: floor,
            waste_daily_threshold: waste,
        }
    }

    #[test]
    fn limit_classes_are_entity_scoped() {
        let id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        assert_eq!(limit_class(KEY_SPEND_LIMIT, id), format!("key_spend_limit:{id}"));
        assert_ne!(
            limit_class(KEY_SPEND_LIMIT, id),
            limit_class(PROVIDER_SPEND_LIMIT, id)
        );
    }

    #[test]
    fn defaults_are_conservative_and_whole() {
        let d = AlertThresholds::defaults();
        assert_eq!(d.spend_multiplier, Decimal::new(5, 0));
        assert_eq!(d.spend_floor_cents, 100);
        assert_eq!(d.waste_daily_threshold, 50);
    }

    #[test]
    fn spend_threshold_is_max_of_multiplier_baseline_and_floor() {
        let t = th("5", 100, 50);
        // baseline $1/hr (100c) × 5 = 500c > floor 100c -> threshold 500c
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 100 }.threshold_cents(&t),
            500
        );
        // tiny baseline $0.01/hr (1c) × 5 = 5c < floor -> floor wins (100c)
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 1 }.threshold_cents(&t),
            100
        );
        // no baseline at all -> floor applies (a new tenant can still trip the floor)
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 0 }.threshold_cents(&t),
            100
        );
    }

    #[test]
    fn spend_anomaly_strictly_exceeds_threshold() {
        let t = th("5", 100, 50);
        let signal = SpendSignal { window_cents: 500, baseline_per_hour_cents: 100 };
        assert!(!signal.is_anomalous(&t), "equal to threshold is not anomalous");
        let over = SpendSignal { window_cents: 501, baseline_per_hour_cents: 100 };
        assert!(over.is_anomalous(&t), "one cent over threshold fires");
    }

    #[test]
    fn spend_floor_catches_runaway_without_baseline() {
        let t = th("5", 100, 50);
        // A brand-new tenant with no history burns $2 in the window: 200c > floor 100c.
        let runaway = SpendSignal { window_cents: 200, baseline_per_hour_cents: 0 };
        assert!(runaway.is_anomalous(&t));
        // Normal dev traffic stays under the $1/hr floor.
        let dev = SpendSignal { window_cents: 3, baseline_per_hour_cents: 2 };
        assert!(!dev.is_anomalous(&t));
    }

    #[test]
    fn fraction_multiplier_rounds_threshold_up() {
        // multiplier 1.5 × 3c baseline = 4.5c -> ceil 5c
        let t = th("1.5", 0, 50);
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 3 }.threshold_cents(&t),
            5
        );
        // 4c window still under the 5c threshold; 5c is not > 5c either.
        let s = SpendSignal { window_cents: 5, baseline_per_hour_cents: 3 };
        assert!(!s.is_anomalous(&t));
    }

    #[test]
    fn waste_spike_passes_daily_threshold() {
        let t = th("5", 100, 50);
        assert!(!WasteSignal { flagged_requests_24h: 50 }.is_anomalous(&t));
        assert!(WasteSignal { flagged_requests_24h: 51 }.is_anomalous(&t));
        // A different per-tenant threshold applies.
        let strict = th("5", 100, 10);
        assert!(WasteSignal { flagged_requests_24h: 11 }.is_anomalous(&strict));
    }

    #[test]
    fn lifecycle_start_fires_a_new_episode() {
        assert_eq!(
            decide_fire(false, None, 1_000, true, REFIRE_AFTER_MS),
            FireAction::Start
        );
    }

    #[test]
    fn lifecycle_silent_until_24h_elapse_while_firing() {
        // Opened at t=0, delivered at t=0. Evaluations at +1h and +23h stay silent.
        assert_eq!(
            decide_fire(true, Some(0), 3_600_000, true, REFIRE_AFTER_MS),
            FireAction::KeepSilent
        );
        assert_eq!(
            decide_fire(true, Some(0), 23 * 3_600_000, true, REFIRE_AFTER_MS),
            FireAction::KeepSilent
        );
    }

    #[test]
    fn lifecycle_refires_after_24h_while_still_firing() {
        assert_eq!(
            decide_fire(true, Some(0), 24 * 3_600_000 + 1, true, REFIRE_AFTER_MS),
            FireAction::ReFire
        );
    }

    #[test]
    fn lifecycle_clear_records_episode_end() {
        assert_eq!(
            decide_fire(true, Some(0), 3_600_000, false, REFIRE_AFTER_MS),
            FireAction::Clear
        );
    }

    #[test]
    fn lifecycle_idle_when_nothing_open_and_condition_false() {
        assert_eq!(
            decide_fire(false, None, 0, false, REFIRE_AFTER_MS),
            FireAction::Idle
        );
    }
}
