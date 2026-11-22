#!/usr/bin/env bash
# Every `llm-provider* = "x.y"` (or `{ version = "x.y"`) in READMEs, rustdoc and
# examples must equal the minor of the crates. Exit 0, or 1 with the file:line list.
# Usage: scripts/release-check.sh [root]   (default: the repository root)
set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$root"

version=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\)\.[0-9]*.*/\1/p' crates/llm-provider/Cargo.toml | head -1)
if [ -z "$version" ]; then
  echo "release-check.sh: no version in crates/llm-provider/Cargo.toml" >&2
  exit 2
fi

dep='llm-provider(-tools|-macros)? = (\{ version = )?'
ok="${dep}\"${version//./\\.}(\\.[0-9]+)?\""
stale=$(grep -rnE "${dep}\"[0-9]+\.[0-9]+" --include='*.md' --include='*.rs' README.md crates \
  | grep -v '/CHANGELOG.md:' | grep -vE "$ok" || true)

if [ -n "$stale" ]; then
  echo "release-check.sh: lines that do not say $version:" >&2
  echo "$stale" >&2
  exit 1
fi
echo "release-check.sh: all versions say $version"
