"""Synthetic archive integrity tests. No fixture executable is ever launched."""
import copy
import hashlib
import json
from pathlib import Path
import stat
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import external_cli as assets
from test_delivery import synthetic_universal, synthetic_pe


class AssetTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "build").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / "build")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.archive = self.root / "fixture.zip"
        binary = bytearray(synthetic_universal())
        for offset in (48, 80):
            struct.pack_into("<I", binary, offset + 12, 2)
        self.files = {"bin/mesh-thumb": bytes(binary), "LICENSE": b"original test license",
                      "README.md": b"readme", "Cargo.lock": b"test lock",
                      "docs/cli.md": b"contract", "docs/protocol.md": b"protocol",
                      "licenses/runtime.txt": b"test notice"}
        self.manifest = {
            "asset_schema": 1, "protocol": 1, "name": "mesh-thumb", "version": "0.3.0",
            "target": "universal-apple-darwin", "platform": "macos", "architecture": "universal",
            "source_revision": "a" * 40, "binary": "bin/mesh-thumb",
            "sha256": hashlib.sha256(binary).hexdigest(), "size": len(binary),
            "files": {name: {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for name, data in self.files.items()},
        }
        self.lock = {"schema": 1, "repository": "miyako/mesh", "version": "0.3.0",
                     "tag": "v0.3.0", "source_revision": "a" * 40, "assets": {
                         "macos": {"url": "https://github.com/miyako/mesh/releases/download/v0.3.0/mesh-thumb-v0.3.0-universal-apple-darwin.zip",
                                   "target": "universal-apple-darwin", "binary_sha256": self.manifest["sha256"]}}}
        self.write_archive()

    def write_archive(self, extra=(), mode=None):
        manifest = json.dumps(self.manifest).encode()
        with zipfile.ZipFile(self.archive, "w") as packed:
            for name, data in [*self.files.items(), ("manifest.json", manifest), *extra]:
                info = zipfile.ZipInfo(name)
                info.create_system = 3
                info.external_attr = (mode if mode is not None else
                                      stat.S_IFREG | (0o755 if name == "bin/mesh-thumb" else 0o644)) << 16
                packed.writestr(info, data)
        self.lock["assets"]["macos"]["sha256"] = assets.sha256(self.archive)
        self.lock["assets"]["macos"]["manifest_sha256"] = hashlib.sha256(manifest).hexdigest()

    def verify(self):
        return assets.verify_archive(self.archive, self.lock, "macos")

    def test_complete_archive_verifies_without_execution(self):
        with patch.object(assets.subprocess, "run") as run:
            self.assertEqual(self.verify(), self.manifest)
            run.assert_not_called()

    def test_wrong_archive_hash_manifest_hash_or_binary_hash(self):
        for field in ("sha256", "manifest_sha256", "binary_sha256"):
            original = copy.deepcopy(self.lock)
            self.lock["assets"]["macos"][field] = "0" * 64
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.verify()
            self.lock = original

    def test_identity_mismatch_is_not_accepted(self):
        for field, value in (("source_revision", "b" * 40), ("protocol", True),
                             ("target", "aarch64-apple-darwin"), ("architecture", "arm64"),
                             ("version", "0.2.0")):
            before = copy.deepcopy(self.manifest)
            self.manifest[field] = value
            self.write_archive()
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.verify()
            self.manifest = before

    def test_unsigned_release_is_accepted_by_digest(self):
        # mesh-thumb releases are deliberately unsigned; identity + digests pin them.
        for flag in ("signed", "notarized"):
            before = copy.deepcopy(self.manifest)
            self.manifest[flag] = False
            self.write_archive()
            self.assertEqual(self.verify(), self.manifest)
            self.manifest = before

    def test_unsafe_duplicate_unlisted_and_special_entries(self):
        for name in ("../escape", "/absolute", "a/../b", "a\\b", "a:b", "NUL.txt",
                     "a//b", "a/b.", "a/b ", "a/\x00b", "LICENSE", "license", "unlisted"):
            with self.subTest(name=name):
                self.write_archive(extra=[(name, b"bad")])
                with self.assertRaises(ValueError):
                    self.verify()
        for mode in (stat.S_IFLNK | 0o777, stat.S_IFIFO | 0o600, stat.S_IFREG | 0o4755):
            self.write_archive(mode=mode)
            with self.assertRaises(ValueError):
                self.verify()

    def test_missing_required_files_and_corrupted_inventory(self):
        self.files["licenses/runtime.txt"] = b"changed"
        self.write_archive()
        with self.assertRaisesRegex(ValueError, "hash/size"):
            self.verify()
        del self.files["README.md"]
        del self.manifest["files"]["README.md"]
        self.files["licenses/runtime.txt"] = b"test notice"
        self.write_archive()
        with self.assertRaisesRegex(ValueError, "license"):
            self.verify()
        self.files["README.md"] = b"readme"
        self.manifest["files"]["README.md"] = {"size": len(b"readme"),
                                               "sha256": hashlib.sha256(b"readme").hexdigest()}
        self.write_archive()

    def test_expansion_limits_and_non_executable_permission(self):
        with patch.object(assets, "MAX_EXPANDED", 1), self.assertRaisesRegex(ValueError, "expansion"):
            self.verify()
        self.write_archive(mode=stat.S_IFREG | 0o644)
        with self.assertRaisesRegex(ValueError, "permission"):
            self.verify()

    def test_platform_headers_reject_thin_wrong_arch_and_dll(self):
        with self.assertRaises(ValueError):
            assets.binary_header(b"\xcf\xfa\xed\xfe" + b"\0" * 100, "macos")
        pe = bytearray(synthetic_pe())
        struct.pack_into("<H", pe, 64 + 22, 2)
        assets.binary_header(pe, "windows")
        struct.pack_into("<H", pe, 64 + 22, 0x2002)
        with self.assertRaises(ValueError):
            assets.binary_header(pe, "windows")

    def test_https_origin_and_target_guards(self):
        for url in ("http://github.com/x", "https://github.com.evil.invalid/x",
                    "https://user:pass@github.com/x", "https://github.com:444/x"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                assets.checked_url(url)
        with self.assertRaises(ValueError):
            assets.platform_for("windows-arm64")

    def test_pending_release_and_expiring_ci_url_fail_closed(self):
        path = self.root / "cli/asset-lock.json"
        path.parent.mkdir()
        path.write_text(json.dumps(dict(self.lock, assets={})))
        with self.assertRaisesRegex(ValueError, "pending"):
            assets.configuration(self.root)
        self.lock["assets"]["windows"] = dict(self.lock["assets"]["macos"],
                                               target="x86_64-pc-windows-msvc")
        self.lock["assets"]["macos"]["url"] = "https://api.github.com/repos/miyako/mesh/actions/artifacts/1/zip"
        path.write_text(json.dumps(self.lock))
        with self.assertRaisesRegex(ValueError, "release ZIP"):
            assets.configuration(self.root)

    def test_symlink_cache_does_not_escape(self):
        link = self.root / "linked.zip"
        link.symlink_to(self.archive)
        with self.assertRaisesRegex(ValueError, "regular"):
            assets.verify_archive(link, self.lock, "macos")


if __name__ == "__main__":
    unittest.main()