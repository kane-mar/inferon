# Technical Specification: Autonomous Inference Optimizer (AIO-Proxy)

> Source: Pasted by user (Kane Mar); no public URL
> Collected: 2026-08-30
> Published: Unknown

A zero-SDK, drop-in proxy (baseURL override) providing Automated Passive LoRA Distillation coupled with Global Budget-Constrained Multi-Model Routing.

## 1. High-Level System Architecture

The system operates via three asynchronous planes:

1. **Data Plane (Hot Path)**: Sub-15ms proxy routing requests between frontier providers, self-hosted base models, and auto-distilled LoRA adapters.
2. **Telemetry & Dataset Plane (Warm Path)**: Asynchronous log ingestion, heuristic scoring, synthetic validation pair generation, and dataset clustering.
3. **Training & Distillation Plane (Cold Path)**: Autonomous fine-tuning pipelines (LoRA/QLoRA on small open-weight models), offline benchmarking, and zero-downtime adapter hot-swapping.

```
                                 [ Client Application ]
                                           |
                                           | (POST /v1/chat/completions)
                                           v
+----------------------------------------------------------------------------------------+
| DATA PLANE (Hot Path Ingress)                                                          |
|                                                                                        |
|  +----------------------------------------------------------------------------------+  |
|  | 1. Zero-Friction Proxy Ingress (Rust / Axum)                                     |  |
|  |    * Auth & Tenant Context Extraction (API Keys, Organization ID)                |  |
|  |    * Schema Validation & Token Counting                                         |  |
|  +---------------------------------------+------------------------------------------+  |
|                                          |                                             |
|                                          v                                             |
|  +----------------------------------------------------------------------------------+  |
|  | 2. Budget-Constrained Router (OmniRouter Optimization Core)                       |  |
|  |    * Complexity Classifier: Embeds prompt via fast bi-encoder (<3ms)              |  |
|  |    * Dual-Head Scoring: Predicts Quality Gap d(q_frontier - q_distilled)         |  |
|  |    * Lagrangian Solver: Evaluates real-time token budget vs. latency SLA         |  |
|  +-------------------+--------------------------------------+-----------------------+  |
|                      |                                      |                          |
|         [Route: Low Complexity]                [Route: High Complexity]                |
|                      v                                      v                          |
|  +------------------------------------------+    +----------------------------------+  |
|  | 3. Distilled Model Cluster               |    | 4. Frontier Upstream Providers   |  |
|  |    * vLLM / SGLang Engine Cluster        |    |    * Anthropic (Claude 3.5 Sonnet)|  |
|  |    * Dynamic Multi-LoRA Serving Pool     |    |    * OpenAI (GPT-4o)             |  |
|  |    * Quantized 1B-8B Base Engines        |    |    * Google (Gemini Pro)         |  |
|  +-------------------+----------------------+    +------------------+----------------+  |
|                      |                                          |                      |
|                      +-------------------+----------------------+                      |
|                                          |                                             |
|                                          v                                             |
|  +----------------------------------------------------------------------------------+  |
|  | 5. Response Streaming & Shadow Sampling Hook                                     |  |
|  |    * Streams tokens back to client instantly                                     |  |
|  |    * Asynchronously pushes payload trace to Kafka / Redpanda Buffer              |  |
|  +----------------------------------------------------------------------------------+  |
+------------------------------------------+---------------------------------------------+
                                           |
                                           v (Asynchronous Trace Stream)
+----------------------------------------------------------------------------------------+
| TELEMETRY & DISTILLATION CONTROL PLANE (Warm & Cold Paths)                             |
|                                                                                        |
|  +-------------------------------------+    +---------------------------------------+  |
|  | 6. Trace Sanitation & Quality Filter|    | 7. Automated LoRA Training Pipeline   |  |
|  |    * Filters: 200 OK, regex checks  |    |    * Auto-clusters prompts by task    |  |
|  |    * De-duplicates via MinHash LSH  | --> |    * Triggers unsloth/Axolotl run     |  |
|  |    * Synthetic edge-case generation |    |    * Evaluates vs. Frontier Gold Set  |  |
|  +-------------------------------------+    +-------------------+-------------------+  |
|                                                                 |                      |
|                                                                 v                      |
|                                             +---------------------------------------+  |
|                                             | 8. Adapter Registry & Hot Deployment  |  |
|                                             |    * Uploads weights to S3/MinIO      |  |
|                                             |    * Registers new LoRA with vLLM     |  |
|                                             |    * Updates Router cost/quality graph|  |
|                                             +---------------------------------------+  |
+----------------------------------------------------------------------------------------+
```

## 2. Core Subsystems

### Subsystem A: Ingress & Classification (Hot Path)

- **Ingress Runtime**: Rust (Axum/Tokio) or Go (FastHTTP) proxy acting as a drop-in replacement for https://api.openai.com/v1.
- **Prompt Featurization**:
  - Extracts structural metadata: system prompt hash, message count, tool definitions, token length.
  - Injects a compact embedding using a quantized bi-encoder (e.g., BGE-micro / ModernBERT-base running on ONNX runtime on CPU) in under 3ms.
- **Routing Decision Function**:
  The router solves the constrained knapsack problem for each query x:

  ```
  max_{m in M} E[Q(m, x)]   subject to   sum_{i=1..N} Cost(m_i, x_i) <= B_period,   Latency(m, x) <= L_max
  ```

  - Evaluates the Quality Gap dq = Qhat_frontier(x) - Qhat_distilled(x).
  - If dq <= epsilon_threshold (where epsilon is dynamically adjusted by the remaining budget B_period via Lagrangian multipliers), dispatch to the local distilled model; otherwise, route to the frontier model.

### Subsystem B: Dynamic LoRA Serving Cluster

- **Inference Engine**: Multi-GPU cluster managed by vLLM or SGLang with dynamic multi-LoRA loading enabled.
- **Base Models**: Standardized base checkpoints (e.g., Llama-3.2-3B-Instruct, Qwen-2.5-7B-Instruct).
- **Multi-Tenancy & Zero-Cold-Start Serving**:
  - Base model weights stay pinned in GPU HBM.
  - Tenant-specific LoRA adapters (10MB-50MB) are paged in dynamically via copy-on-write RAM-to-VRAM loading using custom memory pooling.

### Subsystem C: Telemetry, Dataset Curation & Distillation Engine

- **Event Ingestion**: All requests and completions stream to a Redpanda / Kafka event log.
- **Autonomous Curation Pipeline**:
  1. Noise Filtering: Discards requests with client errors, truncated generation stops, or low downstream feedback scores.
  2. Deduplication & Clustering: Runs MinHash LSH and k-means clustering over prompt embeddings to isolate distinct workload types (e.g., JSON extraction, SQL generation, creative copywriting).
  3. Synthetic Pair Augmentation: For thin clusters, generates edge-case variations using the frontier model to enforce schema boundaries and edge-case resilience.
- **Automated Training Orchestrator (Temporal.io + Unsloth)**:
  1. Once a cluster reaches N >= 1,000 high-quality verified traces, triggers an ephemeral cloud GPU worker (e.g., 1x L4 or 1x A10G).
  2. Runs QLoRA fine-tuning (Rank 16-64, Alpha 32) using cross-entropy loss against frontier model outputs.
  3. Executes an automated eval suite (100 held-out golden traces):
     - JSON Schema Validity: Must reach 100%.
     - Semantic Cosine Similarity / Exact Match: Must reach >= 98% parity with frontier output.
  4. Upon passing verification, pushes the adapter artifact to S3 and registers the LoRA ID in the proxy's active routing table.

## 3. Data Schema & Control Plane Specifications

### 3.1 Organization Configuration Schema (PostgreSQL)

```sql
CREATE TABLE tenant_configurations (
    tenant_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_hash VARCHAR(64) NOT NULL UNIQUE,
    monthly_budget_usd NUMERIC(10, 2) NOT NULL DEFAULT 500.00,
    current_spend_usd NUMERIC(10, 2) NOT NULL DEFAULT 0.00,
    routing_strategy VARCHAR(32) NOT NULL DEFAULT 'balanced_cost', -- 'max_quality', 'balanced_cost', 'hard_budget'
    min_quality_threshold NUMERIC(3, 2) NOT NULL DEFAULT 0.95,
    max_p95_latency_ms INT NOT NULL DEFAULT 2000,
    frontier_fallback_model VARCHAR(64) NOT NULL DEFAULT 'claude-3-5-sonnet-20241022',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE distilled_adapters (
    adapter_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID REFERENCES tenant_configurations(tenant_id),
    task_signature VARCHAR(64) NOT NULL, -- SHA256 of system prompt + tool schemas
    base_model VARCHAR(64) NOT NULL,    -- e.g., 'meta-llama/Llama-3.2-3B-Instruct'
    adapter_s3_uri TEXT NOT NULL,
    sample_count INT NOT NULL,
    benchmark_parity_score NUMERIC(4, 3) NOT NULL, -- e.g., 0.985
    is_active BOOLEAN NOT NULL DEFAULT FALSE,
    deployed_at TIMESTAMP WITH TIME ZONE
);
```

### 3.2 Ingress Request Routing Pipeline (Rust Dataflow)

```rust
pub struct RoutingDecisionContext {
    pub tenant_id: Uuid,
    pub prompt_tokens: usize,
    pub system_prompt_hash: String,
    pub embedding_vector: Vec<f32>,
    pub remaining_budget_usd: f64,
}

pub enum RouteTarget {
    FrontierProvider {
        provider_name: String,
        model_name: String,
    },
    DistilledLoRA {
        base_model_endpoint: String,
        adapter_id: String,
    },
}

pub async fn determine_route(
    ctx: &RoutingDecisionContext,
    router: &OmniRouterCore,
    registry: &AdapterRegistry,
) -> RouteTarget {
    // 1. Check if an active, validated LoRA adapter exists for this task cluster
    if let Some(adapter) = registry.get_active_adapter(&ctx.tenant_id, &ctx.system_prompt_hash) {
        // 2. Predict quality gap on this specific prompt vector
        let predicted_quality_gap = router.predict_quality_gap(&ctx.embedding_vector, &adapter.adapter_id);

        // 3. Evaluate dynamic Lagrangian acceptance threshold based on remaining budget
        let dynamic_threshold = router.get_current_epsilon_threshold(ctx.remaining_budget_usd);

        if predicted_quality_gap <= dynamic_threshold {
            return RouteTarget::DistilledLoRA {
                base_model_endpoint: adapter.base_model_cluster_url,
                adapter_id: adapter.adapter_id.to_string(),
            };
        }
    }

    // Default fallback to frontier provider
    RouteTarget::FrontierProvider {
        provider_name: "anthropic".to_string(),
        model_name: "claude-3-5-sonnet-20241022".to_string(),
    }
}
```

## 4. End-to-End Execution Sequence

1. **Ingress & Traffic Interception**: Client updates their standard SDK configuration. The client changes baseURL to https://api.aioproxy.com/v1 in their OpenAI/Anthropic SDK. Every standard request passes through the Axum proxy.

2. **Dynamic Decision & Routing**: Sub-5ms evaluation of cost, latency, and expected quality gap. The proxy checks the system prompt hash against the tenant's adapter registry. If an adapter exists, it runs a lightweight complexity regression. Requests within quality tolerance are routed to the internal vLLM cluster with the specific adapter_id; complex or novel edge cases pass to the configured frontier API.

3. **Response Streaming & Telemetry Logging**: Asynchronous trace capture with zero added latency. The upstream token stream is piped directly to the client via Server-Sent Events (SSE). Concurrently, a complete event record (input payload, system prompt, output tokens, time-to-first-token, completion status) is pushed to the Redpanda trace queue.

4. **Background Distillation Pipeline**: Data sanitation and automated fine-tuning loop. The background worker pulls traces from the queue, clusters them by semantic schema, and checks if a task cluster has crossed the training threshold (N >= 1,000 samples). An ephemeral worker runs LoRA distillation on an open-weight base model, benchmarks against the frontier responses, and outputs an adapter artifact.

5. **Hot-Swap & Continuous Optimization**: Zero-downtime deployment and dynamic threshold adjustment. The newly minted LoRA is registered in the vLLM multi-LoRA pool. The proxy updates its internal routing table and begins routing a small percentage (canary release) of matching traffic to the new adapter, gradually increasing the traffic share as real-time quality verification scores remain high.

## 5. Failure Modes & Mitigations

| Failure Mode | Root Cause | System Mitigation Strategy |
|--------------|------------|----------------------------|
| Model Drift / Schema Corruption | Distilled model hallucinates on novel inputs | Real-time output schema validation. If JSON parsing fails on a distilled response, automatically re-run the request against the frontier model and downweight the LoRA routing probability. |
| Budget Depletion Early in Billing Cycle | Sudden traffic spike | The router's Lagrangian multiplier dynamically shifts epsilon_threshold, aggressively routing higher-complexity queries to the small model to prevent hard service cutoffs. |
| Cold Start on New Workload Types | Customer introduces a completely new prompt template | 100% of traffic routes to the frontier model initially while the dataset plane clusters traces and queues the initial distillation job. |
| vLLM GPU Out-of-Memory (OOM) | Too many concurrent active LoRA adapters | PagedLoRA memory manager unloads inactive adapters from VRAM to Host DRAM using Least Recently Used (LRU) eviction. |

## 6. Commercial MVP Rollout Checklist

1. **Week 1-2 (Proxy Core)**: Deploy Rust/Axum proxy handling transparent OpenAI/Anthropic streaming with zero latency penalty; stream all completions to Redpanda/PostgreSQL.
2. **Week 3-4 (Distillation Pipeline)**: Build the automated dataset formatting and Unsloth training script executing on an on-demand GPU instance.
3. **Week 5-6 (Multi-LoRA Serving)**: Deploy a single 4x L4 GPU node running vLLM with dynamic multi-LoRA routing.
4. **Week 7-8 (OmniRouter Logic)**: Implement prompt embedding classification and budget-constrained routing policies in the proxy core; launch developer console showing cost savings.
