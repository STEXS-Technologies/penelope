#!/usr/bin/env python3
"""Measure complete synthetic SQLite timer workflows; preserve every sample."""
import argparse
import datetime
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PHASES = ['schedule_ns', 'delivery_ns', 'reopen_ns', 'consume_ns']


def summarize(rows, metadata):
    expected = set(itertools.product([1, 16, 128], [9, 128], metadata['work'], [False, True]))
    groups = {}
    identities = set()
    for row in rows:
        key = (row['batch'], row['width'], row['work'], row['reopen'])
        identity = (*key, row['repeat'], row['round'])
        if key not in expected or identity in identities:
            raise ValueError('unexpected or repeated sample')
        identities.add(identity)
        if not (0 <= row['repeat'] < metadata['repeats'] and 0 <= row['round'] < metadata['rounds']):
            raise ValueError('sample index outside bounds')
        if not (row['schema'] == 1 and row['verified'] is True and row['consumed'] == row['batch'] and row['outcomes'] == row['batch']+1):
            raise ValueError('unverified workflow')
        if row['direct_first'] != (row['round'] % 2 == 0):
            raise ValueError('unbalanced order')
        if any(type(row[name]) is not int or row[name] < 0 for name in PHASES+['direct_ns', 'managed_ns']):
            raise ValueError('invalid timing')
        if row['managed_ns'] != sum(row[name] for name in PHASES) or row['direct_ns'] <= 0:
            raise ValueError('invalid phase sum')
        if (row['reopen_ns'] > 0) != row['reopen']:
            raise ValueError('invalid reopen timing')
        groups.setdefault(key, []).append(row)
    if set(groups) != expected or any(len(group) != metadata['repeats']*metadata['rounds'] for group in groups.values()):
        raise ValueError('incomplete profile matrix')
    summaries = []
    for (batch, width, work, reopen), group in sorted(groups.items()):
        if len({row['checksum'] for row in group}) != 1:
            raise ValueError('unstable business output')
        summaries.append(dict(batch=batch, width=width, work=work, reopen=reopen,
                              samples=len(group),
                              median_direct_ns=statistics.median(row['direct_ns'] for row in group),
                              median_managed_ns=statistics.median(row['managed_ns'] for row in group),
                              median_added_ns_per_timer=statistics.median((row['managed_ns']-row['direct_ns'])/batch for row in group),
                              median_slowdown_pct=statistics.median(100*(row['managed_ns']/row['direct_ns']-1) for row in group),
                              phase_medians_ns={name:statistics.median(row[name] for row in group) for name in PHASES}))
    return dict(profiles=summaries, sample_count=len(rows), timer_count=sum(row['batch'] for row in rows))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'target'/('durable-timers-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')))
    parser.add_argument('--repeats', type=int, default=6)
    parser.add_argument('--rounds', type=int, default=2)
    parser.add_argument('--work', default='0,65536')
    args = parser.parse_args()
    work = [int(value) for value in args.work.split(',')]
    if not 2 <= args.repeats <= 32 or not 2 <= args.rounds <= 32 or args.rounds % 2:
        parser.error('need 2..32 repeats and even 2..32 rounds')
    if not work or len(set(work)) != len(work) or any(not 0 <= value <= 16777216 for value in work):
        parser.error('work must be distinct values within 0..16777216')
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    (output/'databases').mkdir()
    env = dict(os.environ, RUSTC_WRAPPER='', RUSTC_WORKSPACE_WRAPPER='', TMPDIR=str(output/'databases'))
    command = ['cargo', 'build', '--release', '--locked', '-p', 'penelope-executor', '--bench', 'durable_timer_workload', '--message-format=json']
    with (output/'build.txt').open('w') as log:
        built = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=log, text=True, check=True)
    binary = next(Path(item['executable']) for line in built.stdout.splitlines() if (item := json.loads(line)).get('executable') and item.get('target', {}).get('name') == 'durable_timer_workload')
    cpu = min(os.sched_getaffinity(0))
    sources = [Path(__file__), ROOT/'crates/penelope-executor/benches/durable_timer_workload.rs',
               *sorted((ROOT/'crates/penelope-executor/benches/support/timer_workload').glob('*.rs')),
               ROOT/'crates/penelope-executor/tests/support/timer_input_legacy.rs', ROOT/'crates/penelope-executor/Cargo.toml', ROOT/'Cargo.lock']
    metadata = dict(schema=1, git_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                    rustc=subprocess.check_output(['rustc', '-Vv'], text=True), platform=platform.platform(), cpu=cpu,
                    filesystem=subprocess.check_output(['findmnt', '-T', str(output), '-n', '-o', 'FSTYPE,SOURCE'], text=True).strip(),
                    cpu_model=next(line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')),
                    rustflags=env.get('RUSTFLAGS', ''), repeats=args.repeats, rounds=args.rounds, work=work,
                    binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                    source_sha256={str(path.relative_to(ROOT)):hashlib.sha256(path.read_bytes()).hexdigest() for path in sources},
                    scope='synthetic serial complete durable timer workflows; phase sum excludes initial open and final verification; not universal overhead')
    (output/'metadata.json').write_text(json.dumps(metadata, indent=2)+'\n')
    configs = list(itertools.product([1,16,128], [9,128], work, [False,True]))
    rows = []
    # All builds and checks precede ordinary timing; no concurrent analysis.
    with (output/'samples.jsonl').open('w') as log:
        for repeat in range(args.repeats):
            for batch,width,units,reopen in configs[::(-1 if repeat % 2 else 1)]:
                command = ['taskset','-c',str(cpu),str(binary),'--batch',str(batch),'--width',str(width),'--work',str(units),'--rounds',str(args.rounds),'--reopen',str(reopen).lower()]
                result = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True, check=True)
                samples = [json.loads(line) for line in result.stdout.splitlines()]
                if len(samples) != args.rounds: raise ValueError('incomplete process output')
                for sample in samples:
                    sample['repeat'] = repeat; rows.append(sample); log.write(json.dumps(sample)+'\n')
                log.flush()
            print(f'repeat={repeat} complete', flush=True)
    (output/'summary.json').write_text(json.dumps(summarize(rows, metadata), indent=2)+'\n')
    print(f'Verified {len(rows)} workflows: {output}')


if __name__ == '__main__':
    main()
