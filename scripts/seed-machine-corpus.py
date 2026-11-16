#!/usr/bin/env python3
"""Seed journal campaigns with quota, duplicate, reopen and history sequences."""
import argparse
import hashlib
from pathlib import Path


def write_seed(directory: Path, prefix: str, data: bytes) -> None:
    path = directory / (prefix + hashlib.sha256(data).hexdigest())
    try:
        with path.open('xb') as output:
            output.write(data)
    except FileExistsError:
        if path.read_bytes() != data:
            raise ValueError(f'conflicting deterministic seed: {path}') from None


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
            write_seed(directory, 'penelope-journal-v1-', data)
    for role in range(6):
        operations = [(0, 0, 1), (0, 1, 2), (4, 1, role), (1, 3, 0),
                      (0, 128, 3), (0, 2, 1), (3, 0, 0), (5, 3, 4), (2, 0, 0)]
        data = bytes(value for operation in operations for value in operation)
        write_seed(directory, 'penelope-replay-v1-', data)
    print('fuzz_sqlite_machine: 10 deep journal/replay mutation sequences')
    directory = root / 'fuzz_sqlite_machine_outbox'
    directory.mkdir(parents=True, exist_ok=True)
    sequences = [
        [(0, 0), (1, 10), (0, 2), (2, 0), (3, 0), (3, 2), (4, 0), (3, 2)],
        [(0, 1), (0, 0), (1, 10), (0, 0), (1, 10), (0, 0), (1, 10), (0, 0)],
        [(0, 0), (3, 0), (1, 15), (4, 0), (3, 0), (0, 0), (5, 0), (6, 0)],
    ]
    for operations in sequences:
        data = bytes(value for operation in operations for value in operation)
        write_seed(directory, 'penelope-outbox-v1-', data)
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
        write_seed(directory, 'penelope-task-handoff-v1-', data)
    print('fuzz_sqlite_machine_task: 3 deterministic native admission/recovery sequences')
    directory = root / 'fuzz_sqlite_machine_saga'
    directory.mkdir(parents=True, exist_ok=True)
    sequences = [
        [(2, 0), (4, 0), (4, 0), (1, 10), (0, 0), (2, 1), (3, 1), (5, 0)],
        [(2, 0), (5, 0), (2, 0), (3, 0), (1, 15), (3, 0), (6, 0), (7, 0)],
        [(6, 1), (2, 0), (6, 0), (2, 0), (7, 0), (1, 10), (0, 0), (2, 1), (3, 1), (4, 0)],
    ]
    for operations in sequences:
        data = bytes(value for operation in operations for value in operation)
        write_seed(directory, 'penelope-saga-handoff-v1-', data)
    print('fuzz_sqlite_machine_saga: 3 deterministic native admission/recovery sequences')


if __name__ == '__main__':
    main()
