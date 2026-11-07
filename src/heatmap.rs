// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Model × time heatmap (ITEM-099).
//!
//! Presentation over data that already exists: one bounded aggregate query over
//! the selected window, then a pure pivot. No new hot-path data, no materialised
//! table.
//!
//! An empty bucket is `None`, never `0` — "no traffic" and "fast/healthy" must
//! look different (AC2). Colour is a convenience: every cell prints its value.

/// One measured bucket for one model.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    pub requests: i64,
    pub errors: i64,
    /// 95th percentile of the total duration, in ms (None below a sample).
    pub p95_ms: Option<f64>,
}

impl Cell {
    pub fn error_rate(&self) -> Option<f64> {
        (self.requests > 0).then(|| self.errors as f64 / self.requests as f64)
    }
}

/// Pivot `(model, bucket, cell)` rows into aligned rows.
///
/// Every model gets one entry per bucket in `buckets`; a bucket the query did
/// not return is `None`. Models are sorted by total requests (busiest first),
/// then name, so the table is stable between refreshes.
pub fn pivot(
    buckets: &[String],
    rows: &[(String, String, Cell)],
) -> Vec<(String, Vec<Option<Cell>>)> {
    let mut by_model: std::collections::BTreeMap<&str, Vec<Option<Cell>>> =
        std::collections::BTreeMap::new();
    for (model, bucket, cell) in rows {
        let row = by_model
            .entry(model.as_str())
            .or_insert_with(|| vec![None; buckets.len()]);
        if let Some(index) = buckets.iter().position(|candidate| candidate == bucket) {
            row[index] = Some(*cell);
        }
    }
    let mut pivoted: Vec<(String, Vec<Option<Cell>>)> = by_model
        .into_iter()
        .map(|(model, cells)| (model.to_string(), cells))
        .collect();
    pivoted.sort_by(|a, b| {
        let total = |cells: &[Option<Cell>]| cells.iter().flatten().map(|c| c.requests).sum::<i64>();
        total(&b.1)
            .cmp(&total(&a.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    pivoted
}

/// The bucket expression, whitelisted (never interpolated from user input).
pub fn bucket_expr(step: &str) -> &'static str {
    match step {
        "hour" => "hour",
        _ => "day",
    }
}

#[cfg(test)]
mod tests;
