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
