// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Multi-provider load balancing (ITEM-100).
//!
//! When two providers serve the same model, one of them should not take all the
//! traffic until it fails. Selection distributes across the healthy ones, still
//! weighted by the adaptive policy's health weights (ITEM-069) so a degrading
//! provider is demoted and a zeroed one is skipped.
//!
//! The choice is a **pure function** of the candidates, their weights and a
//! seed: no background balancer, no per-request state beyond a counter, and
//! reproducible in tests. A single-provider model is byte-for-byte the old
//! behaviour (`Reason::Only`).

/// How to spread requests. Weighted is the default: it keeps the health signal
/// meaningful, while round-robin ignores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Weighted,
    RoundRobin,
}

/// Why this provider was chosen (recorded on the trace, AC5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// There was only one eligible provider.
    Only,
    /// Weighted distribution.
    Weight,
    /// Round-robin distribution.
    RoundRobin,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Only => "only",
            Self::Weight => "weight",
            Self::RoundRobin => "round_robin",
        }
    }
}

/// One eligible provider: an opaque slot (the caller's index) and its weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub slot: usize,
    pub weight: f64,
}

/// The chosen slot and why.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub slot: usize,
    pub reason: Reason,
}

/// Spread a sequential counter over the seed space (splitmix64 finaliser).
///
/// A per-request counter increments by one, which would sit inside the first
/// weight bucket for a thousand requests; the mix makes consecutive seeds land
/// in unrelated buckets while staying deterministic.
pub fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Pick a provider.
///
/// Candidates with a non-positive or non-finite weight are unhealthy (or zeroed
/// by the policy) and are skipped. Returns `None` when nothing is eligible.
pub fn choose(candidates: &[Candidate], seed: u64, strategy: Strategy) -> Option<Choice> {
    let eligible: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.weight.is_finite() && candidate.weight > 0.0)
        .collect();
    match eligible.len() {
        0 => None,
        1 => Some(Choice {
            slot: eligible[0].slot,
            reason: Reason::Only,
        }),
        count => {
            let index = match strategy {
                Strategy::RoundRobin => (seed % count as u64) as usize,
                Strategy::Weighted => weighted_index(&eligible, seed),
            };
            Some(Choice {
                slot: eligible[index].slot,
                reason: match strategy {
                    Strategy::Weighted => Reason::Weight,
                    Strategy::RoundRobin => Reason::RoundRobin,
                },
            })
        }
    }
}

/// Weights in thousandths, so the choice stays deterministic integer maths and
/// never depends on a float rounding order.
fn weighted_index(candidates: &[&Candidate], seed: u64) -> usize {
    let steps: Vec<u64> = candidates
        .iter()
        .map(|candidate| ((candidate.weight * 1_000.0).round() as u64).max(1))
        .collect();
    let total: u64 = steps.iter().sum();
    let mut bucket = seed % total;
    for (index, step) in steps.iter().enumerate() {
        if bucket < *step {
            return index;
        }
        bucket -= *step;
    }
    candidates.len() - 1
}

/// The share of requests each candidate should receive, for a sanity check in
/// the dashboard (and in tests).
pub fn share(candidates: &[Candidate], slot: usize) -> f64 {
    let total: f64 = candidates
        .iter()
        .filter(|candidate| candidate.weight.is_finite() && candidate.weight > 0.0)
        .map(|candidate| candidate.weight)
        .sum();
    let mine = candidates
        .iter()
        .find(|candidate| candidate.slot == slot)
        .filter(|candidate| candidate.weight.is_finite() && candidate.weight > 0.0)
        .map_or(0.0, |candidate| candidate.weight);
    if total <= 0.0 {
        0.0
    } else {
        mine / total
    }
}

#[cfg(test)]
mod tests;
