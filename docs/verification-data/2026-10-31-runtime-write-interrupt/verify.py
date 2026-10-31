"""Check retained syscall traces, successful test output, fixtures and artifact pins."""
from pathlib import Path
import gzip
import hashlib
import json
import re

root = Path(__file__).resolve().parent
meta = json.loads((root / 'evidence.json').read_text())
for name, digest in meta['artifact_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
rows = [json.loads(line) for line in gzip.decompress((root / 'processes.jsonl.gz').read_bytes()).decode().splitlines()]
traces = {r['name']: r['text'] for r in (json.loads(line) for line in gzip.decompress((root / 'traces.jsonl.gz').read_bytes()).decode().splitlines())}
assert len(rows) == len(traces) == 60
expected = {(variant, compiler, iteration, case) for variant in ['baseline', 'fixed'] for compiler in ['stable', '1.85.0'] for iteration in range(5) for case in ['interrupted', 'wouldblock', 'deadline']}
assert {(r['variant'], r['toolchain'], r['iteration'], r['case']) for r in rows} == expected
for row in rows:
    assert row['returncode'] == 0 and 'test result: ok. 1 passed; 0 failed;' in row['stdout']
    assert row['source_sha256'] == meta['runtime_sha256'][row['variant']]
    calls = len(re.findall(r'\b(?:clock_nanosleep|nanosleep)\(', traces[row['trace']]))
    assert calls == row['sleep_calls']
    if row['case'] == 'wouldblock':
        assert calls == 8
    elif row['variant'] == 'fixed':
        assert calls == 0
    elif row['case'] == 'interrupted':
        assert calls == 16
    else:
        assert calls > 0
    if row['case'] == 'deadline':
        assert int(re.search(r'interrupted attempts before deadline: (\d+)', row['stdout'])[1]) > 0
for variant in ['baseline', 'fixed']:
    for compiler in ['stable', '1.85.0']:
        group = [r for r in rows if r['variant'] == variant and r['toolchain'] == compiler]
        assert len(group) == 15 and len({r['binary_sha256'] for r in group}) == 1
for compiler in ['workspace', 'msrv']:
    log = gzip.decompress((root / (compiler + '.log.gz')).read_bytes()).decode()
    assert 'test result: ok. 28 passed' in log and 'test result: ok. 66 passed' in log
    assert 'FAILED' not in log and 'error:' not in log
print('Verified 60 traced executions, interruption/backpressure/deadline behavior and both full workspace logs.')
