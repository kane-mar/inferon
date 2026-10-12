## ITEM-054: Router axis for DeepSeek-native tiers

**As a** tenant
**I want** the heuristic router to downshift an expensive reasoning model to the
cheap tier for simple prompts
**So that** cost protection applies to DeepSeek traffic, not just gpt-4o

Real usage (ITEM-018 close-out, 2026-10-12) is 99.7% `deepseek-flash`; the
ITEM-015 router (`is_gpt4o_tier` → `gpt-4o-mini`) never fires, so slice 2's
saving mechanism is **inert for the platform's default provider**.

**Acceptance Criteria:**
1. The router recognises **configurable tier pairs** (expensive → cheap) rather
   than hard-coding gpt-4o → gpt-4o-mini; a DeepSeek pair (e.g.
   `deepseek-v4-pro` → `deepseek-flash`) is preconfigured.
2. A simple prompt on the expensive DeepSeek model is downgraded; a complex one
   (tools, high temperature, long context) is not.
3. The decision is recorded on the trace (like the ITEM-045 routing event) so the
   saving is observable.
4. The tenant router toggle/threshold (ITEM-046) applies to whichever pair.

**Technical Notes:**
- Generalise `router::is_gpt4o_tier` / `maybe_downgrade` into a pair table,
  keeping the existing gpt-4o pair working.
- Only downgrade when the request is on the expensive member of a pair.

**Source:** ITEM-018 AC5 finding, 2026-10-12 real traffic.

## Close-out (2026-10-12)

- Router generalised to a `TIER_PAIRS` table (`gpt-4o` → `gpt-4o-mini`,
  `deepseek-v4-pro` → `deepseek-flash`); `downgrade_target()` +
  `maybe_downgrade_with()`.
- `budget::check_budget` returns `ModelOverride { model, reason }`
  (`heuristic_router` | `budget`); the proxy records the router's downshift on
  the trace via migration 024 (`traces.router_event`).
- Tests: a DeepSeek-pair unit test + an integration test that a simple
  `deepseek-v4-pro` request serves `deepseek-flash` and traces the event.
- **Live**: a real simple `deepseek-v4-pro` request served `deepseek-flash`,
  `router_event = {from_model: deepseek-v4-pro, to_model: deepseek-flash,
  reason: heuristic_router}`.
