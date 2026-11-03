## ITEM-106: One presentation language across the reports

**As a** reader
**I want** every page to answer the same way
**So that** I can compare without re-learning each screen

**Current state (2026-11-03).** Windows differ and are unlabelled (Runtime/Datasets
hard-code 30 days; others have a switcher); the same metric appears on up to five
pages; KPI rows run 3/4/6/7 up; money prints `$0.000129`; hashes truncate to ten
characters with no tooltip; the heatmap has no legend.

**Acceptance Criteria**
1. A `WindowChip` on every report header: "Last 30 days · change", one component.
2. One KPI component: value, unit, window, and a `title` defining the metric.
3. Money adapts: two decimals above a cent, up to six below it, never trailing
   zeros.
4. Truncated hashes and timestamps carry their full value in a `title`.
5. The heatmap has a legend (what the tint means, and that a dot is no traffic).
