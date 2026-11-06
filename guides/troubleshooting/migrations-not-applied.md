# Migrations did not apply after an upgrade

A feature is missing, a page errors, or the proxy logs a database error after a redeploy.

## Symptom

Any of these, always right after an upgrade:

- a dashboard page returns `500` with a `dashboard_error`;
- the proxy log shows `column … does not exist` or a decode error;
- a setting you saved does not appear anywhere.

## What it means

The Postgres entrypoint only runs `db/init/*.sql` on a **fresh volume**. Every later
migration has to be applied explicitly — `./scripts/migrate.sh` does that, and it re-applies
*all* files in order because each one is written to be idempotent. Nothing tracks which have
run; the schema is the record.

So a deploy that rebuilt the image but skipped the migration step leaves new code against an
old schema. The usual mistake is `docker compose up -d --build` with no `migrate.sh` before
it.

## Fix

1. **Run the migrations** from the project root on the host:
   `./scripts/migrate.sh` — it waits for Postgres, applies every file in order and prints
   how many it applied.
2. **Then restart what reads the schema**: the proxy (`docker compose --profile prod up -d
   --no-build proxy`). Migrations are separate from the image.
3. **Check the schema version** by looking for the newest object a release expects, e.g.
   `docker exec inferon-postgres-1 psql -U inferon -d inferon -c '\d config_audit'`.
4. **If a migration fails**, read its error before retrying: a failed `ALTER` in the middle
   of the list leaves a partially applied schema, and re-running is safe only because each
   step is idempotent.

## How to confirm it worked

- `migrate.sh` exits `0` and reports the file count.
- The page or endpoint that failed returns `200`.
- The proxy log has no further decode errors after the restart.

## When to stop and ask

- `migrate.sh` fails on the same statement twice.
- A column exists and the proxy still complains about it — the proxy may be running an older
  image than you think (`docker compose ps` shows the container's age).
- Data is missing that no migration deletes: that is a restore, not a migration.

_Last verified 2026-11-06 against the live host (master acdaffc)._
