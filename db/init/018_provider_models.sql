-- ITEM-044 rework (PO 2026-10-07): model selection belongs to a **provider**,
-- not the tenant. `tenant_models` becomes provider-scoped — one row per
-- (provider, model). Models are the curated catalog for the provider's kind, so
-- an `openai-compatible` provider (no catalog) has no selectable rows and stays
-- unrestricted.
--
-- Idempotent: every migration is re-applied on each deploy (scripts/migrate.sh).

ALTER TABLE tenant_models ADD COLUMN IF NOT EXISTS provider_id UUID;

-- Backfill: attach each existing tenant-scoped selection to the tenant's first
-- provider (the one the proxy resolves). Guarded so a re-apply after
-- `tenant_id` is dropped is a no-op (the statement would not parse otherwise).
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'tenant_models' AND column_name = 'tenant_id'
    ) THEN
        EXECUTE $sql$
            UPDATE tenant_models tm
            SET provider_id = (
                SELECT p.id FROM providers p
                WHERE p.tenant_id = tm.tenant_id
                ORDER BY p.created_at
                LIMIT 1
            )
            WHERE tm.provider_id IS NULL
        $sql$;
    END IF;
END $$;

-- Selections with no provider to attach to are meaningless: shared-key tenants
-- keep the unrestricted catalog defaults.
DELETE FROM tenant_models WHERE provider_id IS NULL;

-- Replace the (tenant_id, model_id) primary key with (provider_id, model_id).
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'tenant_models_pkey'
          AND pg_get_constraintdef(oid) NOT LIKE '%provider_id%'
    ) THEN
        ALTER TABLE tenant_models DROP CONSTRAINT tenant_models_pkey;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'tenant_models_pkey') THEN
        ALTER TABLE tenant_models
            ADD CONSTRAINT tenant_models_pkey PRIMARY KEY (provider_id, model_id);
    END IF;
END $$;

ALTER TABLE tenant_models DROP COLUMN IF EXISTS tenant_id;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'tenant_models_provider_fk') THEN
        ALTER TABLE tenant_models
            ADD CONSTRAINT tenant_models_provider_fk
            FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE;
    END IF;
END $$;
