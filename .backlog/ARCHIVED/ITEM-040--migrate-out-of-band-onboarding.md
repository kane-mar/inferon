## ITEM-040: Migrate out-of-band onboarding — close the ITEM-018 handoff path (Accounts · from ITEM-033)

**As a** product owner
**I want** self-serve to be the only human onboarding path
**So that** "sign in and get a key" is the product, not the PO handing out keys

**Acceptance Criteria:**
1. README developer docs: self-serve sign-in is the only documented path for
   humans; `seed-tenant.sh` is documented as operator/machine-only (scripting,
   CI, test fixtures) and its human-key-minting copy is removed
2. The ITEM-018 staged-key handoff closes: any developer named by the PO uses
   self-serve; the already-staged second-real-developer key remains valid as a
   normal key (ITEM-034 rows) — no data migration needed, just docs/runbook
   changes and an explicit close-out note in ITEM-018
3. Ops runbooks updated: the operator console (ITEM-037) covers tenant
   inspection; operator JSON APIs + key stay documented for machines
4. Full smoke passes with zero changes to `/v1/chat` semantics (logins never
   touch the machine-key hot path — the ITEM-035 design principle)

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 7 (completion slice)
- Depends on ITEM-034..039
- Consumes the ITEM-018 residual: its out-of-band minting path (AC1/AC2 "how a
  tenant key is issued") is superseded by self-serve; ITEM-018 itself stays
  open only for its PO-gated human handoff + AC5 capture loop (see ITEM-018)

## Verification record (2026-09-07)

- **AC1 (docs)**: README developer onboarding is self-serve only (sign in →
  key once → SDK example); the seed-tenant section is framed operator/machine
  only and its script header now says so explicitly (scripting, CI fixtures,
  operators without GitHub).
- **AC2 (ITEM-018 close-out)**: an explicit close-out note in ITEM-018 records
  that the out-of-band human path is closed and self-serve is the only human
  onboarding; the staged second-real-developer key remains valid as an
  ordinary api_keys row (no data migration). What remains on ITEM-018 is the
  PO's call (hand the staged key to a named developer or let them self-serve).
- **AC3 (runbooks)**: README operator section leads with the browser console
  (/v1/dashboard/operator, OPERATOR_GITHUB_LOGINS) for humans and documents the
  operator JSON APIs + OPERATOR_API_KEY for machines.
- **AC4 (hot path untouched)**: no /v1/chat code changed across ITEM-034..040 —
  logins/sessions/members/keys never touch the machine-key proxy path (the
  ITEM-035 design principle). Full smoke (65 checks) + 184 tests green.
- Accounts horizon complete: ITEM-034..040 delivered + archived. No code
  changes this slice beyond docs/runbooks — no redeploy needed.
