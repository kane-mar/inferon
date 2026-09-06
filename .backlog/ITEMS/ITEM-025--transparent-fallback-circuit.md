## ITEM-025: Schema Guard — Transparent Fallback Circuit (Slice 4 · was 021c)

**As a** developer on the proxy
**I want** a failed cheap-model response to silently become a frontier-model response
**So that** the end user never sees an error or a malformed body

**Acceptance Criteria:**
1. On any failure classified by ITEM-024: the bad buffered response is discarded and the original prompt is replayed to the tenant's frontier model with the tenant's upstream key; the replacement response streams to the client
2. The client receives exactly one response — a latency spike on that request, never an error, never a duplicated/partial body
3. Exactly one trace is emitted for the request, recording the guard outcome + failure reason (the fallback is not a second request in tenant accounting)
4. If the fallback upstream itself fails, the request fails loudly and cleanly (no silent data loss) with the fallback reason preserved on the trace

**Technical Notes:**
- Parent: ITEM-021 (archived), slice 021c
- Depends on ITEM-023 + ITEM-024
- Open decision from parent: the frontier model defaults to the tenant's *original requested model* (pre-ITEM-015 downgrade — the correct "frontier" in a DeepSeek-only world); an explicit per-tenant configured frontier is the alternative — confirm before building
