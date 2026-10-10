#!/usr/bin/env python3
"""Extract the dated changelog section for a release tag, as in Shardline."""
import argparse
import re
from pathlib import Path


def release_notes(changelog: str, tag: str) -> str:
    version = tag.removeprefix("v")
    headings = list(re.finditer(
        rf"^## \[{re.escape(version)}\] - \d{{4}}-\d{{2}}-\d{{2}}$",
        changelog,
        re.MULTILINE,
    ))
    if len(headings) != 1:
        raise ValueError(f"Expected one dated changelog entry for {version}, found {len(headings)}")
    remainder = changelog[headings[0].end():]
    notes = re.split(r"^## ", remainder, maxsplit=1, flags=re.MULTILINE)[0].strip()
    if not notes:
        raise ValueError(f"Empty changelog entry for {version}")
    return notes + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        notes = release_notes(args.changelog.read_text(encoding="utf-8"), args.tag)
    except ValueError as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(notes, encoding="utf-8")


if __name__ == "__main__":
    main()
