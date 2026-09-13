#!/usr/bin/env bash
# Verifies the allowed direct internal dependency graph and rejects production
# infrastructure clients from the ports-only Penelope workspace.
set -euo pipefail

metadata="$(cargo metadata --format-version=1 --no-deps)"

require_internal_dependencies() {
    local package_name="$1"
    local expected_dependencies="$2"
    local actual_dependencies

    actual_dependencies="$(jq -r --arg package_name "$package_name" '
        .packages[]
        | select(.name == $package_name)
        | [.dependencies[] | select(.name | startswith("penelope-")) | .name]
        | sort
        | join(" ")
    ' <<<"$metadata")"

    if [[ "$actual_dependencies" != "$expected_dependencies" ]]; then
        printf 'layer violation: %s directly depends on [%s], expected [%s]\n' \
            "$package_name" "$actual_dependencies" "$expected_dependencies" >&2
        exit 1
    fi
}

require_internal_dependencies penelope-core ''
require_internal_dependencies penelope-domain 'penelope-core'
require_internal_dependencies penelope-intent 'penelope-core penelope-domain'
require_internal_dependencies penelope-ports 'penelope-domain'
require_internal_dependencies penelope-executor 'penelope-core penelope-domain penelope-intent penelope-ports'
require_internal_dependencies penelope-statechronicle 'penelope-domain penelope-ports'
require_internal_dependencies penelope 'penelope-core penelope-domain penelope-executor penelope-intent penelope-ports penelope-statechronicle'

forbidden_infrastructure_dependencies='^(aws|aws-config|aws-sdk-|axum|deadpool|diesel|lapin|mongodb|postgres|rdkafka|redis|reqwest|rusqlite|sea-orm|sqlx|surrealdb|tokio-postgres)$'
if jq -e --arg pattern "$forbidden_infrastructure_dependencies" '
    .packages[]
    | select(.name | startswith("penelope-"))
    | .dependencies[].name
    | select(test($pattern))
' <<<"$metadata" >/dev/null; then
    printf '%s\n' 'layer violation: production infrastructure dependency found in Penelope workspace' >&2
    exit 1
fi
