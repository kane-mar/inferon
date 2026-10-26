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
