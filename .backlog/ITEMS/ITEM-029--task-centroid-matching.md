## ITEM-029: OmniRouting — Task-Centroid Matching (Slice 5 · was 022b)

**As a** product owner
**I want** the embedder's vector to resolve to a task class
**So that** routing decisions can compare predicted quality per task

**Acceptance Criteria:**
1. Task centroids are built/refreshed from the existing trace data (clusters by system_prompt_hash + linter class) — no hand-labeled corpus required in the MVP
2. Each prompt's embedding is cosine-matched against centroids in memory; the winning task assignment feeds the routing decision and lands on the trace
3. A no-match prompt falls back gracefully to the ITEM-015 heuristic path (embedding never blocks a request)
4. Classification quality is gated against the ITEM-015 heuristic on the same corpus before this becomes a routing input

**Technical Notes (refreshed 2026-11-04, backlog review):**
- ITEM-028 (the in-process embedder) is **delivered** and runs off the hot path;
  ITEM-018 is **delivered** (real traffic flows), so the old "no real corpus"
  gate is gone.
- ITEM-089 is **delivered** and already computes what AC1 needs: a versioned
  vector per distinct prompt (stored once, never re-embedded) and cosine
  clusters at 0.92 over live traffic. The routing use (map a request to a task
  centroid) can consume that pipeline instead of building a second one.
- **The gate is now a PO decision, not a dependency**: whether centroid routing
  is worth the risk now that its input exists. If pulled, AC1's method is the
  ITEM-089 pipeline; AC4's quality gate still compares against the ITEM-015
  heuristic on the same corpus.
- Parent: ITEM-022 (archived), slice 022b. ITEM-030/072 follow this item.
