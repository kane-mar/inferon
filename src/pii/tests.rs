use super::*;

fn counts(pairs: &[(&str, i64)]) -> Counts {
    let mut counts = Counts::default();
    for (class, count) in pairs {
        *counts.by_class.entry((*class).to_string()).or_insert(0) += count;
    }
    counts
}

#[test]
fn an_email_address_is_detected() {
    assert_eq!(scan("write to jane.doe+tag@example.co.uk today"), counts(&[("email", 1)]));
    assert_eq!(scan("jane@example"), counts(&[]), "no TLD");
    assert_eq!(scan("jane@@example.com").total(), 0, "not an email");
}

#[test]
fn a_card_number_is_detected_only_when_luhn_valid() {
    // 4242 4242 4242 4242 is the canonical test card (Luhn-valid).
    assert_eq!(scan("card 4242 4242 4242 4242 exp 12/30"), counts(&[("card", 1)]));
    assert_eq!(scan("order 4242-4242-4242-4242"), counts(&[("card", 1)]));
    assert_eq!(scan("card 4242 4242 4242 4241"), counts(&[]), "fails Luhn");
}

#[test]
fn luhn_is_the_standard_checksum() {
    assert!(luhn("4242424242424242"));
    assert!(luhn("4111111111111111"));
    assert!(!luhn("4242424242424241"));
    assert!(!luhn(""));
    assert!(!luhn("4242-4242"), "non-digits are rejected, not ignored");
}

#[test]
fn an_ssn_shape_is_detected_and_impossible_areas_are_not() {
    assert_eq!(scan("ssn 123-45-6789"), counts(&[("ssn", 1)]));
    assert_eq!(scan("ssn 000-45-6789").total(), 0, "area 000");
    assert_eq!(scan("ssn 666-45-6789").total(), 0, "area 666");
    assert_eq!(scan("ref 123-45-67890").total(), 0, "part of a longer number");
}

#[test]
fn api_key_shapes_are_detected() {
    assert_eq!(
        scan("key sk-abcdefghijklmnopqrstuvwxyz012345"),
        counts(&[("api_key", 1)])
    );
    assert_eq!(scan("aws AKIAIOSFODNN7EXAMPLE"), counts(&[("api_key", 1)]));
    assert_eq!(scan("gh ghp_abcdefghijklmnopqrstuvwxyz"), counts(&[("api_key", 1)]));
    assert_eq!(scan("just sk-short").total(), 0, "too short to be a key");
}

#[test]
fn an_international_phone_number_is_detected() {
    assert_eq!(scan("call +1 415 555 0132"), counts(&[("phone", 1)]));
    assert_eq!(scan("call 415-555-0132"), counts(&[("phone", 1)]));
    assert_eq!(scan("id 4155550132").total(), 0, "bare digits are usually an id");
}

#[test]
fn normal_prose_has_no_findings() {
    for text in [
        "extract the invoice total from the document",
        "summarise this in three bullets",
        "the answer is 42",
        "version 1.2.3 shipped on 2026-10-31",
        "{\"temperature\": 0.2, \"max_tokens\": 4096}",
    ] {
        assert_eq!(scan(text).total(), 0, "{text}");
    }
}

#[test]
fn overlapping_shapes_count_once_and_the_longest_wins() {
    // A card number with spaces also looks like a phone in parts; only the card
    // is reported.
    let found = scan("pay 4242 4242 4242 4242 now");
    assert_eq!(found, counts(&[("card", 1)]));
}

#[test]
fn redaction_replaces_each_match_with_its_class_placeholder() {
    let (text, found) = redact("mail jane@example.com or call +1 415 555 0132");
    assert_eq!(text, "mail <email> or call <phone>");
    assert_eq!(found, counts(&[("email", 1), ("phone", 1)]));
}

#[test]
fn redaction_is_idempotent() {
    let source = "jane@example.com 4242 4242 4242 4242 123-45-6789";
    let (once, first) = redact(source);
    let (twice, second) = redact(&once);
    assert_eq!(twice, once);
    assert_eq!(second.total(), 0, "the placeholders are not PII shapes");
    assert_eq!(first.total(), 3);
}

#[test]
fn the_matched_value_is_never_in_the_counts() {
    // The record is what gets stored; it must not be able to leak the value.
    let (_, found) = redact("jane@example.com 4242 4242 4242 4242");
    let record = trace_record(&found, "redacted", "off");
    let serialized = record.to_string();
    assert!(!serialized.contains("jane"), "{serialized}");
    assert!(!serialized.contains("4242"), "{serialized}");
    assert_eq!(record["classes"]["email"], 1);
    assert_eq!(record["total"], 2);
    assert_eq!(record["storage"], "redacted");
}

#[test]
fn a_json_payload_is_scanned_and_redacted_in_place() {
    let payload = serde_json::json!({
        "messages": [
            {"role": "system", "content": "contact jane@example.com"},
            {"role": "user", "content": "my card is 4242 4242 4242 4242"}
        ],
        "metadata": {"owner": "bob@example.org"}
    });
    let found = scan_value(&payload);
    assert_eq!(found, counts(&[("email", 2), ("card", 1)]));

    let (redacted, counts) = redact_value(&payload);
    assert_eq!(counts, found);
    let serialized = redacted.to_string();
    assert!(!serialized.contains("jane"));
    assert!(!serialized.contains("4242 4242"));
    assert!(serialized.contains("<email>"));
    assert_eq!(scan_value(&redacted).total(), 0, "nothing left to find");
}

#[test]
fn a_large_payload_stays_linear() {
    // Bounded-cost guard: a 200 KB payload must not take a noticeable slice of
    // the request's budget (measured locally at ~1–2 ms in debug).
    let text = "ordinary words with no shapes at all ".repeat(5_000);
    let started = std::time::Instant::now();
    let found = scan(&text);
    let elapsed = started.elapsed();
    assert_eq!(found.total(), 0);
    eprintln!("scan 200 KB: {elapsed:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "scanning 200 KB took {elapsed:?} — that is not a linear pass"
    );
}
