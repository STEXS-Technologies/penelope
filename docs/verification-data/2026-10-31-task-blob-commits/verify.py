"""Check archive hashes, source pins, all runs, regressions and paired statistics."""
import gzip
import hashlib
import json
import pathlib
import random
import statistics

here = pathlib.Path(__file__).resolve().parent
manifest = json.loads((here / 'manifest.json').read_text())
for name, expected in manifest['sha256'].items():
    assert hashlib.sha256((here / name).read_bytes()).hexdigest() == expected, name
rows = [json.loads(line) for line in gzip.decompress((here / 'runs.jsonl.gz').read_bytes()).decode().splitlines()]
assert len(rows) == 48
for pair in range(24):
    group = rows[pair * 2:pair * 2 + 2]
    assert [r['pair'] for r in group] == [pair, pair]
    assert [r['variant'] for r in group] == (['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline'])
    for run in group:
        assert [r['size'] for r in run['rows']] == [0, 32, 4096, 1048576]
        assert all(r['iterations'] == 128 and r['elapsed_ns'] > 0 for r in run['rows'])
analysis = json.loads((here / 'analysis.json').read_text())
for result, size in zip(analysis, [0, 32, 4096, 1048576], strict=True):
    assert result['size'] == size and result['pairs'] == 24
    changes = []
    timings = {'baseline': [], 'candidate': []}
    for pair in range(24):
        values = {r['variant']: next(x['elapsed_ns'] / 128 for x in r['rows'] if x['size'] == size) for r in rows if r['pair'] == pair}
        changes.append(100 * (values['candidate'] / values['baseline'] - 1))
        for variant in timings:
            timings[variant].append(values[variant])
    rng = random.Random(3102026 + size)
    boot = sorted(statistics.mean(rng.choices(changes, k=24)) for _ in range(20000))
    assert result['change_percent'] == statistics.mean(changes)
    assert result['ci95'] == [boot[499], boot[19499]]
    for variant in timings:
        assert result[variant + '_ns'] == statistics.mean(timings[variant])
for compiler in ['stable', '1.85.0']:
    for repeat in range(10):
        log = gzip.decompress((here / f'repeat-{compiler}-{repeat:02}.log.gz').read_bytes()).decode()
        assert '2 passed; 0 failed' in log
for name in ['workspace', 'msrv', 'clippy']:
    log = gzip.decompress((here / (name + '.log.gz')).read_bytes()).decode()
    assert 'error:' not in log and 'test result: FAILED' not in log
for name in ['baseline', 'candidate']:
    assert hashlib.sha256(gzip.decompress((here / (name + '-tasks.rs.gz')).read_bytes())).hexdigest() == manifest['binary_and_source_hashes'][name + '-tasks.rs']
print('Verified 48 processes, 192 observations, 24,576 timed commits, four statistics and 20 regression runs.')
