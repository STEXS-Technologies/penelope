#!/usr/bin/env python3
"""Record sampled whole-benchmark stack usage; not a universal static bound."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def parse_massif(text):
    snapshots = []
    current = None
    for line in text.splitlines():
        if line.startswith('snapshot='):
            if current is not None:
                snapshots.append(current)
            current = {'snapshot': int(line.split('=', 1)[1])}
        elif current is not None and line.startswith(('mem_heap_B=', 'mem_heap_extra_B=', 'mem_stacks_B=')):
            key, value = line.split('=', 1)
            if key in current or int(value) < 0:
                raise ValueError('duplicate or negative Massif memory field')
            current[key] = int(value)
    if current is not None:
        snapshots.append(current)
    keys = {'snapshot', 'mem_heap_B', 'mem_heap_extra_B', 'mem_stacks_B'}
    if not snapshots or any(set(row) != keys for row in snapshots):
        raise ValueError('missing complete Massif snapshots')
    return {'snapshots': len(snapshots),
            'maximum_sampled_stack_bytes': max(row['mem_stacks_B'] for row in snapshots),
            'maximum_sampled_total_bytes': max(row['mem_heap_B'] + row['mem_heap_extra_B'] + row['mem_stacks_B'] for row in snapshots)}


def run(arguments, **kwargs):
    return subprocess.run(arguments, cwd=ROOT, check=True, text=True, capture_output=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    if any(os.environ.get(key) for key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER')):
        parser.error('custom compiler settings would confound this profile')
    build = run(['cargo', 'bench', '-p', 'penelope-timeline', '--bench', 'timeline_allocations', '--no-run', '--locked', '--message-format=json'])
    (output / 'build.jsonl').write_text(build.stdout)
    (output / 'build.stderr').write_text(build.stderr)
    artifacts = [row['executable'] for line in build.stdout.splitlines()
                 if (row := json.loads(line)).get('reason') == 'compiler-artifact'
                 and row.get('executable') and row['target']['name'] == 'timeline_allocations']
    if len(artifacts) != 1:
        raise ValueError('expected exactly one timeline benchmark executable')
    control = output / 'stack-control'
    source = ROOT / 'scripts/fixtures/timeline_stack_control.rs'
    run(['rustc', '--edition=2024', '-O', str(source), '-o', str(control)])
    results = {}
    for name, executable in [('control', str(control)), ('timeline', artifacts[0])]:
        result = run(['valgrind', '--tool=massif', '--stacks=yes', '--time-unit=B', '--detailed-freq=1',
                      '--max-snapshots=1000', '--massif-out-file=' + str(output / f'{name}.massif'), executable])
        (output / f'{name}.stdout').write_text(result.stdout)
        (output / f'{name}.stderr').write_text(result.stderr)
        results[name] = parse_massif((output / f'{name}.massif').read_text())
        results[name]['executable_sha256'] = hashlib.sha256(Path(executable).read_bytes()).hexdigest()
    if results['control']['maximum_sampled_stack_bytes'] < 65_536:
        raise ValueError('Massif missed the 64 KiB stack positive control')
    if results['timeline']['maximum_sampled_stack_bytes'] == 0:
        raise ValueError('no timeline stack samples')
    metadata = {'results': results, 'revision': run(['git', 'rev-parse', 'HEAD']).stdout.strip(),
                'rustc': run(['rustc', '-vV']).stdout, 'valgrind': run(['valgrind', '--version']).stdout.strip(),
                'platform': platform.platform(), 'scope': 'sampled entire hosted allocation benchmark, including fixture construction and reporting; all state sizes/horizons; no formal worst-case bound',
                'limitations': ['does not attribute stack to an individual operation', 'sampling can miss transient peaks', 'compiled red-zone usage and arbitrary consumer callbacks are not bounded', 'not a portable no_std call-chain bound']}
    (output / 'summary.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps(results))


if __name__ == '__main__':
    main()
