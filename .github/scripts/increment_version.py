#!/usr/bin/env python3
import os
import subprocess


def get_latest_tag():
    try:
        # Run git describe to find the closest tag
        result = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0"],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip()
    except subprocess.CalledProcessError:
        return "v0.0.0"


def main():
    bump_type = os.getenv("BUMP_TYPE", "patch")
    release_type = os.getenv("RELEASE_TYPE", "stable")
    github_env_path = os.getenv("GITHUB_ENV")

    latest_tag = get_latest_tag()

    # Parse tag structure
    was_rc = "-rc." in latest_tag
    if was_rc:
        base_part, rc_part = latest_tag.split("-rc.")
        rc_num = int(rc_part)
        version_str = base_part.lstrip("v")
    else:
        rc_num = -1
        version_str = latest_tag.lstrip("v")

    try:
        major, minor, patch = map(int, version_str.split("."))
    except ValueError:
        major, minor, patch = 0, 0, 0

    # Apply version bump logic
    if not was_rc or release_type == "stable":
        if bump_type == "major":
            major += 1
            minor = 0
            patch = 0
        elif bump_type == "minor":
            minor += 1
            patch = 0
        else:
            patch += 1

    # Format the new tag
    if release_type == "rc":
        new_rc_num = (rc_num + 1) if was_rc else 0
        new_tag = f"v{major}.{minor}.{patch}-rc.{new_rc_num}"
    else:
        new_tag = f"v{major}.{minor}.{patch}"

    print(f"Successfully calculated next tag: {new_tag}")

    # Write the new tag to GITHUB_ENV so subsequent steps can use it
    if github_env_path:
        with open(github_env_path, "a") as f:
            f.write(f"NEW_TAG={new_tag}\n")
    else:
        print("Warning: GITHUB_ENV variable not found. Script running locally?")


if __name__ == "__main__":
    main()
