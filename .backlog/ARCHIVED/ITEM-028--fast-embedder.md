## ITEM-028: OmniRouting — FastEmbedder (Slice 5 · was 022a)

**As a** product owner
**I want** every prompt embedded in-process in under 3ms
**So that** task-aware routing can use semantic signal instead of heuristics alone

**Acceptance Criteria:**
1. The proxy embeds prompts with the `ort` (ONNX Runtime) crate — a quantized BGE-micro-class model, (1,128) token ids / attention mask / segment ids → 384-d normalized vector (spec `onnx_embed.rs` params: Level3 graph opt, 4 intra threads)
2. Re-baselined (2026-10-05, measured): one embed takes **≤150 ms** on the production tier (measured ~141–150 ms p50), so embedding runs **off the hot path** — once per task/system-prompt, cached, never per request
3. Model artifact has a documented shipping path — **image layer** (the ~17 MB model + ~8.8 MB ORT `libonnxruntime.so` bake into the image; trivial against the ~25 GB snapshot story)
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

## Measurement (2026-10-05) — done; AC2 NOT met

Ran `embed_bench` on the durable host (vc2-1c-1gb, 1 vCPU, 955 MB, ITEM-041
rebuilt). Model `TaylorAI/bge-micro-v2` int8 (17 MB) + ONNX Runtime 1.28.0
linux-x64, `ORT_DYLIB_PATH` set, `--features embedder --release`, 50 runs × 3:

| run | mean | p50 | p95 |
|-----|------|-----|-----|
| 1 | 174.52 ms | 143.20 ms | 296.33 ms |
| 2 | 162.01 ms | 140.96 ms | 270.33 ms |
| 3 | 171.59 ms | 150.14 ms | 298.23 ms |

**~141–150 ms p50** — the re-baselined AC2 target (≤10 ms) is missed by ~14×,
and the original <3 ms spec is ~50× off. The 1 vCPU tier is the bottleneck
(the same model measured 7.4 ms on the dev Mac).

Two code issues surfaced by the first real run (commit `ad05ed9`):
- **Missing `token_type_ids` input** — bge-micro-v2 takes segment ids; the
  embedder only fed `input_ids` + `attention_mask`, so the session run failed
  (`Missing Input: token_type_ids`). Single-segment text = all-zero ids. This
  bug was invisible while the bench was blocked.
- **Bench hid load failures** — `from_env()` logs the cause via tracing but the
  ignored bench never initialised a subscriber; it now does (`warn` filter).

**AC2 outcome: not met on the current tier.** The Option-1 assumption (≤10 ms
on "current hardware") is falsified by measurement. Unpark options now:
1. Re-baseline AC2 again around the measured tier (e.g. ≤150 ms) **and** move
   embedding off the hot path (cache per task/system-prompt, not per request —
   the 029 centroid design already embeds the task, not every message);
2. A newer/faster CPU tier (re-run the bench — the gate re-opens on a
   measurement < target);
3. Re-stage 028 until Slice 3 distillation produces a <3 ms model.

**AC3 (shipping path) unresolved**, pending the AC2 decision — but the
mechanics are confirmed trivial: the ~17 MB model + the 8.8 MB ORT runtime
`libonnxruntime.so` bake into the image layer well within the ~25 GB snapshot
story (image layer is the right call; no first-boot download needed).

## Decision (2026-10-05) — AC2 re-baselined (Option 1)

**PO chose Option 1**: re-baseline AC2 to **≤150 ms on the production tier**
(measured, not assumed — ~141–150 ms p50) and treat the embedder as an
**off-hot-path, task-level** signal: embed once per task/system-prompt and
cache the vector (ITEM-029's centroid design already embeds tasks, not
individual requests). The `Embedder` seam stays per-embed (a blocking ~150 ms
call); callers (029/030) own the cache. AC3 (shipping path) resolves to the
**image layer** (model + ORT runtime baked into the image).

Remaining for 028 itself: none pending — the embedder builds, runs, measures,
and degrades gracefully (AC4). Wiring into routing is 029/030, gated on real
task traffic for the centroids.

## Verification record (2026-10-05)

- **AC1 (embed with ort)**: `OrtEmbedder` loads `bge-micro-v2` int8 ONNX +
  `tokenizer.json` and returns a 384-d L2-normalized vector; `embed_bench` runs
  on the host (Level3 opt, 4 intra threads). The first real run surfaced a
  missing `token_type_ids` input — fixed.
- **AC2 (latency, re-baselined)**: measured ~141–150 ms p50 on the production
  tier (vc2-1c-1gb); re-baselined to ≤150 ms off-hot-path (task-level cache).
- **AC3 (shipping path)**: **image layer** — ~17 MB model + ~8.8 MB ORT
  `libonnxruntime.so` bake into the image (trivial vs the ~25 GB snapshot).
- **AC4 (pure Rust + graceful degradation)**: runtime is pure Rust (ort
  `load-dynamic`, no Python); `NoEmbedder` fallback is unit-tested
  (`no_embedder_returns_none`); routing stays heuristic when unavailable.
- Default `cargo test` 190 green; `cargo check --features embedder` green.
