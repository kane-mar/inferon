## ITEM-153: The deploy carried every secret on the command line

**Found** 2026-11-11, while investigating why a CI deploy had been rebuilding the host
for over an hour. `pgrep -fa "docker compose"` on the host printed the whole deploy
command, which included:

`SESSION_SECRET=… OPERATOR_API_KEY=sk-op-… GITHUB_CLIENT_SECRET=… UPSTREAM_MASTER_KEY=sk-… PROVIDER_ENCRYPTION_KEY=…`

Both the CI workflow and `scripts/deploy-durable.sh` exported the environment on the
**ssh command line**. That means, on the host:

1. every secret was visible in `ps` to any process, for the duration of the deploy
   (and in shell history files, if any are enabled);
2. the values existed only for that one command — so when a deploy began with
   `rm -rf /opt/inferon`, the host's `env.local` was destroyed and nothing recreated
   it. The next `docker compose up` would start containers with no
   `SESSION_SECRET`, `PROVIDER_ENCRYPTION_KEY` or `UPSTREAM_MASTER_KEY`: sign-ins and
   provider decryption would fail, and the failure would look like a database
   problem.

**Fix.** Both paths now pipe the environment over **stdin** into a 0600 file, and
recreate it on every deploy (because `/opt/inferon` is re-extracted from the bundle):

```
{ echo "SESSION_SECRET=$SESSION_SECRET"; … } | ssh host "umask 077 && … \
   && cat > /opt/inferon/env.local && chmod 600 /opt/inferon/env.local \
   && set -a && . ./env.local && set +a && docker compose --profile prod up -d --build"
```

`env_file` replaces `compose_env` in the script, and the workflow uses the same shape.
A guard checks that no ssh line interpolates a secret.

**Exposure that needs the owner's decision.** While diagnosing, the values above were
printed into this session's transcript. Rotation is the only safe response:
`SESSION_SECRET` (signs every user out), `GITHUB_CLIENT_SECRET` (regenerate on the
GitHub OAuth app), `OPERATOR_API_KEY`, `UPSTREAM_MASTER_KEY`, and
`PROVIDER_ENCRYPTION_KEY` (the last one re-encrypts provider keys, so it needs a
re-save of each provider, not just a new value).

**Also found, and recovered:** the host's `env.local` had been deleted by the cancelled
deploy. It was rebuilt from the environment of the still-running proxy container
(`docker inspect`) without printing the values — which is the recovery trick worth
remembering: **a running container is a copy of the secrets.**

**Source:** CI investigation, 2026-11-11.
