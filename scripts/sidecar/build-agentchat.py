#!/usr/bin/env python3
"""Build the Chat sidecar: pinned agentchattr source + an embeddable Python runtime.

The installer ships the result under `$RESOURCE/sidecar/`; `rigel-app` copies the source to
the app-data directory at first launch so the room can write config.local.toml and its JSONL
there, while `python.exe` stays in the read-only install directory. Nothing here runs at
install time on a user's machine, and nothing needs Python on their PATH.

Usage (from the repository, with any Python 3.10+ on PATH):

    python scripts/sidecar/build-agentchat.py [--force]

Outputs `crates/rigel-app/resources/sidecar/{agentchattr,python}`. The downloads are cached
under `vendor/cache/`; both the git commit and the archive hash are pinned in
`agentchattr.pin`.
"""

from __future__ import annotations

import argparse
import hashlib
import platform
import shutil
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PIN = Path(__file__).with_name("agentchattr.pin")
OUT = ROOT / "crates/rigel-app/resources/sidecar"
CHECKOUT = ROOT / "vendor/agentchattr"
CACHE = ROOT / "vendor/cache"
MARKER = ".rigel-sidecar"
NOTICE = "THIRD-PARTY-NOTICES.txt"


def require_windows() -> None:
    """The runtime this downloads is the Windows embeddable build, and the resource paths are
    Windows paths. Refuse loudly instead of producing a sidecar the installer cannot run."""
    if sys.platform != "win32":
        raise SystemExit(
            "the Chat sidecar bundles the Windows embeddable Python; build it on Windows "
            "(the desktop installer is Windows-first, see docs/PLATFORMS.md)"
        )
    machine = platform.machine().lower()
    if machine not in {"amd64", "x86_64"}:
        raise SystemExit(
            f"the Chat sidecar is built for Windows x64, not {machine or 'an unknown CPU'}"
        )


def read_pin() -> dict[str, str]:
    values: dict[str, str] = {}
    for line in PIN.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            key, _, value = line.partition("=")
            values[key.strip()] = value.strip()
    return values


def run(argv: list[str], cwd: Path | None = None) -> None:
    print("  $", " ".join(argv), flush=True)
    subprocess.run(argv, cwd=cwd, check=True)


def ensure_checkout(commit: str, repo: str) -> None:
    if not (CHECKOUT / ".git").exists():
        CHECKOUT.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", repo, str(CHECKOUT)])
    head = subprocess.run(
        ["git", "-C", str(CHECKOUT), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if head == commit:
        print(f"agentchattr at {commit[:12]}")
        return
    print(f"agentchattr at {head[:12]}, moving to {commit[:12]}")
    run(["git", "-C", str(CHECKOUT), "fetch", "--depth", "1", "origin", commit])
    run(["git", "-C", str(CHECKOUT), "checkout", "--detach", "FETCH_HEAD"])
    head = subprocess.run(
        ["git", "-C", str(CHECKOUT), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if head != commit:
        raise SystemExit(f"checkout is {head}, expected {commit}")


def download(url: str, target: Path, sha256: str | None = None) -> Path:
    if target.exists() and (sha256 is None or sha256_file(target) == sha256):
        return target
    target.parent.mkdir(parents=True, exist_ok=True)
    print(f"downloading {url}")
    with urllib.request.urlopen(url) as response, target.open("wb") as out:
        shutil.copyfileobj(response, out)
    if sha256 and sha256_file(target) != sha256:
        target.unlink()
        raise SystemExit(f"hash mismatch for {url}")
    return target


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def install_python(version: str, sha256: str, force: bool) -> Path:
    target = OUT / "python"
    marker = target / MARKER
    if marker.exists() and marker.read_text().strip() == version and not force:
        print(f"python {version} already built")
        return target

    if target.exists():
        shutil.rmtree(target)
    archive = download(
        f"https://www.python.org/ftp/python/{version}/python-{version}-embed-amd64.zip",
        CACHE / f"python-{version}-embed-amd64.zip",
        sha256,
    )
    with zipfile.ZipFile(archive) as bundle:
        bundle.extractall(target)

    # The embeddable distribution ships with `import site` commented out; pip needs it, and
    # site-packages is where the wheels land.
    pth = next(target.glob("python*._pth"))
    lines = [line for line in pth.read_text().splitlines() if line.strip() != "import site"]
    lines = [line for line in lines if line.strip() != r"Lib\site-packages"]
    lines.append(r"Lib\site-packages")
    lines.append("import site")
    pth.write_text("\n".join(lines) + "\n")

    get_pip = download("https://bootstrap.pypa.io/get-pip.py", CACHE / "get-pip.py")
    python = target / "python.exe"
    run([str(python), str(get_pip), "--no-warn-script-location"])
    run([str(python), "-m", "pip", "install", "--no-warn-script-location", "-r",
         str(CHECKOUT / "requirements.txt")])
    marker.write_text(version + "\n")
    return target


def copy_source(target: Path) -> None:
    if target.exists():
        shutil.rmtree(target)
    skipped = {".git", "data", ".venv", "__pycache__", "tests", "windows", "macos-linux"}
    def ignore(directory: str, names: list[str]) -> set[str]:
        return {name for name in names if name in skipped or name.endswith(".pyc")}
    shutil.copytree(CHECKOUT, target, ignore=ignore)
    print(f"source copied to {target}")


def write_notices(commit: str, version: str) -> Path:
    """Write the consolidated notice that ships beside the pinned source and runtime.

    agentchattr's MIT text and Python's PSF text are copied from the exact files being bundled,
    so a pin bump cannot leave a stale notice behind. Every installed Python distribution keeps
    its own licence under `Lib/site-packages/*.dist-info/`, which the installer ships as-is.
    """
    agentchattr = CHECKOUT / "LICENSE"
    python_license = OUT / "python" / "LICENSE.txt"
    for path in (agentchattr, python_license):
        if not path.is_file():
            raise SystemExit(f"cannot write {NOTICE}: {path} is missing")
    text = f"""Rigel ships a Chat sidecar so the room works on a machine without Python installed.

This file covers the two components in `$RESOURCE/sidecar` that are not Rigel's own work.
Rigel itself is MIT OR Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE in the repository).

================================================================================
agentchattr (https://github.com/bcurts/agentchattr), commit {commit}
MIT License
================================================================================

{agentchattr.read_text(encoding="utf-8").strip()}

================================================================================
Python {version} (https://www.python.org/), Python Software Foundation License
================================================================================

{python_license.read_text(encoding="utf-8").strip()}

================================================================================
Python packages installed under Lib/site-packages
================================================================================

Each distribution keeps its own licence in `Lib/site-packages/<name>-<version>.dist-info/`
(`METADATA` and, when the wheel provides one, `licenses/`). Those files are shipped with the
sidecar unchanged; this notice does not replace them.
"""
    target = OUT / NOTICE
    target.write_text(text.replace("\r\n", "\n"), encoding="utf-8")
    print(f"notices written to {target}")
    return target


def main() -> int:
    require_windows()

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--force", action="store_true", help="rebuild the runtime from scratch")
    args = parser.parse_args()

    pin = read_pin()
    commit, repo = pin["commit"], pin["repo"]
    version, sha256 = pin["python"], pin["python_sha256"]

    print(f"sidecar: agentchattr {commit[:12]}, python {version}")
    ensure_checkout(commit, repo)
    OUT.mkdir(parents=True, exist_ok=True)
    python = install_python(version, sha256, args.force)
    copy_source(OUT / "agentchattr")
    (OUT / "agentchattr" / MARKER).write_text(f"{commit} {version}\n")
    write_notices(commit, version)

    total = sum(path.stat().st_size for path in OUT.rglob("*") if path.is_file())
    print(f"python: {python}")
    print(f"sidecar size: {total / 1e6:.1f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
