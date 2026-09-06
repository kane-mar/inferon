//! Session tokens + the session cookie (ITEM-035).
//!
//! A session token is `"<user_id>.<expiry_hex>.<hmac_hex>"` where the HMAC is
//! HMAC-SHA256 over `"<user_id>.<expiry_hex>"` keyed by the `SESSION_SECRET`
//! env secret. Verification never errors: a forged, expired, or malformed
//! token yields `None` — the caller treats it as signed out (ITEM-035 AC2).
//!
//! Cookie plumbing is hand-rolled (one read header, one write header) instead
//! of a cookie crate — the smaller surface for a single session cookie
//! (decision recorded at pull, ITEM-035).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "inferon_session";
/// Name of the OAuth CSRF-state cookie (short-lived, cleared after callback).
pub const OAUTH_STATE_COOKIE: &str = "inferon_oauth_state";
/// Session lifetime: 30 days, fixed (no sliding renewal yet).
pub const SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60;
/// Lifetime of the OAuth CSRF state cookie.
pub const OAUTH_STATE_TTL_SECS: u64 = 10 * 60;

pub fn issue_session(user_id: Uuid, secret: &str, now_secs: u64, ttl_secs: u64) -> String {
    let exp = now_secs.saturating_add(ttl_secs);
    let payload = format!("{user_id}.{exp:016x}");
    format!("{payload}.{}", hex::encode(hmac_sha256(secret, &payload)))
}

pub fn verify_session(token: &str, secret: &str, now_secs: u64) -> Option<Uuid> {
    let (payload, mac_hex) = token.rsplit_once('.')?;
    let mac_bytes = hex::decode(mac_hex).ok()?;
    // Constant-time check; any mismatch simply means signed out.
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&mac_bytes).ok()?;
    // payload = "<user_id>.<expiry hex>".
    let (id, exp_hex) = payload.rsplit_once('.')?;
    let user_id = Uuid::parse_str(id).ok()?;
    let exp = u64::from_str_radix(exp_hex, 16).ok()?;
    if now_secs >= exp {
        return None;
    }
    Some(user_id)
}

/// The value of cookie `name` in a `Cookie` request header, if present.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value.trim())
    })
}

/// A `Set-Cookie` value for `name` with `value`, `max_age` seconds of life,
/// and the `HttpOnly; SameSite=Lax; Path=/` attributes (+ `Secure` over TLS).
pub fn cookie(name: &str, value: &str, max_age: u64, secure: bool) -> String {
    let secure_attr = secure.then_some("; Secure").unwrap_or("");
    format!("{name}={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure_attr}")
}

/// A `Set-Cookie` value that stores `token` in the session cookie:
/// `HttpOnly; SameSite=Lax; Path=/; Max-Age=<SESSION_TTL_SECS>`, plus `Secure`
/// when the request arrived over TLS.
pub fn set_cookie(token: &str, secure: bool) -> String {
    cookie(SESSION_COOKIE, token, SESSION_TTL_SECS, secure)
}

/// A `Set-Cookie` value that expires the session cookie immediately.
pub fn clear_cookie(secure: bool) -> String {
    cookie(SESSION_COOKIE, "", 0, secure)
}

fn hmac_sha256(secret: &str, message: &str) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
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
}
