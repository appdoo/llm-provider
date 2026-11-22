#!/usr/bin/env bash
# Tests L2-L4 of Spec 64 for scripts/changelog-check.sh and scripts/release-check.sh,
# on fixtures built in a temporary git repository. Run from anywhere.
set -euo pipefail

scripts=$(cd "$(dirname "$0")/../../scripts" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

# expect <name> <expected exit> <command...>: runs it, compares the exit code.
expect() {
  local name=$1 want=$2 got=0
  shift 2
  "$@" >"$work/out" 2>&1 || got=$?
  if [ "$got" -ne "$want" ]; then
    echo "FAIL $name: exit $got, want $want"
    cat "$work/out"
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
}

# contains <name> <text>: the last command's output names it.
contains() {
  if grep -q -- "$2" "$work/out"; then echo "ok   $1"; else
    echo "FAIL $1: output lacks '$2'"
    cat "$work/out"
    failures=$((failures + 1))
  fi
}

# --- L2, L3: changelog-check.sh on two fixture ranges
repo="$work/repo"
git init --quiet "$repo"
cd "$repo"
git config user.email t@example.invalid
git config user.name t
mkdir -p crates/llm-provider/src
echo 'fn a() {}' >crates/llm-provider/src/lib.rs
echo '# Changelog' >crates/llm-provider/CHANGELOG.md
git add -A
git commit --quiet -m base
base=$(git rev-parse HEAD)

echo 'fn b() {}' >>crates/llm-provider/src/lib.rs
git commit --quiet -am src-only
src_only=$(git rev-parse HEAD)

echo '- a line' >>crates/llm-provider/CHANGELOG.md
echo 'fn c() {}' >>crates/llm-provider/src/lib.rs
git commit --quiet -am src-and-changelog
both=$(git rev-parse HEAD)

expect "L2 src without changelog fails" 1 env LABELS= "$scripts/changelog-check.sh" "$base" "$src_only"
contains "L2 names the crate" "llm-provider changes src"
expect "L2 src with changelog passes" 0 env LABELS= "$scripts/changelog-check.sh" "$base" "$both"
expect "L3 label no-changelog passes" 0 env LABELS=docs,no-changelog "$scripts/changelog-check.sh" "$base" "$src_only"

# --- L4: release-check.sh
fx="$work/fx"
mkdir -p "$fx/crates/llm-provider/src"
printf '[package]\nname = "llm-provider"\nversion = "0.3.0"\n' >"$fx/crates/llm-provider/Cargo.toml"
printf '```toml\nllm-provider = "0.2"\n```\n' >"$fx/README.md"
expect "L4 stale minor fails" 1 "$scripts/release-check.sh" "$fx"
contains "L4 lists file:line" "README.md:2:"
printf '```toml\nllm-provider = "0.3"\n```\n' >"$fx/README.md"
expect "L4 fixed passes" 0 "$scripts/release-check.sh" "$fx"

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
echo "all script tests passed"
