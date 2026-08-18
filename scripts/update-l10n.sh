#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_file="${NEXAWAL_XCSTRINGS:-$repo_root/../nexawal/nexawal/Localizable.xcstrings}"
out_file="$repo_root/assets/l10n.json"

if [[ ! -f "$source_file" ]]; then
  echo "error: Localizable source not found: $source_file" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required" >&2
  exit 1
fi

jq '
  .strings
  | to_entries
  | map(select(.key != ""))
  | reduce .[] as $item ({};
      . as $acc
      | reduce (($item.value.localizations | if . == null then {} else . end) | to_entries[]) as $loc ($acc; .[$loc.key] = (($acc[$loc.key] // {}) + {($item.key): ($loc.value.stringUnit.value // "")}))
    )
' "$source_file" > "$out_file"

echo "updated $(wc -c < "$out_file") bytes to $out_file"
