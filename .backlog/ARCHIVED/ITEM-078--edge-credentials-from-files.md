## ITEM-078: Edge credentials from files, not argv

**As an** operator running an edge node
**I want** to pass the node token and provider key as file paths
**So that** no long-lived secret appears in `ps` or shell history

**Found:** 2026-10-23 — the start command passed
`--provider-key "$(cat ~/.config/inferon/lmstudio.token)"`, so the key was
visible in the process list (same for the node token).

**Acceptance Criteria**
1. `--provider-key-file <path>` and `--node-token-file <path>` are accepted; the
   file wins over the inline flag.
2. A trailing newline in the file is trimmed.
3. A missing/unreadable file, or an empty credential, fails fast with a message
   naming both forms.
4. Neither secret ever needs to appear in argv.

## Close-out (2026-10-26)

- `--provider-key-file <path>` and `--node-token-file <path>` are accepted; the
  file wins over the inline flag, a trailing newline is trimmed, and a
  missing/unreadable file or empty credential fails fast naming both forms.
- `edge_runtime::credential` is unit-tested (inline, file-wins, unreadable,
  missing-credential message).
- **Live**: the node restarted with
  `--node-token-file ~/.config/inferon/edge-node.token --provider-key-file
  ~/.config/inferon/lmstudio.token`; `ps` shows only the paths, `/healthz` → 200,
  and a chat completion returned 200 (`content: "works"`).
- Rust **312 → 314** tests, clippy clean. Commit `f809254`; CI `40312314161`.
