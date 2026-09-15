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

# Port contracts are the infrastructure-facing API. Identity and category text
# must be admitted through validated DTO parsing, never passed as raw text.
if awk '/^pub trait /,/^}/' crates/penelope-ports/src/implementation.rs | grep -En '\bString\b|&str' >/dev/null; then
    printf '%s\n' 'layer violation: a Penelope port accepts raw text; use a validated newtype or DTO' >&2
    exit 1
fi

# Versioned protocol records must not expose raw textual state. The explicit
# identifier parsing constructors in the domain crate are the sole text entry
# boundary; once parsed, public fields and tuple payloads remain typed values.
if grep -REn '^\s*pub\s+[[:alnum:]_]+\s*:\s*(String|&str)\b|^\s*pub\s+struct\s+[[:alnum:]_]+[^\{]*\(\s*pub\s+(String|&str)\b' \
    crates --include='*.rs' >/dev/null; then
    printf '%s\n' 'layer violation: a public Penelope protocol value exposes raw text; use a validated newtype or enum' >&2
    exit 1
fi

# Library failures are typed `thiserror` enums. Handwritten Error impls make
# error taxonomy audits and source chaining inconsistent across crates.
if grep -REn 'impl\s+(std::error::)?Error\s+for' crates --include='*.rs' >/dev/null; then
    printf '%s\n' 'layer violation: handwritten Error implementation found; derive thiserror::Error instead' >&2
    exit 1
fi

# Error taxonomies must remain typed `thiserror` enums. Looking at the small
# attribute window keeps this source-level gate independent of fragile error
# text, while still catching a newly added hand-rolled error enum in any crate,
# example, or benchmark.
missing_thiserror_derive="$(
    find crates -type f -name '*.rs' -print | while IFS= read -r source; do
        awk '
            /^#\[derive\(/ {
                derive = $0
                next
            }
            /^(pub )?enum [[:alnum:]_]*Error[[:space:]]*\{/ {
                if (derive !~ /derive\([^)]*Error/) {
                    printf "%s:%d: error enum does not derive thiserror::Error\\n", FILENAME, NR
                }
                derive = ""
            }
            /^[[:space:]]*$/ { next }
            { derive = "" }
        ' "$source"
    done
)"
if [[ -n "$missing_thiserror_derive" ]]; then
    printf '%s\n' "$missing_thiserror_derive" >&2
    exit 1
fi
