#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_icon="$repo_root/assets/nexawal.png"
macos_icon="$repo_root/assets/nexawal-macos.png"
bundle_icon="$repo_root/assets/nexawal.icns"

for required_tool in magick sips iconutil; do
    if ! command -v "$required_tool" >/dev/null 2>&1; then
        echo "error: $required_tool is required to regenerate the macOS icon" >&2
        exit 1
    fi
done

icon_work_dir="$(mktemp -d)"
trap 'rm -rf "$icon_work_dir"' EXIT

# Native macOS app icons occupy a smaller, rounded shape inside the 1024px
# canvas. The source artwork remains full bleed for Windows and Linux.
magick "$source_icon" -resize 824x824! "$icon_work_dir/artwork.png"
magick -size 824x824 xc:black -fill white \
    -draw 'roundrectangle 0,0 823,823 185,185' \
    "$icon_work_dir/mask.png"
magick "$icon_work_dir/artwork.png" "$icon_work_dir/mask.png" \
    -alpha off -compose CopyOpacity -composite \
    "$icon_work_dir/masked.png"
magick "$icon_work_dir/masked.png" -gravity center -background none \
    -extent 1024x1024 "$macos_icon"

iconset_dir="$icon_work_dir/NexaWal.iconset"
mkdir -p "$iconset_dir"

sips -z 16 16 "$macos_icon" --out "$iconset_dir/icon_16x16.png" >/dev/null
sips -z 32 32 "$macos_icon" --out "$iconset_dir/icon_16x16@2x.png" >/dev/null
sips -z 32 32 "$macos_icon" --out "$iconset_dir/icon_32x32.png" >/dev/null
sips -z 64 64 "$macos_icon" --out "$iconset_dir/icon_32x32@2x.png" >/dev/null
sips -z 128 128 "$macos_icon" --out "$iconset_dir/icon_128x128.png" >/dev/null
sips -z 256 256 "$macos_icon" --out "$iconset_dir/icon_128x128@2x.png" >/dev/null
sips -z 256 256 "$macos_icon" --out "$iconset_dir/icon_256x256.png" >/dev/null
sips -z 512 512 "$macos_icon" --out "$iconset_dir/icon_256x256@2x.png" >/dev/null
sips -z 512 512 "$macos_icon" --out "$iconset_dir/icon_512x512.png" >/dev/null
cp "$macos_icon" "$iconset_dir/icon_512x512@2x.png"

iconutil -c icns "$iconset_dir" -o "$bundle_icon"

echo "Generated $macos_icon"
echo "Generated $bundle_icon"
