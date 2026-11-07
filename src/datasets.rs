// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Verified-example curation (ITEM-086).
//!
//! A trace becomes a dataset pair only when its completion is *verified*: the
//! request succeeded, the whole completion was captured, nothing was truncated,
//! and — when the request carried a JSON schema — the content satisfies it.
//! Anything else is recorded as excluded with the reason, so the dataset surface
//! can show why traffic did not qualify instead of silently dropping it.
//!
//! Thumbs-down is applied by the read model: the thumb is written after the
//! trace, so it cannot be part of the write-time decision.

use serde_json::Value;

use crate::guard;
use crate::quality::Outcome;
use crate::TraceMessage;

/// Whether a trace is a usable instruction pair, and why not if it is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classification {
    pub state: &'static str,
    pub reason: Option<&'static str>,
    /// Stable identity of the completion, for exact dedupe (only when a
    /// completion was captured). Two identical completions count once.
    pub completion_hash: Option<String>,
}

impl Classification {
    fn excluded(reason: &'static str, hash: Option<String>) -> Self {
        Self {
            state: "excluded",
            reason: Some(reason),
            completion_hash: hash,
        }
    }
}

/// Classify one trace. `None` = no completion was captured at all (nothing to
/// exclude: there is no pair to fail).
pub fn classify(trace: &TraceMessage) -> Option<Classification> {
    let completion = trace.raw_completion.as_ref()?;
    let content = completion["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let hash = crate::sha256_hex(&completion.to_string());

    if trace.raw_completion_capped {
        return Some(Classification::excluded("capped", Some(hash)));
    }
    if trace.truncated {
        return Some(Classification::excluded("truncated", Some(hash)));
    }
    if trace.outcome != Outcome::Ok {
        return Some(Classification::excluded(trace.outcome.as_str(), Some(hash)));
    }
    if guard_failed(trace) {
        return Some(Classification::excluded("guard_failed", Some(hash)));
    }
    if let Some(schema) = trace
        .raw_prompt
        .as_ref()
        .and_then(guard::request_schema)
    {
        if !guard::content_matches_schema(&content, &schema) {
            return Some(Classification::excluded("schema_invalid", Some(hash)));
        }
    }
    Some(Classification {
        state: "qualified",
        reason: None,
        completion_hash: Some(hash),
    })
}

fn guard_failed(trace: &TraceMessage) -> bool {
    trace
        .guard_event
        .as_ref()
        .and_then(|event| event["outcome"].as_str())
        == Some("fallback_failed")
}

/// True when the completion's assistant content is JSON that satisfies `schema`.
pub fn content_matches_schema(content: &str, schema: &Value) -> bool {
    guard::content_matches_schema(content, schema)
}

#[cfg(test)]
mod tests;
