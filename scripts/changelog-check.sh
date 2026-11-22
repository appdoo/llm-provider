#!/usr/bin/env bash
# CI rule: a change under crates/<name>/src needs a line in crates/<name>/CHANGELOG.md,
# unless the pull request carries the label no-changelog.
# Usage: LABELS="a,b" scripts/changelog-check.sh <base> <head>
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: changelog-check.sh <base> <head>" >&2
  exit 2
fi
base=$1
head=$2

case ",${LABELS:-}," in
  *,no-changelog,*)
    echo "changelog-check.sh: label no-changelog, skipped"
    exit 0
    ;;
esac

changed=$(git diff --name-only "$base" "$head")
missing=0
for crate in $(printf '%s\n' "$changed" | sed -n 's#^crates/\([^/]*\)/src/.*#\1#p' | sort -u); do
  if ! printf '%s\n' "$changed" | grep -qx "crates/$crate/CHANGELOG.md"; then
    echo "changelog-check.sh: $crate changes src but not crates/$crate/CHANGELOG.md (add a line, or label the pull request no-changelog)" >&2
    missing=1
  fi
done
exit "$missing"
