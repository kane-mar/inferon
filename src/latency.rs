// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! The latency contract, as arithmetic (ITEM-092).
//!
//! `proxy_us` measures the proxy's own pre-dispatch work. The claim is **p50 <
//! 1 ms**; the p95 guard catches a broken cache (which measured 6–28 ms) without
//! pretending the 1 vCPU host's scheduler tail is under our control.
//!
//! The verdict is a pure function so it can be tested, and it is only ever
//! computed over *recent* traffic: a window that reaches back past a deploy would
//! judge today's build by yesterday's code.

/// The claim: p50 under this, in µs.
pub const P50_BUDGET_US: i64 = 1_000;
/// The tail guard (a broken cache, not scheduler jitter), in µs.
pub const P95_GUARD_US: i64 = 5_000;
/// Below this many samples a percentile is noise, not a verdict.
pub const MIN_SAMPLES: i64 = 20;
/// The verdict covers the current build, so it reads a fixed recent window
/// rather than the report's display window.
pub const CONTRACT_WINDOW_HOURS: i64 = 24;

/// The claim is about a **small prompt**. Anything above this had its own
/// measured cost long before the claim was written, so judging a 285k-token
/// request against a 1 ms budget is not a verdict about our code — it is a
/// verdict about the client's payload. The deploy gate and the unit suite use
/// ~35-token requests, which is what "small" was calibrated on.
pub const SMALL_PROMPT_TOKENS: i64 = 4_000;

/// ITEM-136: gateway time is proportional to the payload, so one blended number
/// describes neither end of the traffic. These are the bands the report shows;
/// each carries its own percentiles and its own share of requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    /// Upper bound in prompt tokens; the last band has `None`.
    pub max_tokens: Option<i64>,
    /// The label a reader sees.
    pub label: &'static str,
}

pub const BANDS: &[Band] = &[
    Band { max_tokens: Some(SMALL_PROMPT_TOKENS), label: "small" },
    Band { max_tokens: Some(50_000), label: "medium" },
    Band { max_tokens: Some(150_000), label: "large" },
    Band { max_tokens: None, label: "huge" },
];

/// Which band a prompt of `tokens` belongs to. A trace with no recorded token
/// count is "unknown" — never silently folded into a band it was not measured in.
pub fn band_of(tokens: Option<i64>) -> &'static str {
    let Some(tokens) = tokens else {
        return "unknown";
    };
    for band in BANDS {
        match band.max_tokens {
            Some(max) if tokens <= max => return band.label,
            Some(_) => {}
            None => return band.label,
        }
    }
    "unknown"
}

/// The SQL predicate that keeps a band's rows, so the query and the label can
/// never disagree about where a boundary sits.
pub fn band_bounds(index: usize) -> (i64, Option<i64>) {
    let lower = if index == 0 { 0 } else { BANDS[index - 1].max_tokens.unwrap_or(0) + 1 };
    (lower, BANDS[index].max_tokens)
}

/// One evaluation of the claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub samples: i64,
    pub p50_us: Option<i64>,
    pub p95_us: Option<i64>,
    /// `None` when there are too few samples (or no percentile) to say.
    pub p50_met: Option<bool>,
    pub p95_met: Option<bool>,
    /// ITEM-127: the median prompt size the verdict covered. Gateway time grows
    /// with the payload, so the claim has to be read beside it.
    pub prompt_tokens_p50: Option<i64>,
}

pub fn verdict(samples: i64, p50_us: Option<i64>, p95_us: Option<i64>) -> Verdict {
    let enough = samples >= MIN_SAMPLES && p50_us.is_some();
    Verdict {
        samples,
        p50_us,
        p95_us,
        p50_met: enough.then(|| p50_us.is_some_and(|p50| p50 < P50_BUDGET_US)),
        p95_met: enough.then(|| p95_us.is_some_and(|p95| p95 < P95_GUARD_US)),
        prompt_tokens_p50: None,
    }
}

impl Verdict {
    /// Attach the payload size the percentiles were measured at (ITEM-127).
    pub fn with_payload(mut self, prompt_tokens_p50: Option<i64>) -> Self {
        self.prompt_tokens_p50 = prompt_tokens_p50;
        self
    }
}

#[cfg(test)]
mod tests;
