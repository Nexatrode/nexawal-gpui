#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_file="${NEXAWAL_XCSTRINGS:-$repo_root/../nexawal/nexawal/Localizable.xcstrings}"
out_file="${NEXAWAL_L10N_OUTPUT:-$repo_root/assets/l10n.json}"

if [[ ! -f "$source_file" ]]; then
  echo "error: Localizable source not found: $source_file" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required" >&2
  exit 1
fi

temp_file="$(mktemp "${out_file}.XXXXXX")"
trap 'rm -f "$temp_file"' EXIT

# Retain desktop-only translations. Importing the mobile catalog must not erase
# keys added by the desktop UI. Skip obsolete Apple extraction entries.
jq --slurpfile existing "$out_file" '
  .strings
  | to_entries
  | map(select(.key != "" and .value.extractionState != "stale"))
  | reduce .[] as $item ($existing[0];
      . as $acc
      | reduce (($item.value.localizations // {}) | to_entries[] | select((.value.stringUnit.value // "") != "")) as $loc ($acc; .[$loc.key] = (($acc[$loc.key] // {}) + {($item.key): $loc.value.stringUnit.value}))
    )
' "$source_file" > "$temp_file"
mv "$temp_file" "$out_file"

echo "updated $(wc -c < "$out_file") bytes to $out_file"
