#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("stamp-version.py")
SPEC = importlib.util.spec_from_file_location("stamp_version", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
stamp_version = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stamp_version)


class StampVersionTests(unittest.TestCase):
    def test_set_json_version_updates_top_level_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "package.json"
            path.write_text(json.dumps({"name": "roux", "version": "0.1.0"}))

            stamp_version.set_json_version(path, "1.2.3-pre.4")

            self.assertEqual(json.loads(path.read_text())["version"], "1.2.3-pre.4")

    def test_set_json_version_rejects_non_object(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "package.json"
            path.write_text("[]")

            with self.assertRaisesRegex(ValueError, "not a JSON object"):
                stamp_version.set_json_version(path, "1.2.3")

    def test_set_package_lock_version_updates_root_package(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "package-lock.json"
            path.write_text(
                json.dumps(
                    {
                        "name": "roux",
                        "version": "0.1.0",
                        "packages": {"": {"name": "roux", "version": "0.1.0"}},
                    }
                )
            )

            stamp_version.set_package_lock_version(path, "1.2.3")

            data = json.loads(path.read_text())
            self.assertEqual(data["version"], "1.2.3")
            self.assertEqual(data["packages"][""]["version"], "1.2.3")

    def test_set_package_lock_version_requires_existing_lockfile(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "package-lock.json"

            with self.assertRaisesRegex(FileNotFoundError, "package-lock.json"):
                stamp_version.set_package_lock_version(path, "1.2.3")

    def test_set_cargo_package_version_updates_literal_package_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "Cargo.toml"
            path.write_text('[package]\nname = "roux-cli"\nversion = "0.1.0"\n\n[dependencies]\n')

            stamp_version.set_cargo_package_version(path, "1.2.3")

            self.assertIn('version = "1.2.3"', path.read_text())

    def test_set_cargo_package_version_rejects_workspace_inherited_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "Cargo.toml"
            path.write_text('[package]\nname = "roux-cli"\nversion.workspace = true\n')

            with self.assertRaisesRegex(ValueError, "literal"):
                stamp_version.set_cargo_package_version(path, "1.2.3")

    def test_set_cargo_lock_versions_updates_named_packages(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "Cargo.lock"
            path.write_text(
                '\n'.join(
                    [
                        "[[package]]",
                        'name = "roux-cli"',
                        'version = "0.1.0"',
                        "",
                        "[[package]]",
                        'name = "tower"',
                        'version = "0.5.3"',
                        "",
                        "[[package]]",
                        'name = "roux-desktop"',
                        'version = "0.1.0"',
                        "",
                    ]
                )
            )

            stamp_version.set_cargo_lock_versions(path, "1.2.3", {"roux-cli", "roux-desktop"})

            lock = path.read_text()
            self.assertIn('name = "roux-cli"\nversion = "1.2.3"', lock)
            self.assertIn('name = "roux-desktop"\nversion = "1.2.3"', lock)
            self.assertIn('name = "tower"\nversion = "0.5.3"', lock)

    def test_set_cargo_lock_versions_requires_existing_lockfile(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "Cargo.lock"

            with self.assertRaisesRegex(FileNotFoundError, "Cargo.lock"):
                stamp_version.set_cargo_lock_versions(path, "1.2.3", {"roux-cli"})

    def test_set_cargo_lock_versions_rejects_missing_package_entry(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "Cargo.lock"
            path.write_text('[[package]]\nname = "roux-cli"\nversion = "0.1.0"\n')

            with self.assertRaisesRegex(ValueError, "roux-desktop"):
                stamp_version.set_cargo_lock_versions(path, "1.2.3", {"roux-cli", "roux-desktop"})


if __name__ == "__main__":
    unittest.main()
