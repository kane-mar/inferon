// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! PII detection and redaction (ITEM-096).
//!
//! Deterministic patterns, no service and no model: the proxy must be able to
//! answer "does this payload contain an email address?" in microseconds, on
//! every request, with the same answer twice.
//!
//! What this is **not**: a compliance guarantee. It catches shapes, not meaning —
//! a name, an address or a free-form identifier is invisible to it, and a
//! determined obfuscation ("j o h n at example dot com") defeats it. The classes
//! and their limits are documented in `.backlog/ITEMS/ITEM-096-…`; treat a clean
//! scan as "no shapes found", never as "no PII".
//!
//! A matched value is **never** recorded: only the class and a count, so the
//! detector's own output cannot leak what it found.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The shapes we recognise. New variants are additive and versioned by the
/// `pii_version` stored with the counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Email,
    Phone,
    Card,
    Ssn,
    ApiKey,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Card => "card",
            Self::Ssn => "ssn",
            Self::ApiKey => "api_key",
        }
    }

    /// The placeholder written into a redacted payload. Chosen so that a second
    /// pass finds nothing here (redaction is idempotent).
    fn placeholder(self) -> &'static str {
        match self {
            Self::Email => "<email>",
            Self::Phone => "<phone>",
            Self::Card => "<card>",
            Self::Ssn => "<ssn>",
            Self::ApiKey => "<api_key>",
        }
    }
}

/// What was found, by class. Never the values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    #[serde(flatten)]
    pub by_class: std::collections::BTreeMap<String, i64>,
}

impl Counts {
    pub fn add(&mut self, class: Class, count: i64) {
        if count > 0 {
            *self.by_class.entry(class.as_str().to_string()).or_insert(0) += count;
        }
    }

    pub fn merge(&mut self, other: &Counts) {
        for (class, count) in &other.by_class {
            *self.by_class.entry(class.clone()).or_insert(0) += count;
        }
    }

    pub fn total(&self) -> i64 {
        self.by_class.values().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_class.is_empty()
    }
}

/// A match: byte range plus class.
#[derive(Debug)]
struct Span {
    start: usize,
    end: usize,
    class: Class,
}

/// Scan a single string. Counts only — the matched text never leaves this
/// function.
pub fn scan(text: &str) -> Counts {
    let mut counts = Counts::default();
    for span in resolve(spans(text)) {
        counts.add(span.class, 1);
    }
    counts
}

/// Replace every match with its class placeholder, returning the text and what
/// was replaced. Idempotent: a second pass over the output finds nothing that
/// the first pass wrote.
pub fn redact(text: &str) -> (String, Counts) {
    let spans = resolve(spans(text));
    if spans.is_empty() {
        return (text.to_string(), Counts::default());
    }
    let mut counts = Counts::default();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for span in &spans {
        out.push_str(&text[cursor..span.start]);
        out.push_str(span.class.placeholder());
        counts.add(span.class, 1);
        cursor = span.end;
    }
    out.push_str(&text[cursor..]);
    (out, counts)
}

/// Scan every string in a JSON value (keys included: a key is data too).
pub fn scan_value(value: &Value) -> Counts {
    let mut counts = Counts::default();
    walk(value, &mut |text| counts.merge(&scan(text)));
    counts
}

/// Redact every string in a JSON value.
pub fn redact_value(value: &Value) -> (Value, Counts) {
    let mut counts = Counts::default();
    let redacted = map_strings(value, &mut |text| {
        let (text, found) = redact(text);
        counts.merge(&found);
        text
    });
    (redacted, counts)
}

/// The trace record (`traces.pii`): counts by class plus what the run did.
pub fn trace_record(counts: &Counts, storage: &str, wire: &str) -> Value {
    json!({
        "version": VERSION,
        "classes": counts.by_class,
        "total": counts.total(),
        "storage": storage,
        "wire": wire,
    })
}

/// Share of requests whose wire payload is forwarded unredacted, so a tenant can
/// see what redaction costs before trusting it everywhere (same discipline as
/// ITEM-091's pruning holdout).
pub const WIRE_HOLDOUT: u8 = 10;

/// Bump when a detector changes shape; stored with every count so old and new
/// scans are never compared as if they were the same measurement.
pub const VERSION: &str = "patterns/1";

// --- detectors --------------------------------------------------------------

/// All candidate spans, before overlap resolution.
fn spans(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // API keys are the longest shapes and rarely overlap with the rest.
        if let Some(end) = api_key_at(text, i) {
            spans.push(Span { start: i, end, class: Class::ApiKey });
            i = end;
            continue;
        }
        if let Some(end) = card_at(text, i) {
            spans.push(Span { start: i, end, class: Class::Card });
            i = end;
            continue;
        }
        if let Some(end) = ssn_at(text, i) {
            spans.push(Span { start: i, end, class: Class::Ssn });
            i = end;
            continue;
        }
        if let Some(end) = email_at(text, i) {
            spans.push(Span { start: i, end, class: Class::Email });
            i = end;
            continue;
        }
        if let Some(end) = phone_at(text, i) {
            spans.push(Span { start: i, end, class: Class::Phone });
            i = end;
            continue;
        }
        // Advance by whole characters: the detectors slice at `i`, so stepping
        // into a multibyte character would panic on any em dash in the payload.
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    spans
}

/// Drop overlapping spans: earliest start wins, then the longer match. Keeps one
/// finding per piece of text, so a card number is never also a phone.
fn resolve(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut kept: Vec<Span> = Vec::new();
    for span in spans {
        if kept.last().is_none_or(|last| span.start >= last.end) {
            kept.push(span);
        }
    }
    kept
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn api_key_at(text: &str, start: usize) -> Option<usize> {
    let rest = &text[start..];
    for prefix in ["sk-", "AKIA", "ghp_", "gho_", "github_pat_"] {
        if let Some(after) = rest.strip_prefix(prefix) {
            let body: usize = after
                .bytes()
                .take_while(|b| is_word(*b) || *b == b'-')
                .count();
            // Real keys are long; a short "sk-" is a word fragment.
            if body >= 16 && start > 0 && !is_word(text.as_bytes()[start - 1]) {
                return Some(start + prefix.len() + body);
            }
        }
    }
    None
}

/// Card numbers: 13–19 digits, optionally grouped by single spaces or dashes,
/// and **Luhn-valid** — the only cheap way to keep this from matching every
/// long number in a log.
fn card_at(text: &str, start: usize) -> Option<usize> {
    if start > 0 && is_word(text.as_bytes()[start - 1]) {
        return None;
    }
    let mut digits = String::new();
    let mut end = start;
    let bytes = text.as_bytes();
    for (offset, byte) in bytes[start..].iter().enumerate() {
        match byte {
            b'0'..=b'9' => {
                digits.push(*byte as char);
                end = start + offset + 1;
            }
            b' ' | b'-' if !digits.is_empty() => {
                // Stop at a valid card before bridging into unrelated digits
                // ("…4242 4242 4242 4242 123-45-6789" is a card, not a 25-digit
                // number).
                if (13..=19).contains(&digits.len()) && luhn(&digits) {
                    break;
                }
                // Otherwise a separator is part of the span only if a digit follows.
                if bytes
                    .get(start + offset + 1)
                    .is_some_and(u8::is_ascii_digit)
                {
                    end = start + offset + 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
        if digits.len() > 19 {
            break;
        }
    }
    if !(13..=19).contains(&digits.len()) || !luhn(&digits) {
        return None;
    }
    // A bare digit run that is part of a longer token is not a card.
    Some(end)
}

/// The Luhn checksum, the standard card-number check.
pub fn luhn(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for byte in digits.bytes().rev() {
        let Some(digit) = (byte as char).to_digit(10) else {
            return false;
        };
        let value = if double {
            let doubled = digit * 2;
            if doubled > 9 { doubled - 9 } else { doubled }
        } else {
            digit
        };
        sum += value;
        double = !double;
    }
    sum.is_multiple_of(10) && !digits.is_empty()
}

/// US SSN shape: 3-2-4 digits with dashes, area not 000/666/9xx.
fn ssn_at(text: &str, start: usize) -> Option<usize> {
    if start > 0 && is_word(text.as_bytes()[start - 1]) {
        return None;
    }
    let bytes = text.as_bytes();
    let at = |index: usize, expected: u8| bytes.get(start + index) == Some(&expected);
    let digits = |index: usize| bytes.get(start + index).is_some_and(u8::is_ascii_digit);
    // 3-2-4: digits everywhere except the two dashes at 3 and 6.
    for index in [0, 1, 2, 4, 5, 7, 8, 9, 10] {
        if !digits(index) {
            return None;
        }
    }
    if !at(3, b'-') || !at(6, b'-') {
        return None;
    }
    let area: u32 = text[start..start + 3].parse().ok()?;
    if area == 0 || area == 666 || area >= 900 {
        return None;
    }
    // Not part of a longer number (the shape ends at start + 11).
    if bytes.get(start + 11).is_some_and(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(start + 11)
}

/// Email: local@domain.tld, both sides plain (no quoting or IP literals).
fn email_at(text: &str, start: usize) -> Option<usize> {
    if start > 0 && is_word(text.as_bytes()[start - 1]) {
        return None;
    }
    let bytes = text.as_bytes();
    let local_len = bytes[start..]
        .iter()
        .take_while(|b| {
            b.is_ascii_alphanumeric() || matches!(**b, b'.' | b'_' | b'%' | b'+' | b'-')
        })
        .count();
    if local_len == 0 || local_len > 64 || bytes.get(start + local_len) != Some(&b'@') {
        return None;
    }
    let mut index = start + local_len + 1;
    let mut labels = 0;
    loop {
        let label_len = bytes[index..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
            .count();
        if label_len == 0 {
            return None;
        }
        labels += 1;
        index += label_len;
        match bytes.get(index) {
            Some(b'.') => {
                index += 1;
                // The final label is the TLD: letters only, 2+.
                let tld_len = bytes[index..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphabetic())
                    .count();
                if tld_len >= 2 && bytes.get(index + tld_len).is_none_or(|b| !is_word(*b)) {
                    return Some(index + tld_len);
                }
            }
            _ => return None,
        }
        if labels > 8 {
            return None;
        }
    }
}

/// Phone: an international `+` number, or a grouped national number. Deliberately
/// narrow — a bare 10-digit run is far more often an id than a phone.
fn phone_at(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start > 0 && is_word(bytes[start - 1]) {
        return None;
    }
    if bytes[start] == b'+' {
        let mut index = start + 1;
        let mut digits = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte.is_ascii_digit() {
                digits += 1;
                index += 1;
            } else if matches!(byte, b' ' | b'-' | b'(' | b')')
                && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
            {
                index += 1;
            } else {
                break;
            }
        }
        if (8..=15).contains(&digits) {
            return Some(index);
        }
        return None;
    }
    // National grouping: 3-3-4 or (3) 3-4 with separators, never bare digits.
    let grouped = |pattern: [usize; 3]| -> bool {
        let mut index = start;
        for (part, len) in pattern.iter().enumerate() {
            for _ in 0..*len {
                if !bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    return false;
                }
                index += 1;
            }
            if part < 2 {
                if !bytes
                    .get(index)
                    .is_some_and(|b| matches!(*b, b'-' | b'.' | b' '))
                {
                    return false;
                }
                index += 1;
            }
        }
        true
    };
    if grouped([3, 3, 4]) || grouped([3, 4, 4]) {
        let mut end = start + 12;
        if bytes.get(start + 3).is_some_and(|b| *b == b' ') {
            end = start + 12;
        }
        return Some(end);
    }
    None
}

// --- JSON walking -----------------------------------------------------------

fn walk(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::String(text) => visit(text),
        Value::Array(items) => items.iter().for_each(|item| walk(item, visit)),
        Value::Object(map) => {
            for (key, item) in map {
                visit(key);
                walk(item, visit);
            }
        }
        _ => {}
    }
}

fn map_strings(value: &Value, visit: &mut impl FnMut(&str) -> String) -> Value {
    match value {
        Value::String(text) => Value::String(visit(text)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| map_strings(item, visit)).collect())
        }
        Value::Object(map) => {
            let mut redacted = Map::new();
            for (key, item) in map {
                redacted.insert(visit(key), map_strings(item, visit));
            }
            Value::Object(redacted)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests;
