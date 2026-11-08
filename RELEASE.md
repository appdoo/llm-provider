# Releasing

The repository is a Cargo workspace of two crates, `llm-provider` and
`llm-provider-macros` (the `#[tool]` and `#[tools]` macros). They are
released together under one version: `release-plz.toml` puts both in one
version group, and `llm-provider` depends on the macros at that version.
Every step below covers both crates.

## Playbook

Every release follows these steps, in order.

1. **The pipeline is green on `main`.** Check the latest `ci.yml` run on
   `main`. Release pull requests opened with `GITHUB_TOKEN` get no CI run, so
   CI is checked on `main`, not on the release pull request.
2. **`scripts/security.sh` passes locally.** It is the one definition of the
   security scans; CI runs the same script.
3. **`CHANGELOG.md` has the release's section**, with the version and the
   date.
4. **The version follows 0.x semver.** A breaking change raises the minor
   version, anything else the patch version. `cargo semver-checks` says
   whether the change breaks.
5. **release-plz opens the release pull request.** Merging it tags `vX.Y.Z`
   and creates the GitHub release.

## Publishing to crates.io (not yet taken)

The crate is not on crates.io yet. Before the first publish:

- [ ] Set `publish = true` in `Cargo.toml` and `llm-provider-macros/Cargo.toml`;
      the macros publish first, since `llm-provider` depends on them.
- [ ] Add the `CARGO_REGISTRY_TOKEN` secret to the repository.
- [ ] Remove `git_only` from `release-plz.toml`.
- [ ] Make the repository public.
- [ ] Enable private vulnerability reporting (GitHub offers it only on public
      repositories) and point the README's Security section at it alone.
- [ ] Add the `macos-latest` leg to the `check` job in `ci.yml`.
- [ ] Turn on `#![warn(missing_docs)]` (111 public items lack docs today) and
      document them.
- [ ] Measure the MSRV and set `rust-version` to it.
- [ ] Check again that the names `llm-provider` and `llm-provider-macros` are
      free on crates.io.

## Account setup (manual, once)

- Install the Renovate GitHub app on the account, so `renovate.json` takes
  effect.
