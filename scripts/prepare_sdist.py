"""Prune the SDK sdist lockfile and extract its verified source (Python 3.12+)."""

import argparse
import io
import subprocess
import tarfile
import tempfile
from pathlib import Path

import tomllib


def pinned_packages(lock):
    packages = tomllib.loads(lock.decode())["package"]
    return {
        (package["name"], package["version"], package.get("source")): package.get("checksum")
        for package in packages
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source", type=Path, help="New directory for the extracted SDK source")
    args = parser.parse_args()
    if args.source.exists():
        parser.error("source directory must not already exist")
    args.source.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.source.parent) as directory:
        unpacked = Path(directory)
        with tarfile.open(args.archive, "r:gz") as archive:
            archive.extractall(unpacked, filter="data")
        roots = list(unpacked.iterdir())
        assert len(roots) == 1 and roots[0].is_dir(), "Expected one source directory"
        root = roots[0]
        lock = root / "Cargo.lock"
        original = pinned_packages(lock.read_bytes())

        # Maturin trims workspace members but copies their entire lockfile into the sdist.
        command = ["cargo", "metadata", "--offline", "--format-version", "1"]
        subprocess.run(command, cwd=root, check=True, stdout=subprocess.DEVNULL)
        updated = lock.read_bytes()
        retained = pinned_packages(updated)
        assert retained, "The SDK lockfile must contain dependencies"
        assert retained.items() <= original.items(), "Source packaging changed a dependency pin"
        subprocess.run([*command, "--locked"], cwd=root, check=True, stdout=subprocess.DEVNULL)

        # Preserve every original archive member except the now-pruned lockfile.
        with tempfile.NamedTemporaryFile(dir=args.archive.parent, delete=False) as temporary:
            replacement = Path(temporary.name)
        try:
            with tarfile.open(args.archive, "r:gz") as source, tarfile.open(replacement, "w:gz") as destination:
                for member in source:
                    if member.name == f"{root.name}/Cargo.lock":
                        member.size = len(updated)
                        destination.addfile(member, io.BytesIO(updated))
                    else:
                        destination.addfile(member, source.extractfile(member) if member.isfile() else None)
            replacement.replace(args.archive)
        finally:
            replacement.unlink(missing_ok=True)
        root.rename(args.source)
    print(f"SDK source ready: kept {len(retained)} dependency pins; removed {len(original) - len(retained)} unused packages")


if __name__ == "__main__":
    main()
