-- ITEM-043 follow-up: complete provider provisioning.
--
-- AC2 needs a "last updated" date; AC5 needs rotation (update name/base_url/key).
-- AC1 needs optional provider metadata (an OpenAI organization id + arbitrary
-- upstream headers, e.g. an `anthropic-version` for a compatible gateway).
ALTER TABLE providers
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE providers
    ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '{}'::jsonb;
