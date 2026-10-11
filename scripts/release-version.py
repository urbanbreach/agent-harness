#!/usr/bin/env python3
"""Reject release versions that do not follow every existing SemVer tag."""
import re
import sys

VERSION = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


def precedence(version):
    match = VERSION.fullmatch(version)
    if not match:
        raise ValueError(f"{version!r} is not a semantic version")
    major, minor, patch, prerelease = match.groups()
    identifiers = []
    for part in prerelease.split(".") if prerelease else []:
        if part.isdigit():
            if len(part) > 1 and part.startswith("0"):
                raise ValueError(f"{version!r} has a leading-zero prerelease number")
            identifiers.append((0, int(part)))
        else:
            identifiers.append((1, part))
    return (int(major), int(minor), int(patch), prerelease is None, identifiers)


def main():
    candidate = precedence(sys.argv[1])
    tags = []
    for line in sys.stdin:
        tag = line.strip()
        try:
            tags.append((precedence(tag.removeprefix("v")), tag))
        except ValueError:
            continue  # Non-release tags have no SemVer precedence.
    if tags:
        latest, tag = max(tags)
        if candidate <= latest:
            raise ValueError(f"{sys.argv[1]} is not newer than {tag}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, IndexError) as error:
        sys.exit(f"release: {error}")
