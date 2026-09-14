#!/usr/bin/env bash
# Scan reachable repository history without emitting candidate secret material.
# This syntax is intentionally compatible with the locally supported gitleaks
# release; CI uses the maintained action for its equivalent history scan.
set -euo pipefail

if ! command -v gitleaks >/dev/null 2>&1; then
    printf '%s\n' 'secret scan requires gitleaks to be installed' >&2
    exit 1
fi

scan_output="$(gitleaks --repo-path . --redact 2>&1)" || {
    printf '%s\n' "$scan_output" >&2
    exit 1
}
printf '%s\n' "$scan_output"

# Older gitleaks releases report findings successfully at process level; make
# the no-findings assertion explicit so local and CI evidence fail closed.
if ! rg -q '0 leaks detected\.' <<<"$scan_output"; then
    printf '%s\n' 'secret scan found one or more potential leaks' >&2
    exit 1
fi
