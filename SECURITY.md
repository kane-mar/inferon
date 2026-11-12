# Security

## Reporting a vulnerability

Email <kane@inferon.dev> with what you found and how to reproduce it. Do not open a
public issue for a vulnerability. We will confirm within three working days and credit
you in the fix unless you would rather not be named.

## What this repository contains, and what it does not

The code, the guides and the backlog are public. **No credential, environment file or
database dump is.** `scripts/check-guides.py` and CI both fail on a secret-shaped
string, and a deploy never carries an environment on a command line (see below).

## Operator secrets

A deployed instance holds five secrets. Each answers one question:

| Secret | What it protects | Cost of rotating |
|---|---|---|
| `SESSION_SECRET` | the signed dashboard cookie | every user signs out |
| `OPERATOR_API_KEY` | the operator console API | the previous key stops working |
| `GITHUB_CLIENT_SECRET` | the OAuth app used to sign in | none; regenerate in the app |
| `UPSTREAM_MASTER_KEY` | the shared platform provider key | the platform upstream stops answering until it is replaced |
| `PROVIDER_ENCRYPTION_KEY` | encrypts each workspace's provider credentials at rest | **the stored ciphertexts become unreadable** — every provider has to be re-saved, so rotate it deliberately, not in a hurry |

### How they are delivered

The environment is written to `/opt/inferon/env.local` with mode `600`, shipped to the
host over `scp` and installed with `install -m 600`, then read by `docker compose`.
It is recreated on **every** deploy, because the deploy re-extracts `/opt/inferon`.

It is deliberately never exported on an ssh command line. That was tried: the values
appear in the host's process list for the length of the deploy, and they vanish with
the command, so the file is destroyed by the next deploy and nothing recreates it.

### Rotating one

1. Generate the new value. Do not paste it into a shell that logs, a chat, or an issue.
2. Put it in the repository's Actions secrets **and** on the host, in that order — CI
   rewrites `env.local` on every deploy, so a value that is only on the host is
   overwritten by the next tag release.
3. Recreate the containers that read it: `docker compose --profile prod up -d proxy web`.
4. Confirm both directions: the new value is accepted, and the old one is refused.

```bash
# the shape of a check, with the value read from a file rather than typed
curl -sS -o /dev/null -w '%{http_code}\n' \
  -H "Authorization: Bearer $(cat /root/operator.key)" \
  https://your-host/v1/dashboard/api/operator/summary   # expect 200
```

A running container is a copy of its environment, which is how a lost `env.local` is
recovered: `docker inspect <container> --format '{{range .Config.Env}}{{println .}}{{end}}'`
piped straight into the file — never printed to a terminal.
