#!/usr/bin/env python3
"""Render Scoop and Homebrew sources from published release sidecars."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


SEMVER_TAG = re.compile(
    r"^v(?P<version>0|[1-9]\d*)\."
    r"(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
SIDECAR_LINE = re.compile(r"^([0-9A-Fa-f]{64})[ \t]+\*?([^\r\n]+)$")
UNRESOLVED_TOKEN = re.compile(r"@@[A-Z0-9_]+@@")


def sidecar_sha256(path: Path, expected_name: str) -> str:
    lines = [line for line in path.read_text(encoding="utf-8-sig").splitlines() if line]
    if len(lines) != 1:
        raise ValueError(f"{path} must contain exactly one non-empty checksum line")
    match = SIDECAR_LINE.fullmatch(lines[0])
    if match is None:
        raise ValueError(f"{path} is not a SHA-256 sidecar")
    actual_name = match.group(2).strip()
    if actual_name != expected_name:
        raise ValueError(
            f"{path} names {actual_name!r}; expected published asset {expected_name!r}"
        )
    return match.group(1).lower()


def render(template: Path, destination: Path, replacements: dict[str, str]) -> None:
    content = template.read_text(encoding="utf-8")
    for token, value in replacements.items():
        content = content.replace(f"@@{token}@@", value)
    unresolved = sorted(set(UNRESOLVED_TOKEN.findall(content)))
    if unresolved:
        raise ValueError(f"unresolved tokens in {template}: {', '.join(unresolved)}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(content, encoding="utf-8", newline="\n")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True)
    parser.add_argument("--win-x86-64-sidecar", required=True, type=Path)
    parser.add_argument("--win-arm64-sidecar", required=True, type=Path)
    parser.add_argument("--macos-universal-sidecar", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()

    tag_match = SEMVER_TAG.fullmatch(args.tag)
    if tag_match is None:
        raise ValueError(f"release tag is not v-prefixed SemVer: {args.tag!r}")
    version = args.tag[1:]

    names = {
        "win_x86_64": f"LTBox-win_x86_64-{args.tag}.zip",
        "win_arm64": f"LTBox-win_arm64-{args.tag}.zip",
        "macos_universal": f"LTBox-macos_universal-{args.tag}.tar.gz",
    }
    replacements = {
        "VERSION": version,
        "WIN_X86_64_SHA256": sidecar_sha256(
            args.win_x86_64_sidecar, names["win_x86_64"]
        ),
        "WIN_ARM64_SHA256": sidecar_sha256(
            args.win_arm64_sidecar, names["win_arm64"]
        ),
        "MACOS_UNIVERSAL_SHA256": sidecar_sha256(
            args.macos_universal_sidecar, names["macos_universal"]
        ),
    }

    root = Path(__file__).resolve().parent
    scoop_output = args.output_dir / "scoop" / "ltbox.json"
    render(root / "scoop" / "ltbox.json.tmpl", scoop_output, replacements)
    # Parse the rendered JSON here so malformed Scoop sources never reach a bucket.
    json.loads(scoop_output.read_text(encoding="utf-8"))

    render(
        root / "homebrew" / "Casks" / "ltbox.rb.tmpl",
        args.output_dir / "homebrew" / "Casks" / "ltbox.rb",
        replacements,
    )


if __name__ == "__main__":
    main()
