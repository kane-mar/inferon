## ITEM-079: The prompt-embedder artifact actually runs

**As a** developer building the task-aware router
**I want** the ONNX embedder to load and embed on a dev machine, not just type-check
**So that** ITEM-029's centroids can be developed with a real signal (TDD, not
host-only)

**Found:** 2026-10-26 — `cargo check --features embedder` passed but no runtime
existed: the pinned `api-27` needs ONNX Runtime 1.27, and 1.27 publishes **no
macOS x86_64** build (arm64 only). The dev machine is macOS 11 x86_64.

**Acceptance Criteria**
1. A repeatable fetch of the model (TaylorAI/bge-micro-v2 quantized int8), its
   tokenizer, and a compatible ONNX Runtime library, per platform.
2. A real embed loads and runs on the dev machine (macOS 11 x86_64) and the
   library path is documented for Linux.
3. The artifact-gated tests run: a latency bench and a semantic check (related
   prompts closer than unrelated, unit norm).
4. The default build stays ORT-free.

## Close-out (2026-10-26)

- `scripts/fetch-embedder.sh` downloads the quantized BGE-micro ONNX (17 MB), its
  `tokenizer.json`, and ONNX Runtime for the current platform; idempotent, prints
  the env vars.
- **Pinned ORT 1.19.2** (`ort` feature `api-19`): the newest release with a macOS
  **x86_64** build targeting macOS 11 (1.23+ needs macOS 13 libc++ symbols).
  Linux x64 ships 1.19.2 too.
- `GraphOptimizationLevel::All` replaces the crate's `Level3`, which maps to
  `ORT_ENABLE_LAYOUT` (ORT 1.21+); `All` is the same level-3 intent on 1.19.
- New artifact-gated test `embed_semantics` (related vs unrelated + unit norm).
- **Verified** on macOS 11.7.11 x86_64: `embed_bench` **p50 8.3–9.5 ms**,
  p95 11.4–13.8 ms (50 runs); `embed_semantics` passed. Default suite 314 tests,
  clippy clean with and without the feature.
- **ITEM-029's embedder gate is cleared**; its remaining gate is real task
  traffic for centroid building.
