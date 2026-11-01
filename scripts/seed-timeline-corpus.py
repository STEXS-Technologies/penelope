#!/usr/bin/env python3
"""Seed bounded timeline campaigns with deterministic deep/boundary traces.

Hash-named files are added without replacing any existing libFuzzer input.
"""
from __future__ import annotations
import argparse
import hashlib
import random
from pathlib import Path

TARGETS = ('projection', 'replay_plan', 'correction', 'branch_promotion', 'effects')

def seeds() -> dict[str, set[bytes]]:
    result: dict[str, set[bytes]] = {name: {b''} for name in TARGETS}
    for seed in range(16):
        rng = random.Random(seed)
        trace = bytes(rng.randrange(256) for _ in range(128))
        for name in TARGETS:
            result[name].add(trace)
    # Long controller traces exercise full slots, stale reservations, promotion,
    # restore and repeated finality/authority operations before mutation begins.
    for operation in range(8):
        for parameter in (0, 1, 15, 255):
            result['projection'].add(bytes([0, 0] + [operation, parameter] * 63))
    result['projection'].add(bytes([0, 7, 3, 0, 3, 0, 4, 0, 4, 1, 6, 0, 3, 0] * 9))
    for kind in range(2):
        for boundary in (0, 7, 15):
            for mutation in range(16):
                for role in range(4) if mutation == 2 else (0,):
                    result['replay_plan'].add(bytes([15, kind, boundary, mutation, role]))
            for failure in range(6):
                for offset in (0, 7, 16):
                    data = bytearray(range(64))
                    data[:7] = bytes([15, 9, kind, boundary, 219, failure, offset])
                    result['correction'].add(bytes(data))
    for mode in range(6):
        data = bytearray(range(64))
        data[:4] = bytes([15, 127, 219, mode])
        result['branch_promotion'].add(bytes(data))
    for parameter in range(128):
        # n=16, independent finality, bounded partial handoff with every failure
        # mode/index, then identical successful retries and duplicate finality.
        result['effects'].add(bytes([15, 0, 0, 16, 1, parameter] + [1, 4] * 16 + [0, 16]))
    return result

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('fuzz/corpus'))
    args = parser.parse_args()
    for name, inputs in seeds().items():
        directory = args.root / f'fuzz_timeline_{name}'
        directory.mkdir(parents=True, exist_ok=True)
        added = 0
        for data in sorted(inputs):
            path = directory / ('penelope-v1-' + hashlib.sha256(data).hexdigest())
            try:
                with path.open('xb') as output:
                    output.write(data)
                added += 1
            except FileExistsError:
                if path.read_bytes() != data:
                    raise ValueError(f'conflicting deterministic seed: {path}') from None
        print(f'{directory.name}: {len(inputs)} deterministic seeds ({added} added)')

if __name__ == '__main__':
    main()
