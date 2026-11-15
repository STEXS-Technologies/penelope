#!/usr/bin/env python3
"""Seed journal campaigns with quota, duplicate, reopen and history sequences."""
import argparse
import hashlib
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('fuzz/corpus'))
    root = parser.parse_args().root
    directory = root / 'fuzz_sqlite_machine'
    directory.mkdir(parents=True, exist_ok=True)
    for amount in (0, 255):
        for base in (0, 8):
            operations = [(0, amount, base + index) for index in range(5)]
            operations += [(3, 0, 0), (2, 0, 0), (6, 0, 0), (7, 0, 0)]
            operations += [(1, index, 0) for index in range(4)]
            operations += [(4, base + index, 0) for index in range(5)]
            operations += [(5, index, index) for index in range(5)]
            data = bytes(value for operation in operations for value in operation)
            path = directory / ('penelope-journal-v1-' + hashlib.sha256(data).hexdigest())
            try:
                with path.open('xb') as output:
                    output.write(data)
            except FileExistsError:
                if path.read_bytes() != data:
                    raise ValueError(f'conflicting deterministic seed: {path}') from None
    print('fuzz_sqlite_machine: 4 deterministic deep journal sequences')
    directory = root / 'fuzz_sqlite_machine_outbox'
    directory.mkdir(parents=True, exist_ok=True)
    sequences = [
        [(0, 0), (1, 10), (0, 2), (2, 0), (3, 0), (3, 2), (4, 0), (3, 2)],
        [(0, 1), (0, 0), (1, 10), (0, 0), (1, 10), (0, 0), (1, 10), (0, 0)],
        [(0, 0), (3, 0), (1, 15), (4, 0), (3, 0), (0, 0), (5, 0), (6, 0)],
    ]
    for operations in sequences:
        data = bytes(value for operation in operations for value in operation)
        path = directory / ('penelope-outbox-v1-' + hashlib.sha256(data).hexdigest())
        if path.exists() and path.read_bytes() != data:
            raise ValueError(f'conflicting deterministic seed: {path}')
        path.write_bytes(data)
    print('fuzz_sqlite_machine_outbox: 3 deterministic lease/expiry/acceptance sequences')
    directory = root / 'fuzz_sqlite_machine_task'
    directory.mkdir(parents=True, exist_ok=True)
    sequences = [
        [(2, 0), (4, 0), (4, 0), (1, 10), (0, 0), (2, 1), (3, 1), (5, 0)],
        [(2, 0), (5, 0), (2, 0), (3, 0), (1, 15), (3, 0), (6, 0), (7, 0)],
        [(7, 0), (2, 0), (1, 10), (0, 0), (6, 1), (4, 0), (2, 1), (3, 1)],
    ]
    for operations in sequences:
        data = bytes(value for operation in operations for value in operation)
        path = directory / ('penelope-task-handoff-v1-' + hashlib.sha256(data).hexdigest())
        if path.exists() and path.read_bytes() != data:
            raise ValueError(f'conflicting deterministic seed: {path}')
        path.write_bytes(data)
    print('fuzz_sqlite_machine_task: 3 deterministic native admission/recovery sequences')



if __name__ == '__main__':
    main()
