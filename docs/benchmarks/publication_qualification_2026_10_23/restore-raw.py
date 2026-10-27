#!/usr/bin/env python3
# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Restore hash-verified historical raw evidence without replacing local files."""

import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tempfile

root = Path(__file__).resolve().parent
manifest = json.loads((root / "raw-manifest.json").read_text())
with tempfile.TemporaryDirectory(prefix="hawdb-evidence-") as temporary:
    archive = Path(temporary) / manifest["archive"]["name"]
    subprocess.run([
        "gh", "release", "download", manifest["release_tag"],
        "--repo", "nowledge-co/hawdb", "--pattern", archive.name,
        "--dir", temporary,
    ], check=True)
    with archive.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != manifest["archive"]["sha256"]:
            raise ValueError("archive checksum mismatch")
    with tarfile.open(archive) as source:
        members = source.getmembers()
        for member, expected in zip(members, manifest["files"], strict=True):
            path = PurePosixPath(member.name)
            if not member.isfile() or path.is_absolute() or ".." in path.parts:
                raise ValueError("unsafe archive member")
            if member.name != expected["path"] or member.size != expected["bytes"]:
                raise ValueError("archive inventory mismatch")
            if root.joinpath(*path.parts).exists():
                raise FileExistsError(member.name)
            with source.extractfile(member) as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != expected["sha256"]:
                    raise ValueError("member checksum mismatch")
        source.extractall(root, filter="data")
print(f"Restored {len(manifest['files'])} verified files to {root}")
