## ITEM-089: Semantic task signatures for grouping

**As a** tenant with many similar prompts
**I want** near-identical tasks grouped into one dataset cluster
**So that** "grouped by task signature" means the task, not an exact string match

**Claim (2026-10-30 brief).** "It auto-indexes these input-output pairs into clean,
deduplicated datasets **grouped by task signature**."

**Current state (2026-10-30).** Grouping is `system_prompt_hash`, an exact content
hash. The ONNX embedder (ITEM-028/079, `embed` feature) is built and benchmarked but
has **no consumer** outside its own module; ITEM-029 (task centroids for routing) is
staged and gated on traffic.

**Gap.** No semantic grouping and no use of the embedder.

**Acceptance Criteria**
1. Each request gets a **task signature**: the embedder's vector over the system
   prompt (falling back to the first user turn when there is no system prompt), with
   the embedder model id and dimension recorded so the signature is versioned.
2. Signatures are clustered with a documented cosine threshold; the assigned cluster
   id is stored on the trace (additive column) and the cluster has a stable label.
3. Counts and exports work by exact hash **and** by cluster, and the dashboard shows
   which grouping is in use.
4. With the embedder feature off (`NoEmbedder`), grouping degrades to the exact hash
   — no errors, no empty page.
5. Clustering is deterministic for a fixed input set (seeded) and bounded in cost
   (batch, not per-request in the hot path; a periodic pass is acceptable).
6. Composes with ITEM-029: the same signature can feed centroid matching without a
   second embedding pipeline.

**Technical Notes**
- `embed::from_env()` already returns `NoEmbedder` when the feature/artifact is
  absent; wire it through `AppState`.
- Cluster ids must survive re-clustering (store the label + a representative hash),
  otherwise datasets jump around between runs.
- 774 traces today is far below a clustering threshold; the item should ship with a
  synthetic-corpus test, not wait for production volume.

**Depends on** ITEM-085/086 for the pairs; independent of ITEM-029's routing use.
**Ranking note.** Rank **8** of 22 · V3.5/E3 — grouping quality; the flywheel works on exact hashes until this lands, so it does not block 087.
---

**Delivered (2026-10-31).** `src/signatures.rs` (pure: `task_text`, `cosine`,
`assign_clusters` — deterministic leader clustering in key order, label = the
representative prompt's hash so a superset keeps the labels it shared). The
periodic pass (`run_pass`, every 300 s, `PASS_BATCH` = 50) reads one payload per
distinct unclustered prompt, embeds off the request path, upserts
`task_signatures` (vector + version + model + source) and points the traces at
the label. Prompts whose payload was not captured, or where `NoEmbedder` returns
nothing, get the exact-hash label — so the pass always drains and the page never
empties. Datasets read model groups by `COALESCE(task_cluster, system_prompt_hash)`
when clustering is in use (`grouping` in the payload says which), and export
accepts `cluster=<label>` as well as `prompt_hash=<hash>`.

Verified: 10 unit tests (text selection, cosine, cluster/dedup/order-independence,
superset stability). Live verification: after the deploy, the datasets payload
reports `grouping`, and the pass logs `task signatures: labelled N prompt(s)`.
