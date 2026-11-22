#!/usr/bin/env bash
# Check the crates from outside, like a stranger would use them: a fresh project,
# the crate added, the README's first program as main.rs, cargo check.
# Usage: scripts/outside-check.sh <version>      after a publish, from crates.io; then docs.rs
#        scripts/outside-check.sh --path <repo>  before a publish, from a checkout
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: outside-check.sh <version> | --path <repo>" >&2
  exit 2
fi

here=$(cd "$(dirname "$0")/.." && pwd)
mode=version
if [ "$1" = "--path" ]; then
  mode=path
  repo=$(cd "${2:?--path needs a directory}" && pwd)
else
  version=$1
  repo=$here
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# first_program <README>: the first ```rust block, without the fence lines.
first_program() {
  awk '/^```rust/ { if (!done) { on = 1; next } } /^```/ { if (on) { on = 0; done = 1 } } on' "$1"
}

# check <crate> <README> <cargo add features...>
check() {
  local crate=$1 readme=$2
  shift 2
  local dir="$work/$crate-check"
  (cd "$work" && cargo new --quiet --bin "$crate-check")
  cd "$dir"
  if [ "$mode" = path ]; then
    cargo add --quiet "$crate" --path "$repo/crates/$crate" "$@"
  else
    cargo add --quiet "$crate@=$version" "$@"
  fi
  cargo add --quiet tokio --features full
  if [ "$crate" = llm-provider-tools ]; then
    # The tools README's program names llm-provider too, as its Install section says.
    if [ "$mode" = path ]; then
      cargo add --quiet llm-provider --path "$repo/crates/llm-provider" --features agent
    else
      cargo add --quiet "llm-provider@=$version" --features agent
    fi
  fi
  first_program "$readme" > src/main.rs
  if [ ! -s src/main.rs ]; then
    echo "outside-check.sh: no rust block in $readme" >&2
    exit 1
  fi
  cargo check --quiet
  echo "outside-check.sh: $crate ok"
  cd "$work"
}

check llm-provider "$repo/README.md" --features agent
check llm-provider-tools "$repo/crates/llm-provider-tools/README.md"

if [ "$mode" = version ]; then
  for crate in llm-provider llm-provider-macros llm-provider-tools; do
    status=$(curl -fsSL "https://docs.rs/crate/$crate/$version/status.json" || echo '{}')
    case "$status" in
      *'"doc_status":true'*) echo "outside-check.sh: docs.rs built $crate $version" ;;
      *)
        echo "outside-check.sh: docs.rs has no green build of $crate $version yet: $status" >&2
        exit 1
        ;;
    esac
  done
fi
echo "outside-check.sh: all checks passed"
