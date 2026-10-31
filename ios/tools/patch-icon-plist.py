#!/usr/bin/env python3
"""Point a built `.app`'s Info.plist at the icon files `make-icon.py` drew.

    python3 ios/tools/patch-icon-plist.py xtool/RigelPhone.app/Info.plist

xtool does not run `actool`, so there is no `Assets.car` to name. What
SpringBoard understands instead is `CFBundleIcons`: a primary icon plus the
base names of the PNGs beside the executable (`AppIcon60x60` is answered by
`AppIcon60x60@2x.png` and `@3x`). The launch screen gets the same treatment
through `UILaunchScreen`'s `UIImageName`.

Idempotent: running it twice writes the same dictionary.
"""
from __future__ import annotations

import plistlib
import sys

PHONE = ["AppIcon60x60", "AppIcon40x40", "AppIcon29x29", "AppIcon20x20"]
PAD = ["AppIcon76x76", "AppIcon83.5x83.5", "AppIcon40x40", "AppIcon29x29"]


def main(path: str) -> None:
    with open(path, "rb") as handle:
        info = plistlib.load(handle)

    info["CFBundleIcons"] = {
        "CFBundlePrimaryIcon": {"CFBundleIconFiles": PHONE},
    }
    info["CFBundleIcons~ipad"] = {
        "CFBundlePrimaryIcon": {"CFBundleIconFiles": PAD},
    }
    info["UILaunchScreen"] = {
        "UIImageName": "LaunchStar",
        "UIImageRespectsSafeAreaInsets": False,
    }

    with open(path, "wb") as handle:
        plistlib.dump(info, handle, fmt=plistlib.FMT_BINARY)
    print("patched", path, "->", PHONE)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: patch-icon-plist.py <Info.plist>")
    main(sys.argv[1])
