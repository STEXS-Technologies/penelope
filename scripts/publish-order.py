#!/usr/bin/env python3
"""Derive and validate the dependency-first crates.io publish order.

All workspace-internal path dependencies are considered, including normal,
build, and dev dependencies: Cargo verifies all of them when packaging a
crate.  The resulting order is deterministic and excludes `publish = false`
workspace members.
"""

from __future__ import annotations

import argparse
import heapq
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


def metadata(path: str | None) -> dict:
    if path:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def is_within(path: str, root: Path) -> bool:
    try:
        Path(path).resolve().relative_to(root)
    except ValueError:
        return False
    return True


def graph(data: dict) -> tuple[list[str], dict[str, set[str]], dict[str, str]]:
    root = Path(data["workspace_root"]).resolve()
    packages = [
        package
        for package in data["packages"]
        if is_within(package["manifest_path"], root) and package.get("publish") != []
    ]
    nodes = {package["name"] for package in packages}
    versions = {package["name"]: package["version"] for package in packages}
    edges: dict[str, set[str]] = {}
    for package in packages:
        edges[package["name"]] = {
            dependency["name"]
            for dependency in package["dependencies"]
            if dependency.get("path") and is_within(dependency["path"], root)
        } & nodes
    return sorted(nodes), edges, versions


def order(nodes: list[str], edges: dict[str, set[str]]) -> list[str]:
    dependents = {node: set() for node in nodes}
    remaining = {node: len(edges[node]) for node in nodes}
    for dependent, dependencies in edges.items():
        for dependency in dependencies:
            dependents[dependency].add(dependent)
    ready = [node for node in nodes if remaining[node] == 0]
    heapq.heapify(ready)
    result: list[str] = []
    while ready:
        node = heapq.heappop(ready)
        result.append(node)
        for dependent in sorted(dependents[node]):
            remaining[dependent] -= 1
            if remaining[dependent] == 0:
                heapq.heappush(ready, dependent)
    if len(result) != len(nodes):
        raise ValueError("workspace publish dependencies contain a cycle")
    return result


def validate_order(nodes: list[str], edges: dict[str, set[str]], lines: list[str]) -> list[str]:
    """Return violations for a proposed order; accepts plain or emitted lines."""
    given: list[str] = []
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#") or line.startswith("wait-for-index "):
            continue
        if line.startswith("publish "):
            line = line.removeprefix("publish ")
        given.append(line.split("@", 1)[0])
    violations: list[str] = []
    expected = set(nodes)
    if len(given) != len(set(given)):
        violations.append("a crate appears more than once")
    for crate in sorted(expected - set(given)):
        violations.append(f"{crate} is missing from the order")
    for crate in sorted(set(given) - expected):
        violations.append(f"{crate} is not a publishable workspace crate")
    position = {crate: index for index, crate in enumerate(given)}
    for dependent, dependencies in edges.items():
        for dependency in dependencies:
            if position.get(dependency, -1) > position.get(dependent, -1):
                violations.append(f"{dependency} must precede {dependent}")
    return violations


def sparse_index_url(crate: str) -> str:
    name = crate.lower()
    if len(name) == 1:
        path = f"1/{name}"
    elif len(name) == 2:
        path = f"2/{name}"
    elif len(name) == 3:
        path = f"3/{name[0]}/{name}"
    else:
        path = f"{name[:2]}/{name[2:4]}/{name}"
    return f"https://index.crates.io/{path}"


def wait_for_index(crate: str, version: str, timeout: int, interval: float) -> int:
    deadline = time.monotonic() + timeout
    url = sparse_index_url(crate)
    while True:
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "penelope-release-ci"})
            with urllib.request.urlopen(request, timeout=30) as response:
                entries = response.read().decode("utf-8").splitlines()
            if any(
                entry.get("name", "").lower() == crate.lower() and entry.get("vers") == version
                for line in entries
                if (entry := json.loads(line))
            ):
                print(f"ok: {crate}@{version} is in the crates.io index")
                return 0
        except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
            print(f"retry: unable to check {crate}@{version}: {error}", file=sys.stderr)
        if time.monotonic() >= deadline:
            print(f"timeout: {crate}@{version} was not indexed after {timeout}s", file=sys.stderr)
            return 1
        time.sleep(interval)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata_json", nargs="?", help="cargo metadata JSON (default: run cargo metadata)")
    parser.add_argument("--emit", action="store_true", help="print ordered crate names only")
    parser.add_argument("--check", metavar="ORDER_FILE", help="validate an order file ('-' reads stdin)")
    parser.add_argument("--wait", nargs=2, metavar=("CRATE", "VERSION"), help="wait for an index entry")
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--interval", type=float, default=5)
    args = parser.parse_args()
    if args.wait:
        return wait_for_index(*args.wait, args.timeout, args.interval)
    try:
        nodes, edges, versions = graph(metadata(args.metadata_json))
        packages = order(nodes, edges)
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    if args.check:
        lines = sys.stdin.read().splitlines() if args.check == "-" else Path(args.check).read_text(encoding="utf-8").splitlines()
        violations = validate_order(nodes, edges, lines)
        if violations:
            print("NOT OK: the given order violates the dependency graph:", file=sys.stderr)
            print(*(f"  - {violation}" for violation in violations), sep="\n", file=sys.stderr)
            return 1
        print("OK: order is a valid dependency-first order")
    elif args.emit:
        print(*packages, sep="\n")
    else:
        print(*(f"publish {package}@{versions[package]}" for package in packages), sep="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
