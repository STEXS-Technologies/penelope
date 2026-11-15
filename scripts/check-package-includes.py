#!/usr/bin/env python3
"""Reject missing literal Rust includes in independently packaged crate archives."""
import argparse
from pathlib import Path, PurePosixPath
import posixpath
import re
import tarfile


INCLUDE = re.compile(
    r'(?:#\[path\s*=\s*|include(?:_str|_bytes)?!\(\s*)"([^"\n]+)"'
)


def validate(archive):
    errors = []
    with tarfile.open(archive) as package:
        members = {member.name: member for member in package.getmembers()}
        for name, member in members.items():
            if not name.endswith(".rs") or not member.isfile():
                continue
            source = package.extractfile(member).read().decode("utf-8")
            for match in INCLUDE.finditer(source):
                relative = match[1]
                target = posixpath.normpath(str(PurePosixPath(name).parent / relative))
                included = members.get(target)
                if (target.split("/", 1)[0] != name.split("/", 1)[0]
                        or included is None or not included.isfile()):
                    errors.append(f"{name}: missing packaged include {relative!r}")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archives", nargs="+", type=Path)
    args = parser.parse_args()
    errors = [error for archive in args.archives for error in validate(archive)]
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    print(f"Literal Rust includes resolve in {len(args.archives)} crate archives")


if __name__ == "__main__":
    main()
