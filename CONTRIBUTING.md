# Contributing

Issues and pull requests are welcome. For a larger change, open an issue first.

Before you open a pull request, these must pass (they are the commands of `.github/workflows/ci.yml`):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features
scripts/security.sh
```

Rules of the code: a new public item has a doc comment (`missing_docs` is on); a behaviour change has a test; a breaking change has a CHANGELOG line with a before/after table. The crate reads no environment variable and sends no request you did not ask for.

By contributing you agree that your work is licensed under the MIT license of this repository. Be kind: see the [Code of Conduct](CODE_OF_CONDUCT.md).
