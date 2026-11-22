# Releasing

Read the table, do the steps in order, merge one pull request. The changelog on
crates.io is the hand-written one: release-plz only renames `## [Unreleased]`
to `## [X.Y.Z] - date` and adds the compare link.

The repository is a Cargo workspace of three crates under `crates/`:

| Crate | Version group | Changelog |
|---|---|---|
| `llm-provider` | `llm-provider` | `crates/llm-provider/CHANGELOG.md` |
| `llm-provider-macros` (the `#[tool]` and `#[tools]` macros) | `llm-provider` | listed under `llm-provider` |
| `llm-provider-tools` (MCP servers as tools) | `llm-provider` | `crates/llm-provider-tools/CHANGELOG.md` |

All three share one version: `release-plz.toml` puts them in one version
group, `llm-provider` depends on the macros at that version, and
`llm-provider-tools` re-exposes core's traits, so a breaking core release is a
breaking tools release.

## Playbook

| # | Step | Who | Done when |
|---|---|---|---|
| 1 | Version: release-plz proposes it; `cargo semver-checks` (CI job `semver`) says whether a break raises the minor. 0.x: a break raises the minor, anything else the patch | CI | `semver` green on `main` |
| 2 | Checks on `main`: `ci.yml` (including the dry-run publish and the macOS leg), `scripts/security.sh`, `scripts/release-check.sh` | CI + agent | all green |
| 3 | Release notes: one wording pass over `[Unreleased]` in each changelog. One line per user-visible change, a before/after row per break, nothing internal; an **After the update** section when a user must act | agent drafts, owner reads | owner's yes |
| 4 | Drift pass: for each `[Unreleased]` line, the names it touches say the same in README, rustdoc and examples; a table in the receipt | agent | no open row |
| 5 | Steps 3 and 4 go into one "release prep" pull request. Then release-plz refreshes its release pull request. The owner reads its diff (versions, the heading, the compare link, nothing else) and merges it: that publishes and tags | **owner** | crates.io shows the version |
| 6 | From outside: `scripts/outside-check.sh <version>`, docs.rs built, the crates.io page shows the README | agent | all green; a failure is a patch release |
| 7 | Tell users: release-plz creates the GitHub release; for a minor version the agent drafts the post | owner posts | - |

Release pull requests opened with `GITHUB_TOKEN` get no CI run, so step 2 is
read on `main`, not on the release pull request. Before the merge,
`scripts/outside-check.sh --path .` proves the same from a checkout.

A source change needs a line in its crate's changelog: CI fails a pull request
that changes `crates/*/src` and no `crates/*/CHANGELOG.md`, unless the pull
request carries the label `no-changelog` (`scripts/changelog-check.sh`).

Rollback: never move a tag. `cargo yank` the version and publish a patch.

Owner only: the merge that publishes, the token, a yank, a post.

## The scripts

| Script | What it checks |
|---|---|
| `scripts/security.sh` | cargo-deny, osv-scanner, gitleaks, semgrep; CI runs the same script |
| `scripts/release-check.sh` | every `llm-provider = "x.y"` in READMEs, rustdoc and examples equals the crates' minor |
| `scripts/outside-check.sh <version>` or `--path <repo>` | a fresh project adds each crate, the README's first program compiles; with a version, docs.rs built all three |
| `scripts/changelog-check.sh <base> <head>` | the changelog rule above; `LABELS` holds the pull request's labels |
| `tests/scripts/scripts.test.sh` | tests for the two checks above that CI runs |

## Publishing to crates.io

release-plz publishes: merging its release pull request publishes each
released crate to crates.io (order: `llm-provider-macros`, `llm-provider`,
`llm-provider-tools`) and tags it (`<crate>-vX.Y.Z`). Nothing is published from
a feature pull request. Before the first publish:

- [ ] Make the repository public.
- [ ] Add the `CARGO_REGISTRY_TOKEN` secret to the repository.
- [ ] Enable private vulnerability reporting (GitHub offers it only on public
      repositories); SECURITY.md points at it alone.
- [ ] Check again that the names `llm-provider`, `llm-provider-macros` and
      `llm-provider-tools` are free on crates.io.

Until a crate is on crates.io, release-plz links the heading to the release
tag; from the second release on it links the compare view.

## Account setup (manual, once)

- Install the Renovate GitHub app on the account, so `renovate.json` takes
  effect.
