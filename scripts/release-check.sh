#!/usr/bin/env bash
# Two checks before a release; exit 0, or 1 with what is wrong.
# 1. Every `llm-provider* = "x.y"` (or `{ version = "x.y"`) in READMEs, rustdoc and
#    examples equals the minor of the crates (the file:line list).
# 2. The guide: every chapter file in crates/llm-provider/guide/ is included by
#    src/guide.rs, every chapter module is linked from README.md, and every guide
#    link in the READMEs (root, crate, examples) names a chapter module.
# Usage: scripts/release-check.sh [root]   (default: the repository root)
set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$root"

version=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\)\.[0-9]*.*/\1/p' crates/llm-provider/Cargo.toml | head -1)
if [ -z "$version" ]; then
  echo "release-check.sh: no version in crates/llm-provider/Cargo.toml" >&2
  exit 2
fi
guide=crates/llm-provider/src/guide.rs
if [ ! -f "$guide" ]; then
  echo "release-check.sh: no $guide" >&2
  exit 2
fi
failed=0

# --- 1. versions
dep='llm-provider(-tools|-macros)? = (\{ version = )?'
ok="${dep}\"${version//./\\.}(\\.[0-9]+)?\""
stale=$(grep -rnE "${dep}\"[0-9]+\.[0-9]+" --include='*.md' --include='*.rs' README.md crates \
  | grep -v '/CHANGELOG.md:' | grep -vE "$ok" || true)
if [ -n "$stale" ]; then
  echo "release-check.sh: lines that do not say $version:" >&2
  echo "$stale" >&2
  failed=1
fi

# --- 2. the guide
# A chapter file the module does not include is a page docs.rs never shows.
for file in crates/llm-provider/guide/*.md; do
  [ -e "$file" ] || continue
  if ! grep -qF "include_str!(\"../guide/$(basename "$file")\")" "$guide"; then
    echo "release-check.sh: chapter file not in the guide module: $file" >&2
    failed=1
  fi
done

modules=$(sed -n 's/^pub mod \(ch[0-9A-Za-z_]*\) {}.*/\1/p' "$guide")

# A chapter the README never links is a chapter nobody finds.
for module in $modules; do
  if ! grep -qF "guide/$module/" README.md; then
    echo "release-check.sh: guide chapter not linked from README.md: $module" >&2
    failed=1
  fi
done

# A link to a chapter that does not exist is a 404 on docs.rs.
for readme in README.md crates/llm-provider/README.md crates/llm-provider/examples/README.md; do
  [ -f "$readme" ] || continue
  for link in $(grep -oE 'guide/ch[0-9A-Za-z_]*' "$readme" | sort -u || true); do
    if ! printf '%s\n' $modules | grep -qx "${link#guide/}"; then
      echo "release-check.sh: $readme links a missing chapter: $link" >&2
      failed=1
    fi
  done
done

[ "$failed" -eq 0 ] || exit 1
echo "release-check.sh: all versions say $version; every guide chapter is included and linked"
