# Contributing to Inferon

Thanks for your interest. This project is small and the rules are short.

## Before you start

- **Open an issue first** for anything larger than a typo or a one-line fix. It
  saves you from building something that does not fit the direction of the project.
- The backlog in [`BACKLOG.md`](BACKLOG.md) shows what is planned and in what
  order. Work that is already on it does not need a new issue.
- Security problems: do not open a public issue. Email <kane@inferon.dev>.

## Licence and the CLA

Inferon is licensed under AGPL-3.0-only (see [`LICENSE`](LICENSE)). By contributing,
you agree that your contribution is licensed under the same terms.

Every contribution also needs the [CLA](CLA.md), because the project also offers a
commercial licence and that needs the owner to hold the rights to all of the code.
Add this line to your pull request:

> I have read the CLA in CLA.md and I agree to it. — Your Name, YYYY-MM-DD

The CLA does not take your copyright; it grants the project the rights it needs.
The pull request cannot merge without the statement.

## Making a change

1. Fork the repository and create a branch.
2. Write the change and the test that proves it.
3. Run the checks below.
4. Open a pull request that explains **why**, not only what.

Commit messages follow the conventional style already in the history:
`fix(pii): advance by whole characters`, `copy(web): rewrite UI text`,
`backlog: record the copy pass`.

## Checks

```bash
# Rust
cargo test --locked
cargo clippy --all-targets -- -D warnings

# Web
cd web
npm ci
npm run check:types
npx vitest run --project unit

# Licences (both must pass)
cargo deny check licenses      # needs `cargo install cargo-deny --locked` once
node scripts/check-npm-licenses.mjs
```

`cargo test` uses the default feature set. If you change the prompt embedder, also
run the gated tests described in `README.md` under "Prompt embedder".

## Adding a dependency

A new dependency needs a licence that is compatible with AGPL-3.0-only. The two
audits above will tell you if it is not. If the licence is acceptable but not yet
on an allow-list, add it to `deny.toml` (Rust) or to
`scripts/check-npm-licenses.mjs` (npm), and write the reason in
[`LICENSES.md`](LICENSES.md) in the same pull request.

## Style

- Match the code around you. Comments explain why, not what.
- Keep the proxy hot path free of avoidable work; it has a sub-millisecond overhead
  budget.
- Documentation is written at a plain reading level, and commands in it are tested
  on a real host. If you change behaviour, update the guides in `guides/`.

## Code of conduct

Be direct about the code and generous with the person. Harassment or personal
attacks are not welcome, and the maintainer may remove a contribution or a
participant for them.
