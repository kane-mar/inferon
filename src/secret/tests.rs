// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn round_trips_a_secret() {
        let secrets = Secrets::from_hex(KEY).expect("valid key");
        let ciphertext = secrets.encrypt("sk-provider-secret").expect("encrypt");
        assert_ne!(ciphertext, "sk-provider-secret");
        assert_eq!(secrets.decrypt(&ciphertext).as_deref(), Some("sk-provider-secret"));
    }

    #[test]
    fn ciphertext_is_non_deterministic() {
        let secrets = Secrets::from_hex(KEY).expect("valid key");
        assert_ne!(secrets.encrypt("x"), secrets.encrypt("x"));
    }

    #[test]
    fn a_different_key_cannot_decrypt() {
        let a = Secrets::from_hex(KEY).expect("valid key");
        let b = Secrets::from_hex(
            "ff0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        )
        .expect("valid key");
        let ciphertext = a.encrypt("secret").expect("encrypt");
        assert_eq!(b.decrypt(&ciphertext), None);
    }

    #[test]
    fn rejects_bad_master_keys_and_ciphertext() {
        assert!(Secrets::from_hex("not-hex").is_none());
        assert!(Secrets::from_hex("0011").is_none(), "wrong length");
        let secrets = Secrets::from_hex(KEY).expect("valid key");
        assert_eq!(secrets.decrypt("zzzz"), None);
        assert_eq!(secrets.decrypt(""), None);
        // Flip a bit in a valid ciphertext → authentication failure.
        let ciphertext = secrets.encrypt("secret").expect("encrypt");
        let mut bytes = hex::decode(&ciphertext).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        assert_eq!(secrets.decrypt(&hex::encode(bytes)), None);
    }
