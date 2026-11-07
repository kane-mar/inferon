// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Eval harness on captured traffic (ITEM-098).
//!
//! A tenant pins a **gold set** from their own verified examples, runs a model
//! against those exact inputs, and scores the answers. Two runs give a
//! regression verdict — the gate ITEM-076's promoter consumes before trusting a
//! distilled adapter.
//!
//! Scoring is deterministic and says what it measured: exact match against the
//! pinned reference when one exists, schema validity when the request carried a
//! contract, otherwise the outcome signal. There is no judge model here — a
//! judge would be another opinion, and the item asks for evidence.

use serde_json::Value;

use crate::quality::Outcome;

/// Below this many scored cases a comparison is not a verdict.
pub const MIN_CASES: i64 = 10;
/// A challenger this far below the baseline (fraction of cases) is a regression.
pub const REGRESSION_MARGIN: f64 = 0.05;

/// How one case was scored — recorded so a comparison is never between
/// different measurements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// The answer matches the pinned reference.
    Exact,
    /// The answer satisfies the request's JSON schema.
    Schema,
    /// Neither a reference nor a schema: the outcome signal decided.
    Outcome,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Schema => "schema",
            Self::Outcome => "outcome",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "exact" => Some(Self::Exact),
            "schema" => Some(Self::Schema),
            "outcome" => Some(Self::Outcome),
            _ => None,
        }
    }
}

/// The score for one case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Score {
    pub method: Method,
    pub pass: bool,
}

/// Normalise text for comparison: trim, collapse whitespace, lowercase.
/// Punctuation is kept — an answer that drops a period is still different.
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Score one answer.
///
/// `reference` is the pinned completion's text (when the gold case has one),
/// `schema` the request's contract (when it has one). Both apply when both are
/// present: an answer that matches the reference but violates the schema is a
/// failure, because the client would have rejected it.
pub fn score(reference: Option<&str>, schema: Option<&Value>, answer: &str, outcome: Outcome) -> Score {
    if let Some(schema) = schema {
        let valid = crate::guard::content_matches_schema(answer, schema);
        if !valid {
            return Score { method: Method::Schema, pass: false };
        }
        return Score { method: Method::Schema, pass: true };
    }
    if let Some(reference) = reference {
        return Score {
            method: Method::Exact,
            pass: normalize(answer) == normalize(reference),
        };
    }
    Score { method: Method::Outcome, pass: outcome == Outcome::Ok }
}

/// A finished run, as stored.
#[derive(Clone, Debug, PartialEq)]
pub struct RunScore {
    pub run_id: uuid::Uuid,
    pub model: String,
    pub scored: i64,
    pub passed: i64,
    /// Cases that never produced a scorable answer (upstream error, partial run).
    pub failed: i64,
}

impl RunScore {
    /// Fraction of scored cases that passed; `None` before the sample floor.
    pub fn rate(&self) -> Option<f64> {
        (self.scored >= MIN_CASES).then(|| self.passed as f64 / self.scored as f64)
    }

    /// A partial run is reported as partial, never scored as zero (AC5).
    pub fn is_partial(&self) -> bool {
        self.failed > 0
    }
}

/// The comparison the gate consumes.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Neither run has the sample floor (or one of them is too incomplete).
    Insufficient { reason: String },
    /// The challenger is at least as good as the baseline.
    Unchanged { baseline: f64, challenger: f64 },
    Improved { baseline: f64, challenger: f64 },
    /// The challenger is worse by more than [`REGRESSION_MARGIN`].
    Regressed { baseline: f64, challenger: f64 },
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Insufficient { .. } => "insufficient",
            Self::Unchanged { .. } => "unchanged",
            Self::Improved { .. } => "improved",
            Self::Regressed { .. } => "regressed",
        }
    }
}

/// Compare a challenger against a baseline.
pub fn compare(baseline: &RunScore, challenger: &RunScore) -> Verdict {
    let (Some(base), Some(challenger_rate)) = (baseline.rate(), challenger.rate()) else {
        return Verdict::Insufficient {
            reason: format!(
                "need {MIN_CASES} scored cases per run (baseline {}, challenger {})",
                baseline.scored, challenger.scored
            ),
        };
    };
    if challenger.is_partial() {
        return Verdict::Insufficient {
            reason: format!("the challenger run is incomplete ({} failed case(s))", challenger.failed),
        };
    }
    if challenger_rate + REGRESSION_MARGIN < base {
        return Verdict::Regressed {
            baseline: base,
            challenger: challenger_rate,
        };
    }
    if challenger_rate > base + REGRESSION_MARGIN {
        return Verdict::Improved {
            baseline: base,
            challenger: challenger_rate,
        };
    }
    Verdict::Unchanged {
        baseline: base,
        challenger: challenger_rate,
    }
}

/// The text an answer carries, whether the completion is OpenAI- or
/// Anthropic-shaped (a gold set can span surfaces).
pub fn answer_text(completion: &Value) -> String {
    if let Some(text) = completion["choices"][0]["message"]["content"].as_str() {
        return text.to_string();
    }
    completion["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

pub mod runner;

#[cfg(test)]
mod tests;
