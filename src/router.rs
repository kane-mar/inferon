//! Heuristic model router (Slice 2, ITEM-015).
//!
//! Scores a request payload for complexity (0.0 dead-simple .. 1.0 frontier):
//! tools/functions +0.50, response_format json +0.30, content >8000 chars
//! +0.40 / >3000 +0.20, code patterns +0.30, JSON-spec patterns +0.25,
//! capped at 1.0. Requests below the downgrade threshold on a gpt-4o-tier
//! model are rewritten to gpt-4o-mini before forwarding. Classification is
//! pure Rust over the already-parsed payload (no hot-path allocations beyond
//! the rare rewrite).

use serde_json::Value;

pub(crate) const DOWNGRADE_THRESHOLD: f64 = 0.35;

const CODE_MARKERS: [&str; 7] = ["```", "def ", "class ", "fn ", "import ", "SELECT ", "FROM "];
const JSON_SPEC_MARKERS: [&str; 2] = ["\"type\": \"object\"", "\"properties\""];

/// Expensive → cheap model pairs the heuristic router can downshift. Ordered:
/// the first pair whose expensive member matches (and whose cheap member does
/// not) wins. DeepSeek-native traffic downshifts `deepseek-v4-pro` →
/// `deepseek-flash` (ITEM-054).
pub const TIER_PAIRS: &[(&str, &str)] = &[
    ("gpt-4o", "gpt-4o-mini"),
    ("deepseek-v4-pro", "deepseek-flash"),
];

/// A per-tenant downshift rule (ITEM-081): `frontier` may be served by `cheap`
/// for simple traffic. Tenant pairs win over the built-in [`TIER_PAIRS`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct TierPair {
    pub frontier: String,
    pub cheap: String,
}

impl TierPair {
    fn matches(&self, model: &str) -> bool {
        model.starts_with(&self.frontier) && !model.starts_with(&self.cheap)
    }
}

/// Where a downgrade target came from — recorded on the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairSource {
    Tenant,
    Global,
}

impl PairSource {
    pub fn label(self) -> &'static str {
        match self {
            PairSource::Tenant => "tenant",
            PairSource::Global => "global",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downgrade {
    pub model: String,
    pub source: PairSource,
}

/// Why a pair set was rejected. The dashboard maps each to a typed 400.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairIssue {
    EmptyModel,
    SameModel,
    UnknownModel(String),
    Duplicate,
    Cycle,
}

/// Reject a pair set the router could not honestly apply. `knows` answers
/// whether a model is routable for the tenant right now.
pub fn validate_pairs(
    pairs: &[TierPair],
    knows: impl Fn(&str) -> bool,
) -> Result<(), PairIssue> {
    for (index, pair) in pairs.iter().enumerate() {
        if pair.frontier.is_empty() || pair.cheap.is_empty() {
            return Err(PairIssue::EmptyModel);
        }
        if pair.frontier == pair.cheap {
            return Err(PairIssue::SameModel);
        }
        if pairs[..index]
            .iter()
            .any(|earlier| earlier.frontier == pair.frontier)
        {
            return Err(PairIssue::Duplicate);
        }
        for model in [&pair.frontier, &pair.cheap] {
            if !knows(model) {
                return Err(PairIssue::UnknownModel(model.clone()));
            }
        }
    }
    if closes_loop(pairs) {
        return Err(PairIssue::Cycle);
    }
    Ok(())
}

/// A loop can only be created with the built-in table, so check the effective
/// graph (tenant pairs shadowing a frontier, plus the remaining defaults).
fn closes_loop(pairs: &[TierPair]) -> bool {
    let mut edges: Vec<(String, String)> = pairs
        .iter()
        .map(|pair| (pair.frontier.clone(), pair.cheap.clone()))
        .collect();
    for &(frontier, cheap) in TIER_PAIRS {
        if !pairs.iter().any(|pair| pair.frontier == frontier) {
            edges.push((frontier.to_string(), cheap.to_string()));
        }
    }
    let mut settled = std::collections::HashSet::new();
    edges.iter().map(|(frontier, _)| frontier).any(|frontier| {
        reaches_itself(
            frontier,
            &edges,
            &mut std::collections::HashSet::new(),
            &mut settled,
        )
    })
}

fn reaches_itself(
    node: &str,
    edges: &[(String, String)],
    path: &mut std::collections::HashSet<String>,
    settled: &mut std::collections::HashSet<String>,
) -> bool {
    if !path.insert(node.to_string()) {
        return true;
    }
    let looped = edges
        .iter()
        .filter(|(from, _)| from == node)
        .any(|(_, to)| {
            path.contains(to)
                || (!settled.contains(to) && reaches_itself(to, edges, path, settled))
        });
    path.remove(node);
    settled.insert(node.to_string());
    looped
}

/// The cheap model to downshift `model` to, if it is the expensive member of a
/// configured tier pair. `None` for the cheap member itself or an unpaired model.
pub(crate) fn downgrade_target(model: &str) -> Option<&'static str> {
    TIER_PAIRS.iter().find_map(|&(expensive, cheap)| {
        (model.starts_with(expensive) && !model.starts_with(cheap)).then_some(cheap)
    })
}

/// The downgrade for `model`: the tenant's own pair first, then the built-in
/// table, so a tenant that attaches its own cheap model is never overridden by
/// the defaults.
pub fn downgrade_for(model: &str, pairs: &[TierPair]) -> Option<Downgrade> {
    pairs
        .iter()
        .find(|pair| pair.matches(model))
        .map(|pair| Downgrade {
            model: pair.cheap.clone(),
            source: PairSource::Tenant,
        })
        .or_else(|| {
            downgrade_target(model).map(|cheap| Downgrade {
                model: cheap.to_string(),
                source: PairSource::Global,
            })
        })
}

/// Total character length of all string message content (non-string content
/// counts as 0 — it is handled by the tool/structure signals instead).
fn content_chars(payload: &Value) -> usize {
    payload["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["content"].as_str())
        .map(str::len)
        .sum()
}

/// Simple unstructured text (rule C of the linter uses a stricter variant).
fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// 0.0 (dead simple) .. 1.0 (frontier reasoning). Pure function of the payload.
pub(crate) fn complexity_score(payload: &Value) -> f64 {
    let mut score: f64 = 0.0;
    for key in ["tools", "functions"] {
        if payload[key].as_array().is_some_and(|a| !a.is_empty()) {
            score += 0.50;
        }
    }
    if payload["response_format"].is_object() {
        score += 0.30;
    }
    let chars = content_chars(payload);
    if chars > 8000 {
        score += 0.40;
    } else if chars > 3000 {
        score += 0.20;
    }

    let mut text = String::with_capacity(chars);
    if let Some(messages) = payload["messages"].as_array() {
        for message in messages {
            if let Some(content) = message["content"].as_str() {
                text.push_str(content);
            }
        }
    }
    if contains_any(&text, &CODE_MARKERS) {
        score += 0.30;
    }
    if contains_any(&text, &JSON_SPEC_MARKERS) {
        score += 0.25;
    }
    score.min(1.0)
}

/// Downgrade a request on the expensive member of a configured tier pair to its
/// cheap model when it is simpler than `threshold`. Returns the model to use;
/// `None` = keep the client's.
pub fn maybe_downgrade_with(payload: &Value, threshold: f64) -> Option<String> {
    maybe_downgrade_with_pairs(payload, threshold, &[]).map(|chosen| chosen.model)
}

/// As [`maybe_downgrade_with`], but honouring the tenant's pairs first and
/// reporting where the target came from.
pub fn maybe_downgrade_with_pairs(
    payload: &Value,
    threshold: f64,
    pairs: &[TierPair],
) -> Option<Downgrade> {
    let model = payload["model"].as_str()?;
    let chosen = downgrade_for(model, pairs)?;
    (complexity_score(payload) < threshold).then_some(chosen)
}

/// Downgrade with the default threshold (ITEM-015 behaviour).
pub fn maybe_downgrade(payload: &Value) -> Option<String> {
    maybe_downgrade_with(payload, DOWNGRADE_THRESHOLD)
}

#[cfg(test)]
mod tests;
