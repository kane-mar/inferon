-- ITEM-094: cross-surface translation.
--
-- `translate_surfaces` lets a request reach a provider that speaks the other
-- surface (an Anthropic SDK to DeepSeek, an OpenAI SDK to Claude). It is on by
-- default because the previous behaviour was a typed refusal — enabling it can
-- only turn an error into a working call — and the switch restores that refusal.
--
-- Idempotent: re-applied on every deploy (scripts/migrate.sh).

ALTER TABLE tenant_configurations
    ADD COLUMN IF NOT EXISTS translate_surfaces BOOLEAN NOT NULL DEFAULT TRUE;
