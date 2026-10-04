## ITEM-028: OmniRouting — FastEmbedder (Slice 5 · was 022a)

**As a** product owner
**I want** every prompt embedded in-process in under 3ms
**So that** task-aware routing can use semantic signal instead of heuristics alone

**Acceptance Criteria:**
1. The proxy embeds prompts with the `ort` (ONNX Runtime) crate — a quantized BGE-micro-class model, (1,128) token ids/attention mask → 384-d normalized vector (spec `onnx_embed.rs` params: Level3 graph opt, 4 intra threads)
2. Embedding adds <3ms on the hot path (Slice 5 latency target), measured, not assumed
3. Model artifact has a documented shipping path (image layer vs first-boot download) — the choice must respect the ~25GB snapshot/backup story
4. Pure Rust at runtime; no Python in the serving path (project decision); graceful degradation if the embedder is unavailable (router falls back to ITEM-015 heuristics)

**Technical Notes:**
- Parent: ITEM-022 (archived), slice 022a — first OmniRouting slice in pull order
- Open decision from parent: embedding artifact size vs quality (BGE-micro ~30MB class) + shipping path

## Engineering review 2026-09-06 — pulled and returned to backlog (staged)

ITEM-028 reached the top of the pullable region after the ITEM-023..027 guard
chain. Pulling it surfaced hard blockers with measured evidence; the item is
staged (not done, not archived) pending a PO decision. Evidence:

- **Dev toolchain cannot build ort**: `ort` 2.x (the only non-yanked line on
  crates.io) publishes no prebuilt ONNX Runtime for `x86_64-apple-darwin`
  (build fails: "no prebuilt binaries available"). The ort 1.x line is
  entirely yanked; `v1.16.3` builds on this macOS-11 x64 machine only as a
  git-tag dependency. Every guard-chain slice depended on the local cargo
  test loop; an ort dependency that only compiles on the Linux host breaks
  the TDD loop (host-only builds are ~30 min round trips on the 1 vCPU
  durable host).
- **Real artifact measured, AC2 not met**: the authentic micro-class artifact
  (TaylorAI/bge-micro-v2 quantized int8 ONNX, 17 MB — the real "BGE-micro")
  measured **mean 7.4 ms** per (1,128) embed on this machine (Level3 graph
  opt, 4 intra threads); all-MiniLM-L6-v2 int8 (23 MB) measured 13 ms. The
  spec's <3 ms target is not reachable on available hardware with any
  existing downloadable micro-class artifact — a sub-3 ms model is itself a
  distillation product (parked Slice 3).
- Shipping path would otherwise be fine: a ~20 MB model + ONNX Runtime layer
  in the Docker image is trivial against the ~25 GB snapshot story.

Staged with unpark triggers (PO choice among these, see REFINEMENT_LOG
2026-09-06):
1. PO re-baselines AC2 (e.g. "≤10 ms on current hardware") AND accepts a
   linux-gated build loop for ort code → pull 028 now;
2. Slice 3 (distillation) is unparked and produces the micro artifact that
   meets <3 ms → 028 rides the 031/032 gate;
3. A newer-CPU proxy tier arrives where the existing artifact measures <3 ms
   (gate re-opens on a measurement).

## Decision + implementation (2026-10-04) — Option 1 accepted; feature gate landed

**PO decision:** take **Option 1** — re-baseline AC2 provisionally (target
≤10 ms on the production tier, pending an actual host measurement) and accept
the **linux-gated build loop** for ORT code. Sequencing: **traffic → measure on
host → finalize the AC/model path.**

**Mitigation landed (this repo):** the ORT path is now an opt-in cargo feature,
so the default build and the macOS TDD loop never compile/link ONNX Runtime:

- `[features] embedder = ["dep:ort", "dep:tokenizers"]`; `ort 2.0.0-rc.13` with
  `load-dynamic` (link-free: type-checks on any platform; the runtime library is
  only needed where it runs) + `tokenizers 0.21` with `fancy-regex` (pure Rust).
- `src/embed.rs`: `Embedder` seam + `NoEmbedder` graceful fallback (AC4), fixed
  128-token window fit (keeps leading `[CLS]`/trailing `[SEP]`, zero-pads),
  attention-masked mean pooling, L2 normalization, `from_env()` loader, and the
  `OrtEmbedder` (Level3 graph opt, 4 intra threads, 384-d) behind the feature.
- Host measurement harness: `embed_bench` (ignored test) —
  `cargo test --features embedder --release -- --ignored --nocapture embed_bench`.
- **Verified:** default `cargo test` green (184 + 5 new embed tests);
  `cargo check --features embedder` and `cargo test --no-run --features embedder`
  compile locally (so the ORT code is type-checked in the normal loop).
- **Not wired into routing**: nothing calls the embedder yet (029/030 consume it),
  so request behavior is byte-identical; the fallback contract is in place.
- Artifact note: ORT 1.28 ships no `x86_64-apple-darwin` prebuilt (only
  aarch64) — runtime is Linux-only, as the linux-gated loop assumes.

**Measurement blocked (2026-10-04):** the durable host was lost (ITEM-041) —
`embed_bench` cannot run until a host exists. When one does: fetch
`TaylorAI/bge-micro-v2` `onnx/model_quantized.onnx` + `tokenizer.json`, build
with `--features embedder` (ort-sys downloads the matching ORT 1.28
`x86_64-unknown-linux-gnu`), run the bench on the target tier, record mean/p50/
p95, then finalize AC2 and the model shipping path (image layer: ~20 MB model +
ORT runtime, trivial against the ~25 GB snapshot story).
