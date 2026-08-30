# Inferon Decision: Toolchain Scope Clarification (2026-08-30)

> Source: Recorded product decision by Kane Mar via pi session, 2026-08-30
> Collected: 2026-08-30
> Published: 2026-08-30

Clarification to the toolchain decision (2026-08-30-inferon-toolchain-decision.md): "no Python, no Ruby" applies **strictly to the Inferon codebase** — proxy, workers, tooling, and scripts we own. Using other tools that may run Python is fine.

Resolved: the open question about Slice 3 fine-tuning is settled — ephemeral ML training workers running Unsloth/PyTorch (e.g., Modal/RunPod jobs) are external tooling and may use Python. Inferon-owned code (proxy, dataset export, orchestration, dashboards, scripts) remains Rust and bash.
