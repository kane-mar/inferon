# Inferon Vertical Slices — Detailed Technical Specification & Execution Architecture

> Source: Pasted by user (Kane Mar); no public URL
> Collected: 2026-08-30
> Published: Unknown

Detailed technical specification and execution architecture for each of the 5 vertical slices of the Inferon platform.

## Slice 1: The Transparent Analytics & Cost Linter Proxy

The objective is to establish the zero-friction ingress pipeline and provide immediate cost visibility. The proxy must achieve zero-copy stream forwarding to guarantee it adds <1ms of latency to upstream API calls.

- Ingress Layer (Rust / Axum):
  - Architecture: Use axum combined with reqwest for the upstream client.
  - Stream Handling: LLM completions use Server-Sent Events (SSE). To avoid buffering the response in memory (which breaks TTFT—Time-To-First-Token), the proxy maps the incoming Axum Body into a reqwest::Body::wrap_stream. The upstream response bytes are then mapped directly back into the Axum response as text/event-stream.
  - Authentication: The proxy extracts the Authorization: Bearer <API_KEY> header, validates it against a local cache (or Redis), and swaps it with Inferon's master upstream API key for OpenAI/Anthropic.
- Asynchronous Telemetry Pipeline:
  - Message Passing: Upon request completion, the proxy sends a lightweight metadata struct (Tenant ID, upstream URL, token counts from the final [DONE] chunk, and total duration) via a tokio::sync::mpsc channel.
  - Storage: A dedicated Tokio background worker reads the channel and performs batch inserts into a PostgreSQL traces table every 500ms. This ensures database I/O never blocks the hot HTTP path.
- Cost Linter Engine:
  - The background worker runs heuristic checks on the saved payload: e.g., scanning for repeating 4,000+ token system prompts without provider cache headers, or formatting simple unstructured text using gpt-4o instead of gpt-4o-mini.

## Slice 2: Heuristic Model Router & Budget Circuit Breaker

The proxy shifts from passive observation to active mutation. It intercepts the request body, alters the routing destination based on static rules, and manages atomic budget state.

- Request Interception & Mutation:
  - Instead of purely streaming the incoming body, the proxy reads the incoming JSON payload into a serde_json::Value struct.
  - Complexity Heuristics: The Rust router evaluates:
    1. Does the payload contain tools or functions arrays?
    2. Is temperature > 0.8 (indicating creative generation rather than strict extraction)?
    3. Is the calculated token length < 500?
  - If the heuristic scores low on complexity, the proxy rewrites the payload: payload["model"] = "gpt-4o-mini" (or Anthropic claude-3-haiku), updates the upstream destination URL, and forwards it.
- Budget Circuit Breaker:
  - State Management: Maintain an atomic counter in Redis (HINCRBY tenant:{id}:spend {cost_in_cents}).
  - Enforcement: Before forwarding, the proxy checks if the tenant's current spend exceeds their defined monthly limit. If exceeded, the proxy either rejects the request with HTTP 429 or forcefully overrides the model parameter to the cheapest available open-source model.

## Slice 3: 1-Click Assisted LoRA Distillation & Serving

This slice introduces the offline training pipeline and dynamic LoRA serving cluster, proving the end-to-end distillation loop.

- Dataset Generation Pipeline (Python):
  - A Python worker queries the PostgreSQL traces table for a specific system_prompt_hash where status = 200.
  - It formats the prompt/response pairs into the standard OpenAI conversational JSONL format.
- Fine-Tuning Worker (Modal / Unsloth):
  - Triggered via a webhook, an ephemeral GPU instance (e.g., 1x A10G on Modal) spins up.
  - The script uses unsloth.FastLanguageModel to load meta-llama/Llama-3.2-3B-Instruct.
  - Training Config: 4-bit QLoRA, r=16, lora_alpha=16, using the SFTTrainer from Hugging Face trl.
  - Upon completion (typically under 10 minutes for 1,000 samples), the worker pushes the adapter_model.safetensors artifact to an AWS S3 bucket.
- Dynamic Serving (vLLM):
  - A persistent vLLM cluster runs on a base model (e.g., Llama-3.2-3B) started with the --enable-lora and --max-loras 100 parameters.
  - Hot-Swapping: Inferon issues a POST /v1/load_lora_adapter request to the vLLM API, pointing to the new S3 bucket path. vLLM dynamically loads the weights into GPU memory without restarting the server.
  - The Rust proxy updates its tenant routing table: all traffic for that specific task is now routed to the vLLM cluster with the parameter model: "tenant_abc_lora_id".

## Slice 4: Self-Healing Fallback & Schema Integrity Guard

Small models (3B-8B) carry a higher risk of hallucinating schema boundaries. This slice builds the deterministic safety net.

- Streaming Output Interceptor:
  - The Rust proxy no longer blindly pipes bytes back to the client. For requests routed to the distilled vLLM model, it buffers the incoming SSE chunks into a temporary memory arena.
- AST / Schema Validation:
  - Once the [DONE] token is received from vLLM, the proxy parses the accumulated string against the JSON Schema provided in the original request's response_format parameter.
- Transparent Fallback Circuit:
  - Success Path: If validation passes, the proxy flushes the buffered SSE chunks to the client. (This introduces a slight TTFT delay on the distilled model, but guarantees safety).
  - Failure Path: If the JSON parsing fails or validation is violated, the proxy silently discards the bad response. It immediately opens a new connection to the frontier fallback model (e.g., Claude 3.5 Sonnet).
  - The proxy streams the frontier model's response back to the client. The client experiences a latency spike on that specific request, but receives a 100% valid response.
  - The failed 3B output is flagged and stored in the database as a "negative sample" for the next DPO (Direct Preference Optimization) fine-tuning run.

## Slice 5: Autonomous Optimization & OmniRouting

The final slice introduces the mathematical routing fabric, shifting from static heuristics to dynamic budget optimization.

- Sub-Millisecond Prompt Classification (Rust / ONNX):
  - The Rust proxy embeds the ort (ONNX Runtime) crate.
  - It loads a highly quantized embedding model (like BGE-micro).
  - Incoming prompts are vectorized in <3ms. The router uses Cosine Similarity against known task centroids in memory to identify the workload type and assign it to a specific LoRA adapter.
- Lagrangian Budget Solver:
  - The router formulates the assignment as a constrained optimization problem. It predicts the expected quality Q for both the frontier model and the distilled model.
  - max_{m in M} E[Q(m, x)] subject to sum_{i=1..N} Cost(m_i, x_i) <= B_remaining
  - If the budget B_remaining is high early in the month, the proxy's threshold epsilon tightens, routing more traffic to the frontier model to maximize quality. As the budget depletes, epsilon loosens, aggressively routing acceptable traffic to the zero-cost distilled adapters to prevent a hard service cutoff.
- Autonomous Evolution Loop:
  - A Temporal.io background workflow continuously monitors the fallback rate (from Slice 4).
  - If a specific LoRA adapter's fallback rate exceeds 5% (indicating concept drift in production), Temporal automatically queues a new Unsloth training job using the newly harvested traces, tests the new adapter against a held-out gold set, and issues the vLLM load_lora_adapter command to seamlessly upgrade the endpoint.
