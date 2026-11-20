"""Deterministic native CLI archives for mesh-thumb; never publishes or signs a release.

Adapted from quickdraw-cli's scripts/package.py (MIT-licensed, miyako/quickdraw-cli).
Deliberate divergences for mesh-thumb:
  * the binary is released unsigned and SHA-256-pinned (no codesigning or
    notarization evidence is produced or asserted);
  * `--version` emits the plain text line `mesh-thumb <version>` (not a JSON
    {"protocol":1,"version":...} object), so the protocol check parses that line;
  * the archive carries the mesh-thumb docs set (docs/cli.md, docs/protocol.md)
    instead of the quickdraw docs set.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import zipfile

try:  # Python 3.11+; CI runs 3.11+, older local hosts use the `tomli` backport
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercise with the backport installed
    import tomli as tomllib  # type: ignore[no-redef]

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "universal-apple-darwin": ("macos", "universal", "mesh-thumb"),
    "x86_64-pc-windows-msvc": ("windows", "x86_64", "mesh-thumb.exe"),
}
PINNED_RUSTC = "rustc 1.100.0 "


def canonical_json(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def check_binary(data, target):
    """Verify actual PE/Mach-O identity rather than trusting the asset filename."""
    if target == "universal-apple-darwin":
        if len(data) < 8 or data[:4] not in (b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"):
            raise ValueError("expected a universal Mach-O executable")
        wide = data[:4] == b"\xca\xfe\xba\xbf"
        size = 32 if wide else 20
        count = struct.unpack_from(">I", data, 4)[0]
        end = 8 + 2 * size
        if count != 2 or len(data) < end:
            raise ValueError("universal executable must contain exactly two slices")
        targets = {0x100000C: "aarch64-apple-darwin", 0x1000007: "x86_64-apple-darwin"}
        seen, ranges = set(), []
        for index in range(2):
            entry = struct.unpack_from(">IIQQII" if wide else ">IIIII", data, 8 + index * size)
            cpu, subtype, offset, length, alignment = entry[:5]
            if cpu not in targets or cpu in seen or (wide and entry[5] != 0):
                raise ValueError("invalid universal architecture table")
            if alignment > 31 or offset % (1 << alignment) or offset < end or length < 32 or offset + length > len(data):
                raise ValueError("invalid universal slice extent or alignment")
            if any(offset < stop and start < offset + length for start, stop in ranges):
                raise ValueError("overlapping universal slices")
            check_binary(data[offset:offset + length], targets[cpu])
            if struct.unpack_from("<I", data, offset + 8)[0] != subtype:
                raise ValueError("universal slice subtype mismatch")
            seen.add(cpu)
            ranges.append((offset, offset + length))
        return
    if target.endswith("apple-darwin"):
        if len(data) < 32 or data[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("expected a little-endian 64-bit Mach-O executable")
        cpu, _, filetype = struct.unpack_from("<III", data, 4)
        expected = 0x100000C if target.startswith("aarch64") else 0x1000007
        if cpu != expected or filetype != 2:
            raise ValueError("Mach-O architecture or executable file type mismatch")
    else:
        if len(data) < 64 or data[:2] != b"MZ":
            raise ValueError("expected a Windows PE executable")
        offset = struct.unpack_from("<I", data, 60)[0]
        if offset > len(data) - 26 or data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("invalid PE header offset or signature")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
        characteristics = struct.unpack_from("<H", data, offset + 22)[0]
        magic = struct.unpack_from("<H", data, offset + 24)[0]
        if machine != 0x8664 or magic != 0x20B or not characteristics & 2 or characteristics & 0x2000:
            raise ValueError("expected x86_64 PE32+ executable, not a DLL")


def notices(target):
    targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"] if target == "universal-apple-darwin" else [target]
    packages = {}
    for native_target in targets:
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", native_target],
            cwd=ROOT,
        ))
        nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
        reached, pending = set(), [metadata["resolve"]["root"]]
        while pending:
            node = pending.pop()
            if node not in reached:
                reached.add(node)
                pending.extend(dep["pkg"] for dep in nodes[node]["deps"])
        packages.update({p["id"]: p for p in metadata["packages"]
                         if p["id"] in reached and p["id"] != metadata["resolve"]["root"]})
    files, inventory = {}, []
    for package in sorted(packages.values(), key=lambda p: (p["name"], p["version"])):
        root = Path(package["manifest_path"]).parent
        selected = []
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE")):
                relative = path.relative_to(root).as_posix()
                name = f"licenses/{package['name']}-{package['version']}/{relative}"
                files[name] = path.read_bytes()
                selected.append(name)
        if not selected:
            raise ValueError(f"missing redistribution notices for {package['name']} {package['version']}")
        inventory.append({
            "name": package["name"], "version": package["version"],
            "declared_license": package["license"], "notice_files": selected,
        })
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    runtime_notice = sysroot / "share" / "doc" / "rust" / "COPYRIGHT-library.html"
    if not runtime_notice.is_file():
        raise ValueError("Rust COPYRIGHT-library.html is required for runtime redistribution")
    files["licenses/rust/COPYRIGHT-library.html"] = runtime_notice.read_bytes()
    files["licenses/dependencies.json"] = canonical_json({
        "scope": "target-filtered Cargo resolution including build tools; over-inclusive, not a runtime SBOM",
        "targets": targets,
        "packages": inventory,
    })
    return files


def create_archive(path, files, executable):
    with zipfile.ZipFile(path, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in sorted(files.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100755 if name == executable else 0o100644) << 16
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, data)


def package(binary, target, output):
    binary = binary.resolve(strict=True)
    rustc = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    if host != target and not (target == "universal-apple-darwin"
                               and host in ("aarch64-apple-darwin", "x86_64-apple-darwin")):
        raise ValueError(f"native packaging requires host {target}, got {host}")
    if not rustc.startswith(PINNED_RUSTC):
        raise ValueError(f"packaging requires the pinned Rust toolchain ('{PINNED_RUSTC.strip()}' prefix), got {rustc.splitlines()[0]}")
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=ROOT):
        raise ValueError("package from a clean committed checkout")
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    cargo = ROOT / "Cargo.toml"
    version = tomllib.loads(cargo.read_text())["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("unsupported package version syntax")
    data = binary.read_bytes()
    check_binary(data, target)
    result = subprocess.run([str(binary), "--version"], check=True, capture_output=True)
    if result.stderr or result.stdout.decode("utf-8") != f"mesh-thumb {version}\n":
        raise ValueError("binary version line differs from this checkout")
    platform, architecture, filename = TARGETS[target]
    executable = f"bin/{filename}"
    files = {
        executable: data,
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "README.md": (ROOT / "README.md").read_bytes(),
        "Cargo.lock": (ROOT / "Cargo.lock").read_bytes(),
        "docs/cli.md": (ROOT / "docs/cli.md").read_bytes(),
        "docs/protocol.md": (ROOT / "docs/protocol.md").read_bytes(),
    }
    files.update(notices(target))
    manifest = {
        "asset_schema": 1, "protocol": 1, "name": "mesh-thumb",
        "version": version, "target": target, "platform": platform, "architecture": architecture,
        "binary": executable, "sha256": sha256(data), "size": len(data),
        "source_revision": source, "rustc": rustc.strip(),
        "signed": False, "notarized": False,
        "files": {name: {"sha256": sha256(content), "size": len(content)} for name, content in sorted(files.items())},
    }
    files["manifest.json"] = canonical_json(manifest)
    stem = f"mesh-thumb-v{version}-{target}"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{stem}.zip"
    sidecar = output / f"{stem}.manifest.json"
    checksum = output / f"{stem}.zip.sha256"
    if any(path.exists() for path in (archive, sidecar, checksum)):
        raise ValueError("asset already exists; use a new output directory")
    create_archive(archive, files, executable)
    sidecar.write_bytes(files["manifest.json"])
    # Raw bytes: guarantees the canonical "hexdigest  name.zip\n" line on every
    # platform (text-mode newline translation differs on Windows).
    checksum.write_bytes(f"{sha256(archive.read_bytes())}  {archive.name}\n".encode("ascii"))
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    try:
        print(package(args.binary, args.target, args.output))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"packaging failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())