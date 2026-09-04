## ITEM-012: smoke.sh Exercises the Chat + Dashboard Surface

**As an** operator
**I want** the deployment smoke test to cover the real proxy surface
**So that** a broken dashboard route or auth regression is caught at deploy time, not by users

**Acceptance Criteria:**
1. `scripts/smoke.sh` still verifies compose + healthz (existing behavior)
2. It additionally: seeds a throwaway tenant, then asserts — chat 401 without a key, chat 200 with the seeded key (mock-free: local upstream URL is fine, or assert only the auth boundary against a dead upstream), and dashboard summary/by-model/waste return 200 with the seeded key
3. Cleanup: the throwaway tenant and any rows it created are removed on exit
4. Fails loudly with a useful message if any check fails

**Technical Notes:**
- Runs on the Docker host (deployment seam); uses the same docker-compose stack
- Auth-boundary assertions avoid needing a live upstream key: an authenticated request to an unreachable upstream still proves the 401/200 split
