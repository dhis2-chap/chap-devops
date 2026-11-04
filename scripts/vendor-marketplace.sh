#!/usr/bin/env bash
# Refresh the vendored marketplace snapshot in vendor/marketplace/.
#
# Downloads registry.yaml and every model file it lists from the upstream
# chap-marketplace repository, verbatim, and records the upstream commit in
# vendor/marketplace/SNAPSHOT. No fetch date: a run against an unchanged
# upstream must leave the directory byte for byte as it was, or the weekly
# workflow opens a pull request for nothing.
set -euo pipefail

REPO="${MARKETPLACE_REPO:-dhis2-chap/chap-marketplace}"
REF="${MARKETPLACE_REF:-main}"
RAW="https://raw.githubusercontent.com/${REPO}/${REF}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="${root}/vendor/marketplace"

mkdir -p "${dest}/models"

echo "fetching ${RAW}/registry.yaml"
curl -sSfL "${RAW}/registry.yaml" -o "${dest}/registry.yaml"

# Model paths are the `- models/<id>.yaml` entries under the `models:` key.
paths="$(sed -n 's/^[[:space:]]*-[[:space:]]*\(models\/[^[:space:]]*\.ya\?ml\)[[:space:]]*$/\1/p' "${dest}/registry.yaml")"

if [ -z "${paths}" ]; then
  echo "no model entries found in registry.yaml" >&2
  exit 1
fi

# Drop model files that upstream no longer lists.
find "${dest}/models" -type f -name '*.yaml' -delete

for path in ${paths}; do
  echo "fetching ${RAW}/${path}"
  curl -sSfL "${RAW}/${path}" -o "${dest}/${path}"
done

sha=""
if command -v jq >/dev/null 2>&1; then
  sha="$(curl -sSfL "https://api.github.com/repos/${REPO}/commits/${REF}" | jq -r '.sha // empty')"
fi

{
  echo "CHAP model marketplace snapshot"
  echo "==============================="
  echo
  echo "upstream:       https://github.com/${REPO}"
  echo "ref:            ${REF}"
  if [ -n "${sha}" ]; then
    echo "commit:         ${sha}"
  fi
  echo
  echo "Files in this directory are verbatim copies of the upstream YAML. build.rs"
  echo "generates the table src/registry/embedded.rs compiles into the binary, which"
  echo "is the last resort fallback when the registry cannot be fetched or read from"
  echo "cache. There is no hand-written file list to keep in step."
  echo
  echo "Refresh with scripts/vendor-marketplace.sh and commit the result."
} > "${dest}/SNAPSHOT"

echo
echo "snapshot written to ${dest}"
echo "build.rs picks the new set up on the next build; run 'cargo test registry' to"
echo "check it, then commit vendor/marketplace/"
