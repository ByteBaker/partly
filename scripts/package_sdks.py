# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Build standalone Rust crates and host-platform Go/Node archives (Python 3.14)."""

import argparse
import json
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def build(output: Path, *, allow_dirty: bool = False) -> None:
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    package = json.loads((ROOT / "sdk/node/package.json").read_text())
    if package["version"] != version:
        raise ValueError("sdk/node/package.json version must match Cargo.toml before packaging")
    output.mkdir(parents=True, exist_ok=True)
    go_os, go_arch = subprocess.check_output(["go", "env", "GOOS", "GOARCH"], text=True).split()
    if (go_os, go_arch) not in {("darwin", "arm64"), ("linux", "amd64")}:
        raise ValueError("Release packages support macOS ARM64 and Linux x86_64")
    platform = f"{go_os}-{go_arch}"
    subprocess.run(["cargo", "package", "--locked", "--target-dir", "target",
                    "-p", "partly-core", "-p", "partly-rust",
                    *(["--allow-dirty"] if allow_dirty else [])],
                   cwd=ROOT, check=True)
    for name in ("partly-core", "partly-rust"):
        shutil.copy2(ROOT / "target/package" / f"{name}-{version}.crate", output)

    subprocess.run(["sh", str(ROOT / "sdk/go/build.sh")], cwd=ROOT, check=True)
    go_root = ROOT / "sdk/go"
    go_name = f"partly-go-{version}-{platform}"
    with tarfile.open(output / f"{go_name}.tar.gz", "w:gz") as archive:
        for name in ("go.mod", "client.go", "native.go", "client_test.go", "README.md", "LICENSE",
                     "NOTICE", "THIRD_PARTY_NOTICES.txt", "RUST_STDLIB_NOTICES.html"):
            archive.add(go_root / name, arcname=f"{go_name}/{name}")
        native = Path("native") / f"{go_os}_{go_arch}" / "libpartly_ffi.a"
        archive.add(go_root / native, arcname=f"{go_name}/{native}")

    subprocess.run(["node", str(ROOT / "sdk/node/build.js")], cwd=ROOT, check=True)
    # Pack only this target, even if a developer previously built another target.
    node_os_arch = subprocess.check_output(
        ["node", "-p", "`${process.platform}-${process.arch}`"], text=True,
    ).strip()
    with tempfile.TemporaryDirectory(prefix="partly-node-package-") as temporary:
        stage = Path(temporary)
        for name in ("package.json", "README.md", *package["files"]):
            if name != "native":
                shutil.copy2(ROOT / "sdk/node" / name, stage / name)
        shutil.copytree(ROOT / "sdk/node/native" / node_os_arch, stage / "native" / node_os_arch)
        package.pop("scripts", None)
        package["os"] = [go_os]
        package["cpu"] = [node_os_arch.split("-")[1]]
        (stage / "package.json").write_text(json.dumps(package, indent=2) + "\n")
        result = subprocess.check_output(
            ["npm", "pack", "--ignore-scripts", "--json", "--pack-destination", str(output)],
            cwd=stage, text=True,
        )
        packed = output / json.loads(result)[0]["filename"]
        packed.rename(output / f"partly-node-{version}-{platform}.tgz")
    print(f"SDK packages ready: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/sdk-packages")
    parser.add_argument("--allow-dirty", action="store_true", help="Only for testing uncommitted changes")
    args = parser.parse_args()
    build(args.output.resolve(), allow_dirty=args.allow_dirty)
