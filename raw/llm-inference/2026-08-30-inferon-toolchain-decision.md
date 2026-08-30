# Inferon Decision: Toolchain — No Python, No Ruby; Rust + Bash Only (2026-08-30)

> Source: Recorded product decision by Kane Mar via pi session, 2026-08-30
> Collected: 2026-08-30
> Published: 2026-08-30

Decision: the Inferon codebase is **Rust and bash only**. No Python, no Ruby — for the proxy, workers, tooling, and scripts. SQL (for Postgres migrations) is unaffected.

Implications:
- Slice 1 items (Docker scaffold, schema, ingress, auth, telemetry, linter, dashboard) are unaffected — all Rust/bash.
- The implementation spec's Slice 3 Python scripts (scripts/export_dataset.py, train_lora.py with Unsloth/trl) are source-documented but must be reimplemented in Rust/bash when Slice 3 is built (dataset export via Rust/sqlx; training orchestration via bash).
- **Open question**: QLoRA fine-tuning via Unsloth/PyTorch is inherently Python-based tooling. Whether "no python" covers ephemeral ML training workers (vs. the Inferon codebase itself) is unresolved — flag before Slice 3 work starts.
