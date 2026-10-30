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

/// The optional client transaction header (ITEM-093). Opaque, tenant-scoped.
pub const SESSION_ID_HEADER: &str = "x-inferon-session-id";
/// Longest accepted transaction id.
pub const MAX_SESSION_ID_LEN: usize = 128;

/// Validate an optional `X-Inferon-Session-Id`: opaque but bounded and
/// printable, so it is safe to store and group by. Invalid values are ignored
/// (grouping is best-effort and must never fail a request).
pub fn parse_id(raw: Option<&str>) -> Option<String> {
    let id = raw?.trim();
    if id.is_empty() || id.len() > MAX_SESSION_ID_LEN {
        return None;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        .then(|| id.to_string())
}

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
    let secure_attr = if secure { "; Secure" } else { "" };
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
mod tests;
