# Product Backlog

## Product Goal

Inferon is live at `https://inferon.agilefederation.com/v1` with true cost numbers and automatic cost protection (Slice 1 + 2 delivered). The next horizon: put it in front of **real external developers**, make the dashboard and alerting trustworthy enough for them to stay, then walk the autonomy roadmap — schema guard, then OmniRouting — with LoRA distillation deliberately parked last.

Delivered: Slice 1 (ingress + tenant auth + telemetry + cost-leak dashboard — ITEM-001..007), Slice 2 (heuristic router + budget circuit breaker — ITEM-015/016) plus ITEM-008..014, 017, 019, 020, 023–027, 034–042 — all archived (see `.backlog/ARCHIVED/`). The data plane is real and live; the **tenant-facing control plane is not** — a tenant cannot bring their own provider, choose models, set fallback rules, or configure routing/budget from the UI, and the onboarding flow does not yet back the landing promise. Those gaps are captured as ITEM-043..048 and rank **above** putting the service in front of real external developers.

Project decisions that shape this backlog (see `wiki/llm-inference/inferon-project-direction.md`): Docker everywhere (deployment unit = image), Postgres for telemetry/control plane, working name **inferon**, default AI **DeepSeek**, toolchain **Rust + bash only** (no Python, no Ruby — SQL migrations unaffected).

## Backlog (single-ranked — small, fine-grained work at the top; larger, chunkier work below)

| ID | Title | Type | Status |
|----|-------|------|--------|
| ITEM-043 | Provider provisioning (bring your own provider) | feature | **mostly done (2026-10-06)** — encrypted provider creds + proxy resolution + `/dashboard/providers`; remaining: "test connection" |
| ITEM-044 | Model catalog & per-tenant selection | feature | **done (2026-10-06)** — catalog + `tenant_models` selection + `/dashboard/models`; disabled models refused (typed 400) and never routed to |
| ITEM-045 | Fallback & routing rules | feature | **done (2026-10-06)** — ordered per-tenant fallback chain on retryable failures (429/5xx/transport), traced + configured at `/dashboard/routing`; per-step triggers + guard trigger deferred |
| ITEM-047 | Dashboard — provider/model/key spend & limits | improvement | **new** — depends on 043/044 |
| ITEM-048 | SDK onboarding flow ("connect your SDK") | feature | **mostly done (2026-10-06)** — `/dashboard/onboarding`: mint key → snippets → first-request detection; provider/model links await 043/044 |
| ITEM-046 | Routing policy & budget controls (UI) | feature | **done (2026-10-06)** — `/dashboard/routing` controls: budget cap + cheap fallback, guard toggle, router toggle + threshold, live spend-vs-cap at a glance; owner-only writes, applied on the next request (no redeploy). Audit trail deferred |
| ITEM-049 | Own the app shell — retire the template, one operator console | tech-debt | **in progress** — backend now renders no HTML (Rust dashboard/console + signup screen retired); remaining: replace the template-shaped Next shell |
| ITEM-018 | First real developer onboarding | improvement | **PO call** — hand the staged key to a real developer; now sits *after* the control plane (043–048) |
| ITEM-029 | OmniRouting: task-centroid matching | feature | **staged** — with 028; needs real task traffic for centroids + the quality gate (see item file) |
| ITEM-030 | OmniRouting: Lagrangian budget solver | feature | **staged** — with 028/029 (consumes the 029 quality-gap signal; see item file) |
| ITEM-031 | OmniRouting: continuous evolution loop | feature | **staged** — unpark = backlog order (Slice 3, PO 2026-09-05) |
| ITEM-032 | OmniRouting: multi-tenant PagedLoRA serving | feature | **staged** — unpark = backlog order; GPU economics err to most performant model (PO 2026-09-05) |

_Last updated: 2026-10-06 (ITEM-046 landed — control plane 043–046 + 048 done)_ — PO
review found the data plane far ahead of the tenant-facing configuration: no
provider provisioning, no model selection, no fallback rules, no routing/budget
UI, and a dashboard that shows numbers without anything to act on. Captured as
**ITEM-043..048** (ranked above ITEM-018), plus **ITEM-049** (own the app shell /
retire the template / one operator console). Platform state: host live over
HTTPS with CI/CD, GitHub OAuth live, accounts + web app + FastEmbedder archived.
The OmniRouting cluster (029–032) remains staged behind the control plane.
