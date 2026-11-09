# Releasing

The repository is a Cargo workspace of three crates under `crates/`:

| Crate | Version group | Changelog |
|---|---|---|
| `llm-provider` | `llm-provider` | `crates/llm-provider/CHANGELOG.md` |
| `llm-provider-macros` (the `#[tool]` and `#[tools]` macros) | `llm-provider` | listed under `llm-provider` |
| `llm-provider-tools` (MCP servers as tools) | its own | `crates/llm-provider-tools/CHANGELOG.md` |

`llm-provider` and `llm-provider-macros` are released together under one
version: `release-plz.toml` puts both in one version group, and
`llm-provider` depends on the macros at that version. `llm-provider-tools`
is versioned on its own and depends on `llm-provider` by path and version.

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
   whether the change breaks, per crate.
5. **release-plz opens the release pull request.** Merging it tags each
   released crate (`<crate>-vX.Y.Z`) and creates the GitHub releases.

## Publishing to crates.io (not yet taken)

The crates are not on crates.io yet. Before the first publish:

- [ ] Set `publish = true` in the three crates' `Cargo.toml`. The order is
      `llm-provider-macros`, then `llm-provider`, then `llm-provider-tools`,
      each depending on the one before.
- [ ] Point the install lines in the READMEs at crates.io.
- [ ] Add the `CARGO_REGISTRY_TOKEN` secret to the repository.
- [ ] Remove `git_only` from `release-plz.toml`.
- [ ] Make the repository public.
- [ ] Enable private vulnerability reporting (GitHub offers it only on public
      repositories) and point the README's Security section at it alone.
- [ ] Add the `macos-latest` leg to the `check` job in `ci.yml`.
- [ ] Turn on `#![warn(missing_docs)]` in `llm-provider` (111 public items
      lack docs today) and document them.
- [ ] Measure the MSRV and set `rust-version` in `[workspace.package]` to it.
- [ ] Check again that the names `llm-provider`, `llm-provider-macros` and
      `llm-provider-tools` are free on crates.io.

## Account setup (manual, once)

- Install the Renovate GitHub app on the account, so `renovate.json` takes
  effect.
