## ITEM-076: OmniRouting — adapter training, gold-set gate and hot swap

**As a** product owner
**I want** a queued drift job to train, pass a held-out gold set, and hot-swap
the adapter
**So that** the evolution loop closes without a human

**Status:** **staged** — gated on the Slice 3 infra (S3 adapter weights, vLLM
cluster, DPO dataset shape from ITEM-026). Split from ITEM-031 on 2026-10-15 when
its detection/queue half shipped.

**Acceptance Criteria**
1. A training run takes a `queued` job from `evolution_jobs` and consumes the
   harvested negative samples (ITEM-026) to produce a candidate adapter.
2. The candidate must pass a held-out gold set before promotion; a failure marks
   the job `failed` with the evidence.
3. A passing candidate is issued via `load_lora_adapter` and the endpoint is
   upgraded without dropping in-flight requests.
4. Every transition (training → testing → promoting → promoted) is written to the
   job row so the operator view shows it.
