"""Native tests of the externally verified executable, never a second CLI build."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_cli import native_target
from make_fixtures import write_fixtures


class NativeContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if os.environ.get("TEST_CLI_LANGUAGE") == "none":
            raise unittest.SkipTest("native executable checks explicitly disabled")
        if not (ROOT / "cli/asset-lock.json").is_file():
            raise unittest.SkipTest("verified durable CLI release pins are pending")
        target = native_target()
        cls.executable = ROOT / "build" / target / ("mesh-thumb.exe" if os.name == "nt" else "mesh-thumb")
        if not cls.executable.is_file():
            raise RuntimeError("missing verified CLI; run scripts/build_cli.py --target " + target)
        from build_provenance import verify
        verify(ROOT, target, cls.executable)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mesh-thumb 日本語 ", dir=ROOT / "build")
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        write_fixtures(self.folder)
        self.input = self.folder / "triangle.stl"

    def invoke(self, args, code=0, error=None):
        completed = subprocess.run([str(self.executable), *map(str, args)],
                                   capture_output=True, timeout=60)
        self.assertEqual(completed.returncode, code, completed.stderr)
        self.assertTrue(completed.stdout.startswith(b"{"))
        envelope = json.loads(completed.stdout)
        self.assertEqual(envelope["schema_version"], 1)
        if code == 0:
            self.assertEqual(completed.stderr, b"")
            self.assertTrue(envelope["success"])
            return envelope["result"]
        self.assertFalse(envelope["success"])
        failure = envelope["error"]
        self.assertNotEqual(completed.stderr, b"")
        stderr = completed.stderr.decode("utf-8")
        self.assertRegex(stderr, r"^mesh-thumb: [a-z_]+: ")
        self.assertTrue(stderr.endswith("\n"))
        if error is not None:
            self.assertEqual(failure["code"], error)
        return failure

    def test_version_and_capabilities(self):
        from external_cli import configuration
        version = configuration(ROOT)["version"]
        completed = subprocess.run([str(self.executable), "--version"], capture_output=True, timeout=30)
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(completed.stdout, ("mesh-thumb " + version + "\n").encode("utf-8"))
        self.assertEqual(completed.stderr, b"")
        value = self.invoke(["capabilities", "--json"])
        self.assertEqual(value["app"], "mesh-thumb")
        self.assertEqual(value["version"], version)
        self.assertEqual(value["backends"], ["auto", "cpu"])
        self.assertEqual(value["formats"], ["stl", "ply"])
        self.assertEqual(value["views"], ["iso", "top", "front", "side"])
        self.assertEqual(value["max_output_dimension"], 8192)
        self.assertEqual(value["max_output_pixels"], 67108864)
        self.assertEqual(value["max_quality"], 8)
        self.assertFalse(value["gpu_backend_available"])

    def test_render_deterministic_and_geometry(self):
        first = self.folder / "first.png"
        second = self.folder / "second.png"
        one = self.invoke(["render", self.input, "-o", first, "--size", "64", "--json"])
        two = self.invoke(["render", self.input, "-o", second, "--size", "64", "--json"])
        self.assertEqual(hashlib.sha256(first.read_bytes()).hexdigest(),
                         hashlib.sha256(second.read_bytes()).hexdigest())
        self.assertEqual((one["width"], one["height"]), (64, 64))
        self.assertEqual((one["input"]["format"], one["input"]["file_size"]),
                         ("stl_ascii", self.input.stat().st_size))
        self.assertEqual(one["input"]["path"], str(self.input))
        geometry = one["geometry"]
        self.assertEqual(geometry["kind"], "mesh")
        self.assertEqual((geometry["vertices"], geometry["triangles"]), (3, 1))
        self.assertFalse(geometry["colors"])
        self.assertFalse(geometry["normals"])
        self.assertTrue(geometry["bounds"]["valid"])
        self.assertEqual(one["normalize_scale"], 1.0)
        self.assertEqual(one["warnings"]["degenerate_triangles"], 0)
        self.assertEqual(one["cache"], "off")

    def test_render_ply_overwrite_and_dimensions(self):
        output = self.folder / "cube.png"
        value = self.invoke(["render", self.folder / "cube.ply", "-o", output,
                             "--size", "32", "--view", "top", "--backend", "cpu", "--json"])
        self.assertEqual(value["geometry"]["triangles"], 12)
        self.assertEqual(value["input"]["format"], "ply_ascii")
        # Output is clobbered silently: a second render to the same path succeeds.
        self.invoke(["render", self.folder / "cube.ply", "-o", output, "--size", "48", "--json"])
        sized = self.invoke(["render", self.folder / "cube.ply", "-o", self.folder / "wide.png",
                             "--width", "32", "--height", "24", "--json"])
        self.assertEqual((sized["width"], sized["height"]), (32, 24))

    def test_inspect_and_failure_envelopes(self):
        inspected = self.invoke(["inspect", self.folder / "cube.ply", "--json"])
        self.assertEqual(inspected["input"]["format"], "ply_ascii")
        self.assertEqual(inspected["geometry"]["triangles"], 12)
        missing = self.folder / "missing.stl"
        failure = self.invoke(["inspect", missing, "--json"], code=1, error="io_error")
        self.assertEqual(failure["path"], str(missing))
        malformed = self.invoke(["render", self.folder / "malformed.stl",
                                 "-o", self.folder / "m.png", "--json"], code=1, error="unsupported_format")
        self.assertIsNotNone(malformed["path"])
        self.assertIn("error", malformed["message"].lower())


if __name__ == "__main__":
    unittest.main()