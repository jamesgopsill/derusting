#!/usr/bin/env python3
import hashlib
import os
import subprocess
import sys

sha_path = "firmware.bbf.sha256"


def generate_sha256(bin_path: str):
    sha256_hash = hashlib.sha256()
    with open(bin_path, "rb") as f:
        # Read in blocks to handle large binaries efficiently
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256_hash.update(byte_block)

    sha_result = sha256_hash.hexdigest()

    with open(sha_path, "w") as out:
        out.write(f"{sha_result}  {os.path.basename(bin_path)}\n")
    print(f"Generated SHA-256 file: {sha_path}")


def main():
    new_tag = os.getenv("NEW_TAG")
    release_type = os.getenv("RELEASE_TYPE", "stable")
    commit_sha = os.getenv("COMMIT_SHA", "unknown")

    if not new_tag:
        print("Error: NEW_TAG environment variable is missing.", file=sys.stderr)
        sys.exit(1)

    binary_path = "buddy/build/mini_release_boot/firmware.bbf"

    # 1. Generate SHA-256 hash
    if os.path.exists(binary_path):
        generate_sha256(binary_path)
    else:
        print(f"Error: Binary not found at {binary_path}", file=sys.stderr)
        sys.exit(1)

    # 2. Assemble the GitHub CLI command
    cmd = [
        "gh",
        "release",
        "create",
        new_tag,
        binary_path,
        sha_path,
        "--target",
        "main",
        "--title",
        f"Release {new_tag}",
        "--notes",
        f"GitHub Actions Release ({release_type}). Commit: {commit_sha}",
    ]

    # Add the pre-release flag if it's an RC
    if release_type == "rc":
        cmd.append("--prerelease")

    if release_type == "dry_run":
        print("🔧 DRY RUN MODE - No release will be created")
        print(f"Command that would be executed: {' '.join(cmd)}")
        print("\nRelease Details:")
        print(f"  Tag: {new_tag}")
        print(f"  Type: {release_type}")
        print(f"  Commit: {commit_sha}")
        print(f"  Binary: {binary_path}")
        print(f"  SHA-256 file: {sha_path}")
    else:
        print(f"Running command: {' '.join(cmd)}")
        subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
