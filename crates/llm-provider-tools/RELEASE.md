# Releasing

The repository holds one crate, `llm-provider-tools`. release-plz
(`release-plz.toml`, `.github/workflows/release-plz.yml`) opens the release
pull request and tags the release.

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

The crate is not on crates.io yet. It can be published only after
`llm-provider` is. Before the first publish:

- [ ] Depend on `llm-provider` by version from crates.io instead of the
      `ssh://` git dependency in `Cargo.toml`, and drop its entry from
      `deny.toml`'s allowed git sources.
- [ ] Point the install lines in `README.md` at crates.io.
- [ ] Set `publish = true` in `Cargo.toml`.
- [ ] Add the `CARGO_REGISTRY_TOKEN` secret to the repository.
- [ ] Remove `git_only` from `release-plz.toml`.
- [ ] Remove the `LLM_PROVIDER_DEPLOY_KEY` steps from the workflows.
- [ ] Make the repository public.
- [ ] Enable private vulnerability reporting (GitHub offers it only on public
      repositories) and point the README's Security section at it alone.
- [ ] Check again that the name `llm-provider-tools` is free on crates.io.

## Account setup (manual, once)

- `llm-provider` is a private repository, fetched over SSH. Create a
  read-only deploy key on `llm-provider` and store its private half as the
  `LLM_PROVIDER_DEPLOY_KEY` secret of this repository; `ci.yml` and
  `release-plz.yml` load it with `webfactory/ssh-agent`.
- Install the Renovate GitHub app on the account, so `renovate.json` takes
  effect.
