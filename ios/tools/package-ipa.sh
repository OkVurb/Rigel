#!/usr/bin/env bash
# Build RigelPhone, give it its icon, and write an IPA Sideloadly can install.
#
#   bash ios/tools/package-ipa.sh [output.ipa]
#
# `xtool dev build --ipa` would package the app, but it runs no asset compiler,
# so the icon has to be copied in beside the executable and named in
# Info.plist before the zip is made. That is all this script adds.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
package="$(cd "$here/../RigelPhone" && pwd)"
icons="$package/Resources/Icon"
out="${1:-$package/xtool/RigelPhone.ipa}"
mkdir -p "$(dirname "$out")"
# Absolute, because the zip runs from inside `xtool/`.
out="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"

cd "$package"
xtool dev build -c release

app="xtool/RigelPhone.app"
[ -d "$app" ] || { echo "no app bundle at $app" >&2; exit 1; }

cp "$icons"/*.png "$app"/
python3 "$here/patch-icon-plist.py" "$app/Info.plist"

rm -rf xtool/Payload "$out"
mkdir -p xtool/Payload
cp -R "$app" xtool/Payload/
(cd xtool && zip -qry "$out" Payload)
rm -rf xtool/Payload

echo "ipa: $out"
unzip -l "$out" | grep -E "AppIcon60x60@3x|LaunchStar|Info.plist" | sed 's/^/  /'
