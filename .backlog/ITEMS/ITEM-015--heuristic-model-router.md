## ITEM-015: Heuristic Model Router (Slice 2)

**As a** developer on the proxy
**I want** simple requests automatically routed to the cheapest adequate model
**So that** my spend drops without me changing a line of SDK code

**Acceptance Criteria:**
1. Complexity classification: tools/functions present (+0.50), response_format json (+0.30), content chars > 8000 (+0.40) or > 3000 (+0.20), code/JSON-spec patterns (+0.30 / +0.25), capped at 1.0
2. Downgrade rule: complexity < 0.35 and model is a gpt-4o-tier full model → rewrite `payload["model"]` to `gpt-4o-mini` and forward the modified bytes
3. Non-downgraded requests forward byte-identical (no mutation)
4. The rewritten model is what lands in the trace (`request_model` reflects the actual upstream model)
5. Hot-path budget stays < 2ms added (Slice 2 latency target); classification is pure Rust over the already-parsed payload

**Technical Notes:**
- Source: engineering spec Slice 2 ComplexityClassifier + downgrade rule (see `wiki/llm-inference/inferon-slice-engineering-spec.md`); budget breaker is separate (ITEM-016)
- Reuses the parsed `payload` already available in `proxy_chat_completions`; mutates `payload` before it becomes `raw_prompt` in the trace
