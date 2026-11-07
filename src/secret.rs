// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! At-rest encryption for provider credentials (ITEM-043).
//!
//! XChaCha20-Poly1305 (AEAD) keyed by the env master key
//! `PROVIDER_ENCRYPTION_KEY` (64 hex chars = 32 bytes). Ciphertext is
//! `hex(nonce || ciphertext+tag)`. Pure Rust — no C toolchain, no bespoke
//! crypto.
//!
//! No/invalid master key → [`Secrets::from_env`] returns `None`, which disables
//! provider provisioning (the proxy keeps using the shared platform key). The
//! key is never logged.

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, XNonce,
};

const NONCE_LEN: usize = 24;

pub struct Secrets {
    cipher: XChaCha20Poly1305,
}

impl Secrets {
    pub fn from_env() -> Option<Self> {
        std::env::var("PROVIDER_ENCRYPTION_KEY")
            .ok()
            .filter(|value| !value.is_empty())
            .and_then(|value| Self::from_hex(&value))
    }

    pub(crate) fn from_hex(master_hex: &str) -> Option<Self> {
        let bytes = hex::decode(master_hex.trim()).ok()?;
        let key: [u8; 32] = bytes.try_into().ok()?;
        Some(Self {
            cipher: XChaCha20Poly1305::new_from_slice(&key).ok()?,
        })
    }

    /// Encrypt `plaintext`, returning `hex(nonce || ciphertext+tag)`.
    pub fn encrypt(&self, plaintext: &str) -> Option<String> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self.cipher.encrypt(&nonce, plaintext.as_bytes()).ok()?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ciphertext);
        Some(hex::encode(out))
    }

    /// Decrypt a value produced by [`Secrets::encrypt`]. `None` on tampering,
    /// wrong key, or malformed input.
    pub fn decrypt(&self, encoded: &str) -> Option<String> {
        let raw = hex::decode(encoded).ok()?;
        if raw.len() <= NONCE_LEN {
            return None;
        }
        let (nonce, ciphertext) = raw.split_at(NONCE_LEN);
        let plaintext = self
            .cipher
            .decrypt(XNonce::from_slice(nonce), ciphertext)
            .ok()?;
        String::from_utf8(plaintext).ok()
    }
}

#[cfg(test)]
mod tests;
