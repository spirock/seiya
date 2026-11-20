"""Offline audit of GitHub Actions run and artifact metadata, not artifact bytes."""

import argparse
import json
from pathlib import Path
import re
import sys

NAMES = {
    "seiya-rust-packages-macOS-ARM64",
    "seiya-rust-pkg-macOS-ARM64",
    "seiya-rust-packages-Linux-X64",
    "seiya-rust-Windows-X64-portable",
    "seiya-rust-Windows-X64-installer",
}
SHA = re.compile(r"[0-9a-f]{40}")


def valid_id(value):
    return type(value) is int and value > 0


def audit(run, artifacts, expected_sha):
    """Validate metadata against an independently supplied expected commit."""
    if not isinstance(expected_sha, str) or not SHA.fullmatch(expected_sha):
        raise ValueError("invalid_expected_sha")
    if not isinstance(run, dict) or not isinstance(artifacts, dict):
        raise ValueError("invalid_metadata")
    repository = run.get("repository")
    head_repository = run.get("head_repository")
    if (
        not valid_id(run.get("id"))
        or run.get("status") != "completed"
        or run.get("conclusion") != "success"
        or run.get("event") != "push"
        or run.get("head_branch") != "main"
        or run.get("head_sha") != expected_sha
        or not isinstance(repository, dict)
        or repository.get("full_name") != "spirock/seiya"
        or not valid_id(repository.get("id"))
        or not isinstance(head_repository, dict)
        or head_repository.get("id") != repository["id"]
        or head_repository.get("full_name") != "spirock/seiya"
    ):
        raise ValueError("unapproved_run")
    entries = artifacts.get("artifacts")
    if not isinstance(entries, list) or len(entries) > 100:
        raise ValueError("invalid_artifact_list")
    selected = {}
    ids = set()
    for item in entries:
        if not isinstance(item, dict):
            raise ValueError("invalid_artifact")
        name = item.get("name")
        if not isinstance(name, str):
            raise ValueError("invalid_artifact")
        if name not in NAMES:
            continue
        info = item.get("workflow_run")
        if (
            name in selected
            or item.get("expired") is not False
            or not valid_id(item.get("id"))
            or item["id"] in ids
            or not valid_id(item.get("size_in_bytes"))
            or not isinstance(info, dict)
            or info.get("id") != run["id"]
            or info.get("head_sha") != expected_sha
            or info.get("head_branch") != "main"
            or info.get("repository_id") != repository["id"]
            or info.get("head_repository_id") != repository["id"]
        ):
            raise ValueError("invalid_required_artifact")
        ids.add(item["id"])
        selected[name] = item["id"]
    if set(selected) != NAMES:
        raise ValueError("missing_required_artifact")
    return selected


def load_json(path):
    with Path(path).open("rb") as stream:
        data = stream.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError("metadata_too_large")
    return json.loads(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", required=True)
    parser.add_argument("--artifacts", required=True)
    parser.add_argument("--expected-sha", required=True)
    args = parser.parse_args()
    try:
        selected = audit(load_json(args.run), load_json(args.artifacts), args.expected_sha)
        for name, artifact_id in sorted(selected.items()):
            print(f"{name} {artifact_id}")
    except (OSError, ValueError, TypeError) as exc:
        print(f"run audit failed: {exc}", file=sys.stderr)
        return 1
    print("Metadata only: verify downloaded hashes, payload and signatures separately.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
