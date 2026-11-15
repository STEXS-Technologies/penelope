#!/usr/bin/env python3
"""Independent struct/SHA-256 machine-v1 encoder; checks frozen fixtures by default.

--write is for initial fixture creation or an explicitly reviewed new protocol
version. Never regenerate v1 fixtures merely to make a changed Rust codec pass.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "crates/penelope-core/tests/fixtures/machine-v1"
MAX = (1 << 64) - 1


def number(value):
    return struct.pack("<Q", value)


def identity(value):
    return bytes([value]) * 32


def encode(caused):
    """Encode the field layout in docs/0.5/machine-wire.md, without Rust calls."""
    before = MAX - 1 if caused else 0
    after = MAX if caused else 1
    data = b"penelope.machine.v1" + bytes([1])
    data += identity(1) + identity(2) + identity(3)
    data += struct.pack("<II", 0x01020304, 0xA0B0C0D0) + number(before)
    data += identity(4) + identity(5)
    data += number(MAX if caused else 0x0102030405060708)
    data += number(after) + number(0x8877665544332211)
    data += number(0x1020304050607080) + identity(6) + identity(7)
    data += bytes([int(caused)])
    if caused:
        data += identity(8) + identity(9) + identity(10)
    intents = [(11, 12, 0), (13, 14, MAX)] if caused else []
    data += struct.pack("<I", len(intents))
    for intent, digest, charge in intents:
        data += identity(intent) + identity(digest) + number(charge)
    return data + hashlib.sha256(data).digest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    manifest = {"protocol": "penelope.machine.v1", "encoder": "Python struct + hashlib; no Rust invocation", "fixtures": {}}
    for name, caused in [("empty.bin", False), ("caused.bin", True)]:
        data = encode(caused)
        manifest["fixtures"][name] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "commit_identity": data[-32:].hex()}
        if args.write:
            DEST.mkdir(parents=True, exist_ok=True)
            (DEST / name).write_bytes(data)
        elif (DEST / name).read_bytes() != data:
            raise SystemExit(f"independent machine-v1 fixture mismatch: {name}")
    manifest_path = DEST / "manifest.json"
    if args.write:
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    elif json.loads(manifest_path.read_text()) != manifest:
        raise SystemExit("machine-v1 fixture manifest mismatch")
    print("machine-v1 independent fixtures verified: empty=329 bytes, caused=569 bytes")


if __name__ == "__main__":
    main()
