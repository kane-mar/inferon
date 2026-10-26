//! Prompt embedder for task-aware routing (ITEM-028).
//!
//! The semantic signal is an **opt-in build**: the `embedder` cargo feature
//! pulls in ONNX Runtime + a tokenizer, and the default build — including the
//! macOS TDD loop — compiles neither. The Linux host enables the feature and
//! ships the runtime library in the image; `ort`'s `load-dynamic` keeps the
//! crate link-free, so this module type-checks on any platform and only needs
//! `libonnxruntime` at run time (`ORT_DYLIB_PATH` or a system path).
//!
//! Without the feature (or without a configured model) `NoEmbedder` is the
//! fallback and callers keep using the ITEM-015 heuristics — embedding is
//! additive, never a hard dependency (ITEM-028 AC4).
//!
//! Model: TaylorAI/bge-micro-v2 quantized int8 ONNX + `tokenizer.json`; the
//! spec's params are (1,128) token ids / attention mask / segment ids →
//! 384-d normalized vector, graph optimization Level3, 4 intra threads.
//!
//! Latency (measured on the production tier, 1 vCPU): **~141–150 ms** per
//! embed. Embedding is therefore an **off-hot-path, task-level** signal —
//! callers embed a task/system-prompt once and cache the vector (ITEM-029),
//! never once per request (ITEM-028 AC2, re-baselined 2026-10-05).

/// BGE-micro vector width (spec: 384-d).
pub const EMBED_DIM: usize = 384;
/// Max sequence length (spec: 128 tokens).
pub const MAX_TOKENS: usize = 128;

/// Embedding seam: `None` means "no semantic signal" — callers must degrade
/// gracefully to the heuristic path (AC4).
///
/// One embed is ~150 ms on the production tier, so this is a task-level
/// primitive: callers own the cache and embed once per task/system-prompt,
/// not per request.
pub trait Embedder: Send + Sync {
    /// L2-normalized 384-d embedding of `text`, or `None` when unavailable.
    fn embed(&self, text: &str) -> Option<[f32; EMBED_DIM]>;
}

/// Default fallback: no embedder configured (or the feature is off).
pub struct NoEmbedder;

impl Embedder for NoEmbedder {
    fn embed(&self, _text: &str) -> Option<[f32; EMBED_DIM]> {
        None
    }
}

/// Fit token ids into the fixed `MAX_TOKENS` window: truncate long inputs
/// keeping the leading `[CLS]` and trailing `[SEP]`, then zero-pad (pad id 0)
/// with a matching attention mask (1 real, 0 pad).
#[cfg(any(test, feature = "embedder"))]
pub(crate) fn fit_to_model_window(ids: &[u32]) -> ([i64; MAX_TOKENS], [i64; MAX_TOKENS]) {
    let mut kept: Vec<u32> = if ids.len() > MAX_TOKENS {
        let mut k = ids[..MAX_TOKENS - 1].to_vec();
        k.push(*ids.last().expect("non-empty"));
        k
    } else {
        ids.to_vec()
    };
    let real = kept.len();
    kept.resize(MAX_TOKENS, 0);
    let mut out_ids = [0i64; MAX_TOKENS];
    let mut mask = [0i64; MAX_TOKENS];
    for (i, id) in kept.iter().enumerate() {
        out_ids[i] = *id as i64;
        mask[i] = if i < real { 1 } else { 0 };
    }
    (out_ids, mask)
}

/// Attention-masked mean pooling over `tokens × EMBED_DIM` hidden states.
/// Panics only on a malformed buffer (a programming error, not user input).
#[cfg(any(test, feature = "embedder"))]
pub(crate) fn mean_pool(hidden: &[f32], mask: &[i64], tokens: usize) -> [f32; EMBED_DIM] {
    assert_eq!(hidden.len(), tokens * EMBED_DIM, "hidden state shape");
    assert_eq!(mask.len(), tokens, "attention mask shape");
    let mut pooled = [0f32; EMBED_DIM];
    let mut count = 0f32;
    for t in 0..tokens {
        if mask[t] == 0 {
            continue;
        }
        count += 1.0;
        for d in 0..EMBED_DIM {
            pooled[d] += hidden[t * EMBED_DIM + d];
        }
    }
    if count > 0.0 {
        for v in pooled.iter_mut() {
            *v /= count;
        }
    }
    pooled
}

/// L2-normalize a vector (a zero vector stays zero).
#[cfg(any(test, feature = "embedder"))]
pub(crate) fn l2_normalize(mut v: [f32; EMBED_DIM]) -> [f32; EMBED_DIM] {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

/// Build the configured embedder. Feature off, paths unset, or model load
/// failure → `NoEmbedder` (logged), so the proxy always starts (AC4).
pub fn from_env() -> std::sync::Arc<dyn Embedder> {
    let model = std::env::var("EMBED_MODEL_PATH")
        .ok()
        .filter(|p| !p.is_empty());
    let tokenizer = std::env::var("EMBED_TOKENIZER_PATH")
        .ok()
        .filter(|p| !p.is_empty());
    #[cfg(feature = "embedder")]
    if let (Some(model), Some(tokenizer)) = (model.clone(), tokenizer.clone()) {
        match ort_impl::OrtEmbedder::load(&model, &tokenizer) {
            Ok(embedder) => {
                tracing::info!("prompt embedder loaded from {model}");
                return std::sync::Arc::new(embedder);
            }
            Err(e) => tracing::warn!("embedder unavailable ({e}); using heuristics"),
        }
    }
    let _ = (model, tokenizer);
    if cfg!(feature = "embedder") {
        tracing::info!("no embedder configured (EMBED_MODEL_PATH / EMBED_TOKENIZER_PATH) — routing stays heuristic");
    }
    std::sync::Arc::new(NoEmbedder)
}

#[cfg(feature = "embedder")]
mod ort_impl {
    use super::{fit_to_model_window, l2_normalize, mean_pool, Embedder, EMBED_DIM, MAX_TOKENS};
    use ort::session::builder::GraphOptimizationLevel;
    use ort::session::Session;
    use ort::value::Tensor;

    /// ONNX Runtime BGE-micro embedder (feature `embedder`).
    pub struct OrtEmbedder {
        session: std::sync::Mutex<Session>,
        tokenizer: tokenizers::Tokenizer,
    }

    impl OrtEmbedder {
        pub fn load(model_path: &str, tokenizer_path: &str) -> Result<Self, String> {
            let session = Session::builder()
                .map_err(|e| format!("session builder: {e}"))?
                // ORT's "level 3 / all optimizations". The crate's `Level3` maps to
                // ORT_ENABLE_LAYOUT, which only exists in ORT 1.21+; `All` is the
                // same intent and valid on the macOS-11-compatible runtime (1.19).
                .with_optimization_level(GraphOptimizationLevel::All)
                .map_err(|e| format!("optimization level: {e}"))?
                .with_intra_threads(4)
                .map_err(|e| format!("intra threads: {e}"))?
                .commit_from_file(model_path)
                .map_err(|e| format!("load {model_path}: {e}"))?;
            let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
                .map_err(|e| format!("load {tokenizer_path}: {e}"))?;
            Ok(Self {
                session: std::sync::Mutex::new(session),
                tokenizer,
            })
        }
    }

    impl Embedder for OrtEmbedder {
        fn embed(&self, text: &str) -> Option<[f32; EMBED_DIM]> {
            let encoding = self.tokenizer.encode(text, true).ok()?;
            let (ids, mask) = fit_to_model_window(encoding.get_ids());
            let ids = Tensor::from_array(([1usize, MAX_TOKENS], ids.to_vec())).ok()?;
            let mask_t = Tensor::from_array(([1usize, MAX_TOKENS], mask.to_vec())).ok()?;
            // BGE models take a segment-ids input; single-segment text is all zeros.
            let token_type_ids =
                Tensor::from_array(([1usize, MAX_TOKENS], [0i64; MAX_TOKENS].to_vec())).ok()?;
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let outputs = session
                .run(ort::inputs![
                    "input_ids" => ids,
                    "attention_mask" => mask_t,
                    "token_type_ids" => token_type_ids,
                ])
                .ok()?;
            let (_, hidden) = outputs["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .ok()?;
            let pooled = mean_pool(hidden, &mask, MAX_TOKENS);
            Some(l2_normalize(pooled))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Host-only latency bench (ITEM-028 AC2/measurement): needs the model
        /// + tokenizer + libonnxruntime on the Linux host.
        ///
        /// cargo test --features embedder --release -- --ignored --nocapture embed_bench
        #[test]
        #[ignore = "needs the model artifact + ONNX Runtime; run on the host"]
        fn embed_bench() {
            let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();
            let embedder = crate::embed::from_env();
            let probe = "Summarize this quarterly report into three bullets for an executive audience.";
            let _ = embedder.embed(probe);
            let runs = 50;
            let mut samples = Vec::with_capacity(runs);
            for _ in 0..runs {
                let started = std::time::Instant::now();
                let out = embedder.embed(probe);
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
                assert!(out.is_some(), "embedder must be configured for the bench");
            }
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mean: f64 = samples.iter().sum::<f64>() / runs as f64;
            let p50 = samples[runs / 2];
            let p95 = samples[(runs * 95) / 100];
            println!("embed_bench: mean {mean:.2}ms p50 {p50:.2}ms p95 {p95:.2}ms ({runs} runs)");
        }

        /// Real-model semantics: related prompts must be closer than unrelated
        /// ones, and every vector is unit length. Artifact-gated like the bench:
        ///
        /// cargo test --features embedder --release -- --ignored --nocapture embed_semantics
        #[test]
        #[ignore = "needs the model artifact + ONNX Runtime; run on the host"]
        fn embed_semantics() {
            let embedder = crate::embed::from_env();
            let related_a = embedder
                .embed("Summarize this quarterly report into three bullets for an executive audience.")
                .expect("embed a");
            let related_b = embedder
                .embed("Give me a three-bullet executive summary of the quarterly results.")
                .expect("embed b");
            let unrelated = embedder
                .embed("Explain how to poach an egg.")
                .expect("embed c");
            let cosine = |x: &[f32; EMBED_DIM], y: &[f32; EMBED_DIM]| -> f32 {
                x.iter().zip(y).map(|(p, q)| p * q).sum()
            };
            let near = cosine(&related_a, &related_b);
            let far = cosine(&related_a, &unrelated);
            assert!(near > far, "related {near:.3} should beat unrelated {far:.3}");
            let norm = |v: &[f32; EMBED_DIM]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm(&related_a) - 1.0).abs() < 1e-4, "unit norm, got {}", norm(&related_a));
        }
    }
}

#[cfg(test)]
mod tests;
