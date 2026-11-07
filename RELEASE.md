# Releasing

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

- [ ] Set `publish = true` in `Cargo.toml`.
- [ ] Add the `CARGO_REGISTRY_TOKEN` secret to the repository.
- [ ] Remove `git_only` from `release-plz.toml`.
- [ ] Make the repository public.
- [ ] Enable private vulnerability reporting (GitHub offers it only on public
      repositories) and point the README's Security section at it alone.
- [ ] Add the `macos-latest` leg to the `check` job in `ci.yml`.
- [ ] Turn on `#![warn(missing_docs)]` (111 public items lack docs today) and
      document them.
- [ ] Measure the MSRV and set `rust-version` to it.
- [ ] Check again that the name `llm-provider` is free on crates.io.

## Account setup (manual, once)

- Install the Renovate GitHub app on the account, so `renovate.json` takes
  effect.
