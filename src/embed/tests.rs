// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    /// AC4: the default build degrades gracefully — no signal, no error.
    #[test]
    fn no_embedder_returns_none() {
        assert_eq!(NoEmbedder.embed("anything"), None);
    }

    #[test]
    fn window_fits_short_and_truncates_long_inputs() {
        let (ids, mask) = fit_to_model_window(&[101, 7, 8, 102]);
        assert_eq!(ids[..4], [101, 7, 8, 102]);
        assert_eq!(mask[..4], [1, 1, 1, 1]);
        assert!(ids[4..].iter().all(|i| *i == 0), "zero padding");
        assert!(mask[4..].iter().all(|m| *m == 0), "padded mask");

        // Long inputs keep the leading [CLS] and the trailing [SEP].
        let long: Vec<u32> = (0..500).collect();
        let (ids, mask) = fit_to_model_window(&long);
        assert_eq!(ids.len(), MAX_TOKENS);
        assert_eq!(ids[0], 0, "leading token kept");
        assert_eq!(ids[MAX_TOKENS - 1], 499, "trailing [SEP] kept");
        assert_eq!(mask.iter().filter(|m| **m == 1).count(), MAX_TOKENS);
    }

    #[test]
    fn mean_pool_ignores_padded_positions() {
        // Two real tokens (values 2 and 4) + one padded token (value 100).
        let mut hidden = vec![0f32; 3 * EMBED_DIM];
        hidden[0] = 2.0;
        hidden[EMBED_DIM] = 4.0;
        hidden[2 * EMBED_DIM] = 100.0;
        let pooled = mean_pool(&hidden, &[1, 1, 0], 3);
        assert_eq!(pooled[0], 3.0, "(2+4)/2, pad excluded");
    }

    #[test]
    fn l2_normalize_produces_unit_norm_and_keeps_zero() {
        let mut v = [0f32; EMBED_DIM];
        v[0] = 3.0;
        v[1] = 4.0;
        let n = l2_normalize(v);
        let norm = n.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
        assert_eq!(l2_normalize([0f32; EMBED_DIM]), [0f32; EMBED_DIM]);
    }

    /// The default (feature-off) configuration always yields the fallback.
    #[test]
    fn from_env_defaults_to_no_embedder_without_configuration() {
        // No EMBED_* env vars are set in the unit-test process.
        assert!(from_env().embed("hi").is_none());
    }
