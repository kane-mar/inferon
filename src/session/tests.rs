    use super::*;

    const SECRET: &str = "test-session-secret";
    const UID: &str = "11111111-1111-1111-1111-111111111111";

    fn uid() -> Uuid {
        Uuid::parse_str(UID).unwrap()
    }

    fn token() -> String {
        issue_session(uid(), SECRET, 1_000, 60)
    }

    #[test]
    fn session_token_round_trips_the_user_id_until_expiry() {
        let token = token();
        assert_eq!(verify_session(&token, SECRET, 1_000), Some(uid()));
        // One second before expiry is still valid; expiry is exclusive.
        assert_eq!(verify_session(&token, SECRET, 1_059), Some(uid()));
    }

    #[test]
    fn session_token_is_rejected_at_and_after_expiry() {
        let token = token();
        assert_eq!(verify_session(&token, SECRET, 1_060), None, "now == expiry");
        assert_eq!(verify_session(&token, SECRET, 2_000), None, "long past expiry");
    }

    #[test]
    fn tampered_forged_or_wrong_secret_tokens_verify_to_none() {
        let token = token();
        let (uuid, rest) = token.split_once('.').unwrap();
        let (exp, mac) = rest.split_once('.').unwrap();

        // Tampered user id (valid hex, so it parses — the mac must catch it).
        let other = if uuid.ends_with('0') { '1' } else { '0' };
        let tampered_id = format!("{}{}", &uuid[..uuid.len() - 1], other);
        let forged = format!("{tampered_id}.{exp}.{mac}");
        assert_eq!(verify_session(&forged, SECRET, 1_000), None, "tampered user id");

        // Tampered expiry.
        let forged_exp = format!("{exp:0<16}", exp = "0");
        let forged = format!("{uuid}.{forged_exp}.{mac}");
        assert_eq!(verify_session(&forged, SECRET, 1_000), None, "tampered expiry");

        // Tampered signature.
        let bad_mac = if mac.ends_with('0') { '1' } else { '0' };
        let forged = format!("{uuid}.{exp}.{}{}", &mac[..mac.len() - 1], bad_mac);
        assert_eq!(verify_session(&forged, SECRET, 1_000), None, "tampered mac");

        // Same token under a different secret.
        assert_eq!(verify_session(&token, "other-secret", 1_000), None, "wrong secret");

        // Garbage never errors — it just means signed out.
        assert_eq!(verify_session("", SECRET, 1_000), None);
        assert_eq!(verify_session("not-a-token", SECRET, 1_000), None);
        assert_eq!(verify_session("a.b.c.d.e", SECRET, 1_000), None);
        assert_eq!(verify_session(&token.replace('1', "z"), SECRET, 1_000), None);
    }

    #[test]
    fn cookie_value_reads_the_named_cookie() {
        assert_eq!(cookie_value("a=1; inferon_session=abc; b=2", SESSION_COOKIE), Some("abc"));
        assert_eq!(cookie_value("inferon_session=", SESSION_COOKIE), Some(""));
        assert_eq!(cookie_value("other=1", SESSION_COOKIE), None);
        assert_eq!(cookie_value("", SESSION_COOKIE), None);
        assert_eq!(cookie_value("inferon_session=abc", "missing"), None);
    }

    #[test]
    fn set_cookie_carries_the_safe_attributes() {
        let insecure = set_cookie("abc", false);
        assert!(insecure.contains("inferon_session=abc"), "value: {insecure}");
        for attr in ["HttpOnly", "SameSite=Lax", "Path=/", &format!("Max-Age={SESSION_TTL_SECS}")] {
            assert!(insecure.contains(attr), "missing {attr}: {insecure}");
        }
        assert!(!insecure.contains("Secure"), "no Secure over plain http: {insecure}");

        let secure = set_cookie("abc", true);
        assert!(secure.contains("Secure"), "Secure over TLS: {secure}");
    }

    #[test]
    fn clear_cookie_expires_the_session_immediately() {
        let clear = clear_cookie(true);
        assert!(clear.starts_with("inferon_session=;"), "empty value: {clear}");
        assert!(clear.contains("Max-Age=0"), "no Max-Age=0: {clear}");
        assert!(clear.contains("HttpOnly") && clear.contains("SameSite=Lax"));
        assert!(clear.contains("Secure"));
    }

#[test]
fn a_transaction_id_is_opaque_but_bounded() {
    // ITEM-093: a grouped request must never fail because of the header.
    assert_eq!(parse_id(Some("tx-42")), Some("tx-42".to_string()));
    assert_eq!(parse_id(Some(" run.1:a_b ")), Some("run.1:a_b".to_string()));
    assert_eq!(parse_id(Some("")), None);
    assert_eq!(parse_id(None), None);
    assert_eq!(parse_id(Some(&"x".repeat(MAX_SESSION_ID_LEN + 1))), None);
    assert_eq!(parse_id(Some("bad id")), None);
    assert_eq!(parse_id(Some("bad/id")), None);
}
