#!/usr/bin/env python3
"""Stamp a Roux release version into every source manifest.

The desktop app and bundled CLI must report the same version. Pre-release and
nightly workflows create tag-only commits, so this script avoids relying on
Cargo or npm being installed while still keeping lockfile package metadata in
sync.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def read_json(path: Path) -> object:
    with path.open() as f:
        return json.load(f)


def write_json(path: Path, data: object) -> None:
    with path.open("w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")


def set_json_version(path: Path, version: str) -> None:
    data = read_json(path)
    if not isinstance(data, dict):
        raise ValueError(f"{path} is not a JSON object")
    data["version"] = version
    write_json(path, data)


def set_package_lock_version(path: Path, version: str) -> None:
    if not path.exists():
        raise FileNotFoundError(f"{path} is required for release version stamping")

    data = read_json(path)
    if not isinstance(data, dict):
        raise ValueError(f"{path} is not a JSON object")

    data["version"] = version
    packages = data.get("packages")
    if isinstance(packages, dict) and isinstance(packages.get(""), dict):
        packages[""]["version"] = version
    write_json(path, data)


def set_cargo_package_version(path: Path, version: str) -> None:
    lines = path.read_text().splitlines()
    in_package = False
    changed = False
    saw_workspace_inherited_version = False

    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("[") and stripped.endswith("]"):
            break
        if in_package and line.startswith("version = "):
            lines[i] = f'version = "{version}"'
            changed = True
            break
        if in_package and stripped == "version.workspace = true":
            saw_workspace_inherited_version = True

    if not changed:
        if saw_workspace_inherited_version:
            raise ValueError(
                f"{path} uses version.workspace = true; stamp-version.py requires a literal "
                "[package] version so release tags can carry app and CLI versions independently"
            )
        raise ValueError(f"did not find [package] version in {path}")

    path.write_text("\n".join(lines) + "\n")


def set_cargo_lock_versions(path: Path, version: str, package_names: set[str]) -> None:
    if not path.exists():
        raise FileNotFoundError(f"{path} is required for release version stamping")

    lines = path.read_text().splitlines()
    current_package: str | None = None
    changed: set[str] = set()

    for i, line in enumerate(lines):
        if line == "[[package]]":
            current_package = None
            continue
        if line.startswith("name = "):
            current_package = line.split('"', 2)[1]
            continue
        if current_package in package_names and line.startswith("version = "):
            lines[i] = f'version = "{version}"'
            changed.add(current_package)
            continue

    missing = package_names - changed
    if missing:
        names = ", ".join(sorted(missing))
        raise ValueError(f"did not find Cargo.lock package version(s): {names}")

    path.write_text("\n".join(lines) + "\n")


def main(argv: list[str]) -> int:
    if len(argv) != 2 or not argv[1]:
        print("usage: scripts/stamp-version.py VERSION", file=sys.stderr)
        return 2

    version = argv[1].removeprefix("v")

    set_json_version(ROOT / "package.json", version)
    set_package_lock_version(ROOT / "package-lock.json", version)
    set_json_version(ROOT / "src-tauri" / "tauri.conf.json", version)
    set_cargo_package_version(ROOT / "src-tauri" / "Cargo.toml", version)
    set_cargo_package_version(ROOT / "crates" / "roux-cli" / "Cargo.toml", version)
    set_cargo_lock_versions(ROOT / "Cargo.lock", version, {"roux-cli", "roux-desktop"})

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
