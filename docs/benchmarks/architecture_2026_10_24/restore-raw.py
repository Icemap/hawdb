#!/usr/bin/env python3
# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Restore only pinned, fully verified architecture evidence; never overwrite."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile


def digest(stream):
    result = hashlib.sha256()
    for block in iter(lambda: stream.read(65536), b""):
        result.update(block)
    return result.hexdigest()


def check_file(path, expected):
    with path.open("rb") as stream:
        if digest(stream) != expected:
            raise ValueError(f"checksum mismatch: {path}")


def restore(group, assets, destination):
    check_file(assets / group["archive"], group["archive_sha256"])
    check_file(assets / group["manifest"], group["manifest_sha256"])
    manifest = json.loads((assets / group["manifest"]).read_text())
    files = manifest["files"]
    if len(files) != group["files"] or sum(f["size"] for f in files) != group["raw_bytes"]:
        raise ValueError("manifest count/size differs from the pinned index")
    destination = destination.resolve()
    with tarfile.open(assets / group["archive"], "r:gz") as archive:
        names = archive.getnames()
        if len(names) != len(set(names)) or sorted(names) != sorted(f["path"] for f in files):
            raise ValueError("archive inventory differs from the complete manifest")
        # Verify every member and every destination before writing the first file.
        for entry in files:
            name = entry["path"]
            relative = PurePosixPath(name)
            if (relative.is_absolute() or "\\" in name
                    or any(part in {"", ".", ".."} for part in name.split("/"))
                    or relative.parts[:3] != ("docs", "benchmarks", "architecture_2026_10_24")):
                raise ValueError(f"unsafe evidence path: {name}")
            target = destination / relative
            if not target.resolve().is_relative_to(destination):
                raise ValueError(f"destination escapes through a symlink: {name}")
            if target.exists() or target.is_symlink():
                raise FileExistsError(f"refusing to overwrite: {target}")
            member = archive.getmember(name)
            if not member.isfile() or member.size != entry["size"]:
                raise ValueError(f"member type/size mismatch: {name}")
            with archive.extractfile(member) as stream:
                if digest(stream) != entry["sha256"]:
                    raise ValueError(f"member checksum mismatch: {name}")
        for entry in files:
            target = destination / entry["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.extractfile(entry["path"]) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output, length=65536)
    print(f"Restored {len(files)} verified files ({group['raw_bytes']:,} bytes).")


def main():
    here = Path(__file__).resolve().parent
    index = json.loads((here / "RAW_EVIDENCE.json").read_text())
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--group", required=True, choices=sorted(index["groups"]))
    parser.add_argument("--destination", type=Path, default=here.parents[2])
    parser.add_argument("--assets-dir", type=Path, help="use already downloaded assets")
    args = parser.parse_args()
    group = index["groups"][args.group]
    if args.assets_dir:
        restore(group, args.assets_dir, args.destination)
        return
    with tempfile.TemporaryDirectory(prefix="hawdb-1240-restore-") as temporary:
        assets = Path(temporary)
        command = ["gh", "release", "download", index["release_tag"],
                   "--repo", index["repository"], "--dir", str(assets)]
        for name in [group["archive"], group["manifest"]]:
            if Path(name).name != name or name.startswith("-"):
                raise ValueError("invalid pinned asset name")
            command += ["--pattern", name]
        subprocess.run(command, check=True)
        restore(group, assets, args.destination)


if __name__ == "__main__":
    main()
