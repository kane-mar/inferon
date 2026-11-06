## ITEM-118: What answers your requests (providers, own models, tiers, checker)

**As a** workspace owner
**I want** to add my own provider, run a model on my own network, choose which model
serves which traffic, and turn on structured output
**So that** the gateway answers with the model I chose, at the price I expect

**Slices from ITEM-113.** Four configuration pages under `guides/config/`:

1. `01-providers.md` — add your own provider: the five kinds, base URLs, the key
   (encrypted, shown once), Test, models discovered, rotate, remove. Local model
   included (keyless, free, cloud must reach it).
2. `02-models-and-tier-pairs.md` — what "expensive" and "cheap" mean here, the
   built-in pairs (`gpt-4o → gpt-4o-mini`, `deepseek-v4-pro → deepseek-flash`), your
   own pairs, what makes a pair inactive.
3. `03-output-checker.md` — a JSON schema contract: what is checked, the replay on a
   frontier model, what the change does to existing calls, the per-step `checker`
   trigger, and how to confirm from the trace.
4. `04-auto-router-and-threshold.md` — the heuristic router: what scores, the
   threshold, when a downgrade saves nothing, how to confirm from the trace.

**Acceptance Criteria**
1. Every page names the exact control (dashboard page, field label) and links to the
   troubleshooting page for the error it can cause.
2. Each page was walked on the live host: the provider, tier pair, checker and reason
   text quoted match what the UI shows today.
3. A local (keyless) provider is demonstrated end to end, or the page states plainly
   that this requires a reachable endpoint and shows the CI stack instead.
4. `python3 scripts/check-guides.py` passes; `Last verified` carries the date.
5. Live on the durable host.

**Source:** ITEM-113 breakdown, 2026-11-05.
