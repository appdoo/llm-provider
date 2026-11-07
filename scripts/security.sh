#!/usr/bin/env bash
# The one definition of the security scans. Run from the repository root.
set -euo pipefail

fail() {
  echo "security.sh: $1 failed" >&2
  exit 1
}

cargo deny --all-features check || fail cargo-deny
osv-scanner scan source --lockfile Cargo.lock || fail osv-scanner
{ gitleaks git --redact . && gitleaks dir --redact .; } || fail gitleaks
semgrep scan --config p/rust --config p/secrets --error --metrics=off src tests || fail semgrep

echo "security.sh: all checks passed"
