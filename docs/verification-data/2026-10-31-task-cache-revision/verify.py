"""Verify source/archive integrity, regression results and all paired timings."""
import gzip
import hashlib
import itertools
import json
import pathlib
import random
import statistics

here = pathlib.Path(__file__).resolve().parent
manifest = json.loads((here / 'manifest.json').read_text())
for name, expected in manifest['sha256'].items():
    assert hashlib.sha256((here / name).read_bytes()).hexdigest() == expected, name

def unpack(name):
    return gzip.decompress((here / (name + '.gz')).read_bytes()).decode()

baseline = unpack('baseline-regression.log').split("\nthread '", 1)[0]
observations = [line[line.index('mode='):] for line in baseline.splitlines() if 'mode=' in line and ' accepted=' in line]
assert len(observations) == 48
assert sum('accepted=true' in line for line in observations) == 30
assert sum('unchanged=false' in line for line in observations) == 30
assert 'test result: FAILED' in unpack('baseline-regression.log')
for compiler in ['stable', '1.85.0']:
    for repeat in range(10):
        log = unpack(f'repeat-{compiler}-{repeat:02}.log')
        observations = [line[line.index('mode='):] for line in log.splitlines() if 'mode=' in line and ' accepted=' in line]
        assert len(observations) == 48
        assert all('accepted=false unchanged=true' in line for line in observations)
        assert '3 passed; 0 failed' in log
for name in ['workspace', 'msrv', 'clippy']:
    log = unpack(name + '.log')
    assert 'error:' not in log and 'test result: FAILED' not in log
for name in ['baseline', 'uncached', 'cached']:
    data = gzip.decompress((here / (name + '-tasks.rs.gz')).read_bytes())
    assert hashlib.sha256(data).hexdigest() == manifest['binary_and_source_hashes'][name + '-tasks.rs']

runs = [json.loads(line) for line in unpack('runs.jsonl').splitlines()]
assert len(runs) == 144
orders = list(itertools.permutations(['baseline', 'uncached', 'cached']))
for block in range(24):
    group = runs[block * 6:block * 6 + 6]
    profiles = [False, True] if block % 2 == 0 else [True, False]
    assert [r['block'] for r in group] == [block] * 6
    assert [r['external_commit'] for r in group] == [profiles[0]] * 3 + [profiles[1]] * 3
    assert [r['variant'] for r in group] == list(orders[block % 6]) * 2
    for run in group:
        external = run['external_commit']
        assert [r['size'] for r in run['rows']] == ([0, 32, 4096] if external else [0, 32, 4096, 1048576])
        assert all(r['external_commit'] == external and r['iterations'] == 128 and r['elapsed_ns'] > 0 for r in run['rows'])
assert sum(len(run['rows']) for run in runs) == 504
analysis = json.loads((here / 'analysis.json').read_text())
assert len(analysis) == 21
index = 0
for external in [False, True]:
    for size in ([0, 32, 4096] if external else [0, 32, 4096, 1048576]):
        for comparison, (before, after) in enumerate([('baseline', 'uncached'), ('baseline', 'cached'), ('uncached', 'cached')]):
            result = analysis[index]
            index += 1
            assert (result['external_commit'], result['size'], result['before'], result['after'], result['blocks']) == (external, size, before, after, 24)
            changes = []
            times = {before: [], after: []}
            for block in range(24):
                values = {r['variant']: next(x['elapsed_ns'] / 128 for x in r['rows'] if x['size'] == size) for r in runs if r['block'] == block and r['external_commit'] == external}
                changes.append(100 * (values[after] / values[before] - 1))
                for variant in times:
                    times[variant].append(values[variant])
            rng = random.Random(3102026 + size + 10000000 * external + 100000000 * comparison)
            boot = sorted(statistics.mean(rng.choices(changes, k=24)) for _ in range(20000))
            assert result['change_percent'] == statistics.mean(changes)
            assert result['ci95'] == [boot[499], boot[19499]]
            assert result['before_ns'] == statistics.mean(times[before])
            assert result['after_ns'] == statistics.mean(times[after])
print('Verified 30 baseline false ACKs, 960 final damage cases, 144 processes, 64,512 timed commits and 21 statistics.')
