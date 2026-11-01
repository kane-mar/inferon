//! Pre-flight payload lint (ITEM-090).
//!
//! Before the call leaves the perimeter, look for the payload waste the
//! post-hoc linter can only describe afterwards: a system prompt repeated turn
//! after turn, an oversized tool schema, a boilerplate block, duplicate
//! messages. Advisory only — the payload is forwarded unchanged (rewriting is
//! ITEM-091); the finding is traced and sent back on `X-Inferon-Lint`.

use serde_json::{json, Value};

/// Tool/function definitions beyond this many bytes are flagged.
pub const SCHEMA_BUDGET_BYTES: usize = 8_000;
/// A system prompt beyond this many characters is flagged as boilerplate.
pub const PROMPT_BUDGET_CHARS: usize = 8_000;
/// How long a system prompt counts as "the same one again".
pub const REPEAT_WINDOW: std::time::Duration = std::time::Duration::from_secs(600);

/// Version of the rule set. Stored with every scan so counts from different rule
/// sets are never compared as if they were the same measurement.
pub const VERSION: &str = "preflight/1";

/// Inspect a parsed request and record **every** scan.
///
/// `repeated_system_prompt` comes from the hot-path history (the module is pure,
/// so the state lives with the caller). The wasted amount is priced with the
/// same engine as the trace, at the request's model.
///
/// A clean payload returns a record with an empty `kinds` list, not `None`: a
/// NULL used to mean both "clean" and "never scanned", which made coverage
/// unmeasurable. `kinds` empty == scanned, nothing found.
pub fn inspect(payload: &Value, repeated_system_prompt: bool, model: &str) -> Value {
    let mut kinds: Vec<&'static str> = Vec::new();
    let mut wasted_tokens = 0i64;

    let system = system_chars(payload);
    if repeated_system_prompt && system > 0 {
        kinds.push("repeated_system_prompt");
        wasted_tokens += (system / 4) as i64;
    }
    let schema_bytes = schema_bytes(payload);
    if schema_bytes > SCHEMA_BUDGET_BYTES {
        kinds.push("oversized_schema");
        wasted_tokens += ((schema_bytes - SCHEMA_BUDGET_BYTES) / 4) as i64;
    }
    if system > PROMPT_BUDGET_CHARS {
        kinds.push("boilerplate");
        wasted_tokens += ((system - PROMPT_BUDGET_CHARS) / 4) as i64;
    }
    let duplicate_chars = duplicate_message_chars(payload);
    if duplicate_chars > 0 {
        kinds.push("duplicate_turns");
        wasted_tokens += (duplicate_chars / 4) as i64;
    }

    // Input-only: the finding is about the payload the model is asked to read.
    let wasted_usd = crate::cost::calculate_cost(
        model,
        crate::cost::tokens_as_i32(wasted_tokens),
        0,
        0,
    );
    // Money is a string, as everywhere else: no float ever touches a cost.
    json!({
        "version": VERSION,
        "kinds": kinds,
        "wasted_tokens_est": wasted_tokens,
        "wasted_usd_est": wasted_usd.round_dp(10).to_string(),
    })
}

/// Characters in the first `system` message.
fn system_chars(payload: &Value) -> usize {
    payload["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"].as_str() == Some("system"))
        .filter_map(|message| message["content"].as_str())
        .map(str::len)
        .sum()
}

/// Bytes of the tool/function definitions.
fn schema_bytes(payload: &Value) -> usize {
    ["tools", "functions"]
        .iter()
        .filter_map(|key| serde_json::to_string(&payload[*key]).ok())
        .map(|value| value.len())
        .sum()
}

/// Characters in messages that exactly repeat an earlier (role, content) pair.
fn duplicate_message_chars(payload: &Value) -> usize {
    let Some(messages) = payload["messages"].as_array() else {
        return 0;
    };
    let mut seen = std::collections::HashSet::new();
    let mut chars = 0usize;
    for message in messages {
        let key = (
            message["role"].as_str().unwrap_or(""),
            message["content"].as_str().unwrap_or(""),
        );
        if key.1.is_empty() {
            continue;
        }
        if !seen.insert(key) {
            chars += key.1.len();
        }
    }
    chars
}

#[cfg(test)]
mod tests;
