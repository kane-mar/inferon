-- Inferon schema migration 007 (idempotent)
-- ITEM-035: human accounts. `users` keyed by a unique GitHub id; a successful
-- OAuth sign-in upserts (first login inserts the row, later logins refresh
-- github_login and last_login_at). Sessions are stateless signed cookies that
-- carry the user id — no server-side session table.

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    github_id BIGINT NOT NULL,
    github_login TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_github_id ON users(github_id);
