# Releasing

The repository is a Cargo workspace of three crates under `crates/`:

| Crate | Version group | Changelog |
|---|---|---|
| `llm-provider` | `llm-provider` | `crates/llm-provider/CHANGELOG.md` |
| `llm-provider-macros` (the `#[tool]` and `#[tools]` macros) | `llm-provider` | listed under `llm-provider` |
| `llm-provider-tools` (MCP servers as tools) | `llm-provider` | `crates/llm-provider-tools/CHANGELOG.md` |

`llm-provider` and `llm-provider-macros` are released together under one
version: `release-plz.toml` puts both in one version group, and
`llm-provider` depends on the macros at that version. `llm-provider-tools`
is in the same group: it re-exposes core's traits, so a breaking core
release is a breaking tools release. It depends on `llm-provider` by path
and version.

## Playbook

Every release follows these steps, in order.

1. **The pipeline is green on `main`.** Check the latest `ci.yml` run on
   `main`. Release pull requests opened with `GITHUB_TOKEN` get no CI run, so
   CI is checked on `main`, not on the release pull request.
2. **`scripts/security.sh` passes locally.** It is the one definition of the
   security scans; CI runs the same script.
3. **Each released crate's `CHANGELOG.md` has the release's section**, with
   the version and the date.
4. **The version follows 0.x semver.** A breaking change raises the minor
   version, anything else the patch version. `cargo semver-checks` says
   whether the change breaks, per crate. It does not see a public dependency
   moving to a new minor: a breaking `llm-provider` release is a breaking
   `llm-provider-tools` release, which the version group enforces.
5. **release-plz opens the release pull request.** Merging it publishes each
   released crate to crates.io, tags it (`<crate>-vX.Y.Z`) and creates the
   GitHub releases.

## Publishing to crates.io

release-plz publishes: merging its release pull request publishes each
released crate to crates.io (order: `llm-provider-macros`, `llm-provider`,
`llm-provider-tools`) and tags it. Nothing is published from a feature
pull request. Before the first publish:

- [ ] Make the repository public.
- [ ] Add the `CARGO_REGISTRY_TOKEN` secret to the repository.
- [ ] Enable private vulnerability reporting (GitHub offers it only on public
      repositories); SECURITY.md points at it alone.
- [ ] Add the `macos-latest` leg to the `check` job in `ci.yml`.
- [ ] Check the release pull request's CHANGELOG diff (`[Unreleased]` becomes
      `[0.3.0] - date`) before merging it.
- [ ] Check again that the names `llm-provider`, `llm-provider-macros` and
      `llm-provider-tools` are free on crates.io.

## Account setup (manual, once)

- Install the Renovate GitHub app on the account, so `renovate.json` takes
  effect.
