"""Reparse retained stdout and independently reproduce all paired summaries."""
import gzip
from collections import Counter
import hashlib
import json
from pathlib import Path
import random
import statistics

ROOT = Path(__file__).resolve().parent
processes = 0
workers = 0
for name in ['initial', 'affinity-pilot', 'final']:
    folder = ROOT / name
    meta = json.loads((folder / 'metadata.json').read_text())
    records = [json.loads(line) for line in gzip.decompress((folder / 'processes.jsonl.gz').read_bytes()).decode().splitlines()]
    benches = ['fence', 'dispatch'] if name == 'initial' else ['fence', 'fence-remote', 'dispatch']
    expected = [(label, bench, -1, True) for label in ['baseline', 'candidate'] for bench in benches]
    for pair in range(meta['pairs']):
        labels = ['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline']
        expected.extend((label, bench, pair, False) for label in labels for bench in benches)
    assert len(records) == len(expected)
    for row, order in zip(records, expected):
        assert (row['label'], row['bench'], row['pair'], row['warmup']) == order
        data = json.loads(row['stdout'])
        values = {}
        if row['bench'].startswith('fence'):
            assert data['samples_per_profile'] == meta['fence_samples']
            assert data.get('worker_cpu') == (1 if row['bench'] == 'fence-remote' else None)
            assert set(data['profiles_ns']) == {'live', 'exited', 'repeated'}
            for key, samples in data['profiles_ns'].items():
                assert len(samples) == meta['fence_samples'] and all(v > 0 for v in samples)
                values[row['bench'] + '/' + key] = statistics.median(samples)
                workers += len(samples)
        else:
            assert len(data['profiles']) == 2
            for profile in data['profiles']:
                workers += len(profile['samples'])
                samples = [r for r in profile['samples'] if not r['warmup']]
                assert len(samples) == meta['dispatch_samples']
                for field in ['setup_ns', 'intent_commit_ns', 'dispatch_start_return_ns', 'dispatch_to_host_checkpoint_ack_ns', 'dispatch_to_durable_completion_and_fence_ns']:
                    values['dispatch/' + profile['profile'] + '/' + field] = statistics.median(r[field] for r in samples)
        assert values == row['median_ns']
    summary = {}
    rng = random.Random(3030)
    for key in sorted({key for row in records for key in row['median_ns']}):
        grouped = {label: {r['pair']:r['median_ns'][key] for r in records if r['label']==label and not r['warmup'] and key in r['median_ns']} for label in ['baseline', 'candidate']}
        assert all(len(v)==meta['pairs'] for v in grouped.values())
        changes = [100*(grouped['candidate'][i]/grouped['baseline'][i]-1) for i in range(meta['pairs'])]
        boot = sorted(statistics.median(rng.choices(changes, k=len(changes))) for _ in range(10000))
        summary[key] = dict(baseline_median_ns=statistics.median(grouped['baseline'].values()), candidate_median_ns=statistics.median(grouped['candidate'].values()), paired_median_percent=statistics.median(changes), paired_bootstrap_95_percent=[boot[250],boot[9749]], all_paired_percent=changes)
    assert summary == json.loads((folder / 'summary.json').read_text())
    processes += len(records)
manifest = json.loads((ROOT / 'evidence.json').read_text())
for name, digest in manifest['artifact_sha256'].items():
    assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest, name
for tool in ['stable', 'msrv']:
    records = [json.loads(line) for line in gzip.decompress((ROOT / 'validation' / ('repeat-'+tool+'.jsonl.gz')).read_bytes()).decode().splitlines()]
    assert len(records) == 450
    counts = Counter(r["command"][2] for r in records)
    assert len(counts) == 9 and set(counts.values()) == {50}
    for index, row in enumerate(records):
        assert row['iteration']==index//9 and row['returncode']==0
        assert 'test result: ok. 1 passed; 0 failed;' in row['stdout']
print(f'Verified {processes} benchmark processes, {workers} worker fixtures including warmups, 900 repeated test processes, all summaries and artifact hashes.')
