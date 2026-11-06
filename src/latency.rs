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
