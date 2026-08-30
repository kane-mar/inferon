# Multi-LoRA Serving Cluster

> Sources: User-provided technical specification, 2026-08-30
> Raw: [aio-proxy-technical-specification.md](../../raw/llm-inference/aio-proxy-technical-specification.md); [inferon-slice-engineering-spec.md](../../raw/llm-inference/inferon-slice-engineering-spec.md)
> Updated: 2026-08-30

## Overview

The distilled model cluster is a multi-GPU inference farm that serves many tenant-specific LoRA adapters over a small set of pinned base models, with zero cold starts: adapter weights are paged in and out of VRAM dynamically while base weights stay resident.

## Engine & Base Models

- **Inference Engine**: vLLM or SGLang with dynamic multi-LoRA loading enabled.
- **Base Models**: standardized checkpoints, e.g., Llama-3.2-3B-Instruct and Qwen-2.5-7B-Instruct; the cluster runs quantized 1B-8B base engines.

## Multi-Tenancy & Zero-Cold-Start Serving

- Base model weights stay **pinned in GPU HBM**.
- Tenant-specific LoRA adapters (10MB-50MB each) are paged in dynamically via **copy-on-write RAM-to-VRAM loading** using custom memory pooling.

## Memory Management & OOM Mitigation

- **PagedLoRA memory manager**: when too many concurrent active adapters threaten GPU OOM, inactive adapters are unloaded from VRAM to Host DRAM using **Least Recently Used (LRU) eviction**.

## Deployment Footprint (MVP)

- A single **4x L4 GPU node** running vLLM with dynamic multi-LoRA routing (Week 5-6 of the rollout).

## Serving Implementation (Detailed Spec)

Concrete mechanics from the [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md):

- A persistent vLLM cluster runs on a base model (e.g., Llama-3.2-3B) started with **--enable-lora** and **--max-loras 100**.
- **Hot-swapping**: Inferon issues a POST /v1/load_lora_adapter request to the vLLM API pointing at the new S3 bucket path; vLLM dynamically loads the weights into GPU memory without restarting the server.
- The Rust proxy then routes all traffic for that task to the vLLM cluster with the parameter model: "tenant_abc_lora_id".
- **Adapter naming (vllm_client.rs)**: lora_name = `{tenant_id}_{task_hash[..8]}`; the load request is POST `{vllm_admin_url}/v1/load_lora_adapter` with JSON body {lora_name, lora_path}.

## See Also

- [Passive LoRA Distillation](passive-lora-distillation.md)
- [Budget-Constrained Routing](budget-constrained-routing.md)
- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
- [Inferon Slice Engineering Specification](inferon-slice-engineering-spec.md)
