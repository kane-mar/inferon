-- The platform's upstream key is for the platform's **own** tenant and
-- operator-seeded tenants only — not a pass-through for human-owned tenants
-- (PO 2026-10-10). Self-serve tenants are keyless and bring their own provider.
--
-- Clear any platform key inherited by a tenant that has a human owner/member.
-- The platform's own tenant (no membership rows) keeps its key.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

UPDATE tenants t
SET upstream_openai_key = NULL
WHERE t.upstream_openai_key IS NOT NULL
  AND EXISTS (
      SELECT 1 FROM tenant_members tm WHERE tm.tenant_id = t.id
  );
