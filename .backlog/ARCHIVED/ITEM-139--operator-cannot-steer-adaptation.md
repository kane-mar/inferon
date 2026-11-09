## ITEM-139: The operator could not see or steer adaptation

**As** the operator of the platform
**I want** to inspect and configure the adaptive loop
**So that** "progressively inspecting and adapting to data" is something I can
actually run, not a claim in the roadmap

**Reported** 2026-11-09 (PO): "there doesn't appear to be a place to configure AI
for the operator, when progressively inspecting and adapting to data."

**What was true.** The adaptive policy was fully built and unreachable. ITEM-064
stores a versioned policy per workspace — router threshold, per-provider health
weights, evidence for why the version exists, `enabled` as a kill switch — and
ITEM-069 applies the weights on the hot path. Three operator endpoints existed
(`GET policy/:tenant`, `POST …/freeze`, `POST …/rollback/:version`). **Nothing in
the UI called any of them**, and there was no way to *set* a policy at all: the
only human actions were "freeze everything" and "go back".

That is the worst shape a capability can be in — the work is done, the control is
absent, and the operator cannot tell whether adaptation is even running.

**Delivered**

- `PUT /v1/dashboard/api/operator/policy/:tenant_id` publishes a new version.
  Every field is optional, so a change touches what it names and leaves the rest —
  the loop itself publishes versions, and a threshold edit must not wipe the
  weights it learned yesterday.
- Guardrails at the edge, matching the loop's own bounds (`MIN_THRESHOLD`
  .. `MAX_THRESHOLD`, weights `0..1`): an out-of-range value is **refused, not
  clamped**, because a number the operator did not type is a number they cannot
  debug later. Operator-only, and it invalidates the hot-path cache so the next
  request uses the new version.
- Evidence records `source: "operator"` (and the timestamp), so a bad manual steer
  is undone by the same `rollback` that undoes a bad automatic step. The history
  stays one list of versions.
- The operator console gains an **Adaptation** card: choose a workspace, read the
  version, state (adapting/frozen), provider weights and the raw evidence, then set
  the router threshold, freeze, or roll back one version.

**Tests** (2 new, 591 total): a tenant key is refused; a partial body changes only
what it names and a later body that omits the threshold leaves it intact; the
version increments; the loop reads the change back; rollback works on an operator
version; and three out-of-range bodies (threshold above the ceiling, below the
floor, a weight over 1) are each a 400.

**What is still not configurable** — deliberately, and recorded rather than hidden:

- **Which models the router may choose.** The tier pairs (ITEM-081) and the router
  threshold define the *shape* of a downgrade; the choice inside it is the
  heuristic's. A per-workspace allow-list of downgrade targets is the next natural
  control, and belongs with ITEM-072's per-cluster preference rather than here.
- **The loop's own aggressiveness.** `STEP` (0.05 per version) and `MIN_SAMPLE` (20)
  are constants. Making them settings invites tuning a control loop by feel; they
  should become configurable when there is evidence that the defaults are wrong.
- **A history view.** Versions are stored and rollback addresses them by number;
  the console shows the active one. A version list with evidence is a small
  addition worth doing when an operator asks to see one.

**Source:** PO request, 2026-11-09.
