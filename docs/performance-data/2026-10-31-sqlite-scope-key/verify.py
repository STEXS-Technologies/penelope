"""Independently reparse samples, ordering, summaries, source/build pins and hashes."""
from pathlib import Path
import gzip
import hashlib
import json
import random
import statistics

root = Path(__file__).resolve().parent
meta = json.loads((root / 'metadata.json').read_text())
rows = [json.loads(line) for line in gzip.decompress((root / 'processes.jsonl.gz').read_bytes()).decode().splitlines()]
benches = ['fence', 'fence-remote', 'dispatch']
expected = [(label, bench, -1, True) for label in ['baseline', 'candidate'] for bench in benches]
for pair in range(meta['pairs']):
    labels = ['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline']
    expected.extend((label, bench, pair, False) for label in labels for bench in benches)
assert len(rows) == len(expected) == 126
workers = 0
for row, order in zip(rows, expected):
    assert (row['label'], row['bench'], row['pair'], row['warmup']) == order
    assert row['stderr'] == '' and row['elapsed_ns'] > 0
    assert row['command'][:3] == ['taskset', '-c', '0']
    data = json.loads(row['stdout'])
    values = {}
    if row['bench'].startswith('fence'):
        assert data['samples_per_profile'] == meta['fence_samples'] == 32
        assert data['worker_cpu'] == (1 if row['bench'] == 'fence-remote' else None)
        assert set(data['profiles_ns']) == {'live', 'exited', 'repeated'}
        for name, samples in data['profiles_ns'].items():
            assert len(samples) == 32 and all(v > 0 for v in samples)
            values[row['bench'] + '/' + name] = statistics.median(samples)
            workers += len(samples)
    else:
        assert len(data['profiles']) == 2
        for profile in data['profiles']:
            workers += len(profile['samples'])
            samples = [r for r in profile['samples'] if not r['warmup']]
            assert len(samples) == meta['dispatch_samples'] == 16
            for field in ['setup_ns', 'intent_commit_ns', 'dispatch_start_return_ns', 'dispatch_to_host_checkpoint_ack_ns', 'dispatch_to_durable_completion_and_fence_ns']:
                values['dispatch/' + profile['profile'] + '/' + field] = statistics.median(r[field] for r in samples)
    assert row['median_ns'] == values
summary = {}
rng = random.Random(3030)
for key in sorted({key for row in rows for key in row['median_ns']}):
    grouped = {label: {r['pair']: r['median_ns'][key] for r in rows if r['label'] == label and not r['warmup'] and key in r['median_ns']} for label in ['baseline', 'candidate']}
    assert all(len(values) == 20 for values in grouped.values())
    changes = [100 * (grouped['candidate'][i] / grouped['baseline'][i] - 1) for i in range(20)]
    boot = sorted(statistics.median(rng.choices(changes, k=20)) for _ in range(10000))
    summary[key] = dict(baseline_median_ns=statistics.median(grouped['baseline'].values()), candidate_median_ns=statistics.median(grouped['candidate'].values()), paired_median_percent=statistics.median(changes), paired_bootstrap_95_percent=[boot[250], boot[9749]], all_paired_percent=changes)
assert summary == json.loads((root / 'summary.json').read_text())
evidence = json.loads((root / 'evidence.json').read_text())
assert workers == evidence['worker_fixtures_including_warmups']
for name, digest in evidence['artifact_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
builds = json.loads((root / 'builds.json').read_text())
for label, binaries in meta['binary_sha256'].items():
    for bench, digest in binaries.items():
        name = 'fence' if bench == 'fence-remote' else bench
        build = builds[f'target/sqlite-scope-latency-{label}-{name}']
        assert build['sha256'] == digest
        assert build['tasks_sha256'] == evidence['tasks_sha256'][label]
print(f'Verified {len(rows)} processes, {workers} worker fixtures, all 16 summaries, build pins and artifact hashes.')

allocation_rows = [json.loads(line) for line in gzip.decompress((root / 'allocations.jsonl.gz').read_bytes()).decode().splitlines()]
assert len(allocation_rows) == 40
assert {(r['variant'],r['toolchain'],r['iteration']) for r in allocation_rows} == {(v,t,i) for v in ['baseline','fixed'] for t in ['stable','1.85.0'] for i in range(10)}
for row in allocation_rows:
    assert row['returncode'] == 0 and row['counts'] == ([24]*3 if row['variant']=='baseline' else [16]*3)
    assert f"repeated public fences={row['counts']}" in row['stdout']
    label = 'candidate' if row['variant']=='fixed' else 'baseline'
    assert row['source_sha256'] == evidence['tasks_sha256'][label]
for compiler in ['workspace','msrv']:
    text = gzip.decompress((root / ('validation-'+compiler+'.log.gz')).read_bytes()).decode()
    assert 'test result: ok. 18 passed;' in text and 'test result: ok. 66 passed;' in text
    assert 'FAILED' not in text and 'error:' not in text
print('Verified 40 allocation probes / 120 fence counts and both full workspace suites.')
