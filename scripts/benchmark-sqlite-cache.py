#!/usr/bin/env python3
"""Verify owned SQLite file-page residency before paired fresh-process replay."""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import mmap
import os
from pathlib import Path
import platform
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
LENGTHS = (16, 128, 512)


def command(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def residency(descriptor, size):
    """Inspect page-cache residency without faulting in the mapped data."""
    if size <= 0:
        raise ValueError("empty cache dataset")
    page_size = os.sysconf("SC_PAGE_SIZE")
    pages = (size + page_size - 1) // page_size
    library = ctypes.CDLL(None, use_errno=True)
    library.mincore.argtypes = (ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p)
    library.mincore.restype = ctypes.c_int
    vector = (ctypes.c_ubyte * pages)()
    with mmap.mmap(descriptor, size, flags=mmap.MAP_PRIVATE,
                   prot=mmap.PROT_READ | mmap.PROT_WRITE) as mapping:
        anchor = ctypes.c_char.from_buffer(mapping)
        address = ctypes.addressof(anchor)
        try:
            if library.mincore(address, size, vector) != 0:
                number = ctypes.get_errno()
                raise OSError(number, os.strerror(number))
        finally:
            del anchor
    return {"resident_pages": sum(bool(value & 1) for value in vector),
            "total_pages": pages, "page_size": page_size, "file_bytes": size}


def set_cache(path, mode):
    if platform.system() != "Linux" or mode not in ("cold", "warm"):
        raise ValueError("Linux and an explicit cold/warm mode are required")
    # Only private benchmark datasets are supplied by main; refuse symlink leaves.
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("cache dataset must be a regular file")
        for extension in ("-wal", "-shm"):
            sidecar = Path(str(path) + extension)
            if sidecar.exists() and sidecar.stat().st_size:
                raise ValueError("close all SQLite connections before cache control")
        before = residency(descriptor, info.st_size)
        if mode == "cold":
            os.fdatasync(descriptor)
            os.posix_fadvise(descriptor, 0, 0, os.POSIX_FADV_DONTNEED)
        else:
            offset = 0
            while offset < info.st_size:
                data = os.pread(descriptor, min(1024 * 1024, info.st_size - offset), offset)
                if not data:
                    raise ValueError("dataset truncated while warming")
                offset += len(data)
        after = residency(descriptor, info.st_size)
        expected = 0 if mode == "cold" else after["total_pages"]
        if after["resident_pages"] != expected:
            raise RuntimeError(f"requested file cache state not established: {mode}: {after}")
        return {"requested": mode, "before": before, "verified_before_launch": after}
    finally:
        os.close(descriptor)


def validate_report(report, length):
    if (report.get("schema") != 1 or report.get("records") != length
            or report.get("journal_mode") != "WAL" or report.get("synchronous") != "FULL"
            or type(report.get("open_and_replay_ns")) is not int
            or report["open_and_replay_ns"] <= 0):
        raise ValueError("invalid replay timing, dataset or durability profile")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--data-parent", type=Path, required=True)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if (platform.system() != "Linux" or args.runs < 1
            or args.cpu not in os.sched_getaffinity(0) or not args.data_parent.is_dir()):
        parser.error("Linux, positive runs, available CPU and existing data-parent required")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    build = command("cargo", "+1.99.0", "bench", "-p", "penelope-sqlite", "--bench",
                    "durable_store", "--locked", "--no-run", "--message-format=json",
                    cwd=ROOT, capture_output=True)
    (output / "build.jsonl").write_text(build.stdout)
    (output / "build.stderr").write_text(build.stderr)
    binaries = [entry["executable"] for line in build.stdout.splitlines()
                if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry["target"]["name"] == "durable_store" and entry.get("executable")]
    if len(binaries) != 1:
        raise ValueError("expected one durable_store executable")
    binary = binaries[0]
    metadata = {"schema": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
        "revision": command("git", "rev-parse", "HEAD", cwd=ROOT, capture_output=True).stdout.strip(),
        "tracked_diff": command("git", "diff", "HEAD", cwd=ROOT, capture_output=True).stdout,
        "runner_sha256": digest(Path(__file__)),
        "benchmark_sha256": digest(ROOT / "crates/penelope-sqlite/benches/durable_store.rs"),
        "binary_sha256": digest(Path(binary)), "lock_sha256": digest(ROOT / "Cargo.lock"),
        "rustc": command("rustc", "+1.99.0", "-vV", capture_output=True).stdout,
        "platform": platform.platform(), "cpu": args.cpu, "runs": args.runs,
        "filesystem": json.loads(command("findmnt", "-J", "-T", str(args.data_parent.resolve()),
                                         capture_output=True).stdout),
        "cache_scope": "main database file pages verified immediately before child launch; not device, executable, metadata or whole-system caches",
        "sync_policy": "SQLite WAL synchronous=FULL; no fsync bypass",
        "build_environment": {key: os.environ.get(key) for key in
                              ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER")}}
    records = []
    with tempfile.TemporaryDirectory(prefix="penelope-cache-", dir=args.data_parent.resolve()) as temp:
        directory = Path(temp)
        prepared = command("taskset", "-c", str(args.cpu), binary, "--cache-prepare", temp,
                           capture_output=True)
        (output / "prepare.stdout").write_text(prepared.stdout)
        (output / "prepare.stderr").write_text(prepared.stderr)
        datasets = {length: directory / f"tasks-{length}.db" for length in LENGTHS}
        hashes = {length: digest(path) for length, path in datasets.items()}
        metadata["datasets"] = {str(length): {"sha256": hashes[length], "bytes": path.stat().st_size}
                                for length, path in datasets.items()}
        (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        for run in range(1, args.runs + 1):
            for length, path in datasets.items():
                for mode in (("cold", "warm") if run % 2 else ("warm", "cold")):
                    cache = set_cache(path, mode)
                    # Do not read/hash the database between verification and launch.
                    prefix = output / f"run-{run}-{length}-{mode}"
                    measured = command("/usr/bin/time", "-f", "%e %U %S %M", "-o",
                        str(prefix.with_suffix(".time")), "taskset", "-c", str(args.cpu), binary,
                        "--cache-replay", str(path), str(length), capture_output=True)
                    prefix.with_suffix(".stdout").write_text(measured.stdout)
                    prefix.with_suffix(".stderr").write_text(measured.stderr)
                    report = json.loads(measured.stdout)
                    validate_report(report, length)
                    if digest(path) != hashes[length]:
                        raise ValueError("replay changed authoritative dataset bytes")
                    records.append({"run": run, "mode": mode, "cache": cache, **report})
                    (output / "measurements.json").write_text(json.dumps(records, indent=2) + "\n")
            print(f"Recorded paired cache run {run}", flush=True)


if __name__ == "__main__":
    main()
