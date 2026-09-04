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

const DOWNGRADE_THRESHOLD: f64 = 0.35;

const CODE_MARKERS: [&str; 7] = ["```", "def ", "class ", "fn ", "import ", "SELECT ", "FROM "];
const JSON_SPEC_MARKERS: [&str; 2] = ["\"type\": \"object\"", "\"properties\""];

/// gpt-4o tier but NOT gpt-4o-mini (a mini request can't be downgraded).
pub fn is_gpt4o_tier(model: &str) -> bool {
    model.starts_with("gpt-4o") && !model.starts_with("gpt-4o-mini")
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
pub fn complexity_score(payload: &Value) -> f64 {
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

/// Downgrade a gpt-4o-tier request to gpt-4o-mini when it is simple enough
/// (complexity < 0.35). Returns the model to use; `None` = keep the client's.
pub fn maybe_downgrade(payload: &Value) -> Option<String> {
    let model = payload["model"].as_str()?;
    if is_gpt4o_tier(model) && complexity_score(payload) < DOWNGRADE_THRESHOLD {
        Some("gpt-4o-mini".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(model: &str, body: Value) -> Value {
        let mut p = serde_json::json!({"model": model, "messages": [{"role": "user", "content": "hi"}]});
        if let Some(obj) = body.as_object() {
            for (k, v) in obj {
                p[k] = v.clone();
            }
        }
        p
    }

    #[test]
    fn plain_short_chat_is_simple() {
        let p = payload("gpt-4o", serde_json::json!({}));
        assert!(complexity_score(&p) < DOWNGRADE_THRESHOLD);
        assert_eq!(maybe_downgrade(&p).as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn tools_make_it_complex() {
        let p = payload(
            "gpt-4o",
            serde_json::json!({"tools": [{"type": "function", "function": {"name": "f"}}]}),
        );
        assert_eq!(complexity_score(&p), 0.50);
        assert_eq!(maybe_downgrade(&p), None);
    }

    #[test]
    fn json_mode_alone_scores_below_threshold() {
        // Spec math: response_format adds +0.30, still < 0.35 -> downgraded.
        let p = payload(
            "gpt-4o",
            serde_json::json!({"response_format": {"type": "json_object"}}),
        );
        assert_eq!(complexity_score(&p), 0.30);
        assert_eq!(maybe_downgrade(&p).as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn json_mode_plus_long_content_exceeds_threshold() {
        let p = payload(
            "gpt-4o",
            serde_json::json!({"response_format": {"type": "json_object"}}),
        );
        let mut p = p;
        p["messages"] = serde_json::json!([{"role": "user", "content": "x".repeat(4000)}]);
        assert_eq!(complexity_score(&p), 0.50);
        assert_eq!(maybe_downgrade(&p), None);
    }

    #[test]
    fn long_content_scores_below_threshold() {
        // 4000 chars adds +0.20 only -> still simple by spec math.
        let mut p = payload("gpt-4o", serde_json::json!({}));
        p["messages"] = serde_json::json!([{"role": "user", "content": "x".repeat(4000)}]);
        assert_eq!(complexity_score(&p), 0.20);
        assert_eq!(maybe_downgrade(&p).as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn code_or_json_spec_plus_length_exceeds_threshold() {
        // Code markers +0.30 alone stays below; combined with >3000 chars it
        // clears the bar (0.30 + 0.20 = 0.50).
        let mut code = payload("gpt-4o", serde_json::json!({}));
        code["messages"] = serde_json::json!([{"role": "user", "content": "fn main() {\n  import x\n}"}]);
        assert_eq!(complexity_score(&code), 0.30);
        code["messages"] = serde_json::json!([{"role": "user", "content": "fn main() {\n  import x\n}".repeat(200)}]);
        assert_eq!(complexity_score(&code), 0.50);
        assert_eq!(maybe_downgrade(&code), None);
    }

    #[test]
    fn code_and_json_specs_score_higher() {
        let code = payload("gpt-4o", serde_json::json!({}));
        let mut code = code;
        code["messages"] =
            serde_json::json!([{"role": "user", "content": "fn main() {\n  import x\n}"}]);
        assert_eq!(complexity_score(&code), 0.30);

        let spec = payload("gpt-4o", serde_json::json!({}));
        let mut spec = spec;
        spec["messages"] =
            serde_json::json!([{"role": "user", "content": "{\"type\": \"object\", \"properties\": {}}"}]);
        assert_eq!(complexity_score(&spec), 0.25);
    }

    #[test]
    fn score_is_capped_at_one() {
        let mut p = payload("gpt-4o", serde_json::json!({}));
        p["tools"] = serde_json::json!([{"type": "function"}]);
        p["response_format"] = serde_json::json!({"type": "json_object"});
        p["messages"] = serde_json::json!([{"role": "user", "content": "y".repeat(9000)}]);
        assert_eq!(complexity_score(&p), 1.0);
    }

    #[test]
    fn never_downgrades_mini_or_other_models() {
        assert_eq!(maybe_downgrade(&payload("gpt-4o-mini", serde_json::json!({}))), None);
        assert_eq!(maybe_downgrade(&payload("deepseek-chat", serde_json::json!({}))), None);
    }
}
