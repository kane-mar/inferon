## ITEM-135: Gateway overhead counts the client's upload

**As** the operator reading the Runtime page
**I want** the gateway's latency to measure the gateway
**So that** a slow prompt upload is not mistaken for our own slowness, and a real
regression cannot hide behind it

**Reported** 2026-11-08 (PO): "the gateway overhead is waaaaay out of whack. 7.41
seconds is much too slow."

**What was actually happening.** `proxy_us` was measured from the moment the
request line arrived, which includes **the time the client spends sending its
body**. The workspace's prompts are ~285k tokens ≈ 1.2 MB, and a consumer uplink
takes 1.5–2 s to push that; a stutter takes it to 7.7 s. The proxy's own work on
exactly that request is 8 ms.

Measured on the live host, same payloads, same container:

| payload | `body` stage over the internet | `body` stage via loopback | work after the body |
|---|---|---|---|
| 4 KB | 4.9 ms | 32 µs | 1.1 ms |
| 1.2 MB | 1.75–1.94 s | 6.8 ms | 8.3–8.5 ms |
| 5 MB | 3.0–3.4 s | 20 ms | 23 ms |

The loopback column is the gateway; the internet column is the client's network.
The stage breakdown (`auth`, `body`, `policy`, `route`, `budget`, `chain`, `prune`)
named `body` immediately — it was 99.6% of the reported figure, which is why this
was found in minutes rather than guessed at. Enabling `RUST_LOG=inferon=debug`
temporarily is what produced that evidence.

**Delivered**

- Both ingress surfaces (`/v1/chat/completions`, `/v1/messages`) start the
  gateway's clock when the body is complete.
- `traces.body_ms` (migration 055) records request start → body complete: the
  client's upload, kept because a slow sender is worth seeing, never folded into
  the gateway's number.
- The Runtime API returns `body_p50_ms` / `body_p95_ms`, the p95 KPI prints
  "clients took a median N ms to send the body (not counted above)", and the
  contract definition and claim note now say the measurement starts when the body
  has arrived — in en and fr.
- README's performance contract states the boundary and that rows written before
  2026-11-08 still include the upload, so nobody reads a trend across the line.

**Acceptance Criteria**

1. A request whose body arrives slowly records the upload on `body_ms` and a small
   `proxy_us`. ✔ (test: body in two pieces 300 ms apart → `body_ms ≥ 300`,
   `proxy_us < 100 ms`; red on the previous clock)
2. Every place that states the measurement states the boundary. ✔ README, KPI
   definition, claim note, `overhead_help`, en and fr.
3. Nothing that was genuinely fast becomes slow. ✔ the loopback numbers are
   unchanged (1.1 ms at 4 KB, 8.5 ms at 1.2 MB).

**Not done, and why**

- **The remaining per-request cost was not optimised.** Pre-dispatch is 1.1 ms at
  4 KB and 8.3–8.5 ms at 1.2 MB; the biggest components are the JSON parse of the
  body (6.8 ms at 1.2 MB) and `chain` (0.75–8.4 ms, a `serves` lookup per fallback
  model on a cold route cache). Both are real but small, and they are honest work
  now that the number measures them. If they need to fall, the next step is a
  batched model→provider lookup for the whole chain, not a bigger clock change.
- **The p95 spikes had two causes.** Upload jitter (fixed by this) and CPU
  contention on the 1 vCPU host from the 1.2 MB JSONB trace writes. The second is
  still there; it shows as `policy`/`chain` spikes (8 ms) on cold cache, not as a
  7 s number.

**Source:** PO report, 2026-11-08.
