## ITEM-155: Rotate the secrets that were printed

**Requested** 2026-11-12: "yes" (start the rotation, beginning with the three that can
be done end to end).

**Where the values went.** They were printed by `pgrep -fa "docker compose"` on the
deploy host, because the deploy carried the environment on the ssh command line. So
they reached: this session's transcript, and the host's process list for the length of
a deploy (root-only, single-tenant). **Not** the GitHub Actions logs (the runner masks
them), **not** the repository or its history, **not** the docs site.

**Rotated and verified (2 of 5)**

| Secret | State |
|---|---|
| `SESSION_SECRET` | new 64-hex value on the host; a session minted with it answers 200, one minted with a wrong secret is refused (307 to sign-in) |
| `OPERATOR_API_KEY` | new `sk-op-…` on the host and in `~/.cache/inferon-durable/operator_key`; accepted 200, a wrong key 401 |

Both were generated **on the host** and written into `env.local` with mode 600 in
place; no value was printed, and the local operator key was updated by reading the
file rather than echoing it.

**The trap, stated in the handout.** CI still holds the **old** values in its Actions
secrets, and CI rewrites `env.local` on every deploy. So the next tag release would put
the leaked `SESSION_SECRET` and `OPERATOR_API_KEY` back. GitHub secrets must be updated
**before** the next tag — hence the order in `SECURITY.md`: CI first, then the host.

**Blocked on the owner's accounts**

- `GITHUB_CLIENT_SECRET`: regenerate on the OAuth app, then the host and the
  `OAUTH_CLIENT_SECRET` repo secret.
- `UPSTREAM_MASTER_KEY`: the shared DeepSeek platform key — revoke and reissue at the
  provider, then the host and the repo secret.
- A `0600` handout at `~/.cache/inferon-rotation/HANDOUT.txt` holds the two rotated
  values for pasting into GitHub, with the steps and the warning. It is not in git.

**Not rotated, deliberately**

- `PROVIDER_ENCRYPTION_KEY`. Rotating it makes every stored provider credential
  unreadable, because the ciphertext was sealed with the old key: each workspace's
  provider has to be re-saved (or a re-encryption migration written) in the same
  window. That is a planned change with a maintenance note, not a reaction to a leak
  whose blast radius is a local transcript. Recorded in `SECURITY.md` with its cost.

**Why the GitHub side could not be automated**: Actions secrets must be sealed with
libsodium, and this machine has neither PyNaCl nor `gh`, so the API path is closed.
The handout exists for that reason.

**Also added**: `SECURITY.md` — how to report a vulnerability, what the repository does
not contain, the five secrets and what each costs to rotate, the delivery rule (0600
file, recreated every deploy, never argv), and the recovery trick (a running container
is a copy of its environment).

**Source:** PO request, 2026-11-12.
