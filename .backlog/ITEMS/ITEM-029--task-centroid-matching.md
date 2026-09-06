## ITEM-029: OmniRouting — Task-Centroid Matching (Slice 5 · was 022b)

**As a** product owner
**I want** the embedder's vector to resolve to a task class
**So that** routing decisions can compare predicted quality per task

**Acceptance Criteria:**
1. Task centroids are built/refreshed from the existing trace data (clusters by system_prompt_hash + linter class) — no hand-labeled corpus required in the MVP
2. Each prompt's embedding is cosine-matched against centroids in memory; the winning task assignment feeds the routing decision and lands on the trace
3. A no-match prompt falls back gracefully to the ITEM-015 heuristic path (embedding never blocks a request)
4. Classification quality is gated against the ITEM-015 heuristic on the same corpus before this becomes a routing input

**Technical Notes:**
- Parent: ITEM-022 (archived), slice 022b
- Depends on ITEM-028 (the embedder); consumes the centroid shape built from ITEM-026's negative samples later
- Ordering question (from parent, still open): is this worth building before ITEM-018/020 bring real traffic and real task clusters? If not, ITEM-029/030 may drop below 020's alerts when 018 lands
