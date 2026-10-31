"""Validate fault-matrix outcomes, baseline reproduction, workspace logs and hashes."""
from pathlib import Path
import gzip
import hashlib
import json
import re

root = Path(__file__).resolve().parent
meta = json.loads((root / 'evidence.json').read_text())
for name, digest in meta['artifact_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
expected = {'10': 'Unavailable', '5': 'Unavailable', '6': 'Unavailable', '13': 'Exhausted', '18': 'Exhausted', '11': 'Corrupt', '26': 'Corrupt'}
pattern = r'(task_\w+) SQLite code (\d+): (\w+), expected (\w+); repair/reopen passed'
baseline = gzip.decompress((root / 'baseline.log.gz').read_bytes()).decode()
outcomes = re.findall(pattern, baseline)
assert len(outcomes) == 21 and sum(actual != expected[code] for table, code, actual, wanted in outcomes) == 15
assert 'test result: FAILED. 0 passed; 3 failed;' in baseline
rows = [json.loads(line) for line in gzip.decompress((root / 'repeated.jsonl.gz').read_bytes()).decode().splitlines()]
keys = {(r['toolchain'], r['iteration'], r['case']) for r in rows}
assert len(rows) == 60 and keys == {(t, i, c) for t in ['stable', '1.85.0'] for i in range(10) for c in ['head', 'record', 'blob']}
for row in rows:
    assert row['returncode'] == 0 and 'test result: ok. 1 passed; 0 failed;' in row['stdout']
    found = re.findall(pattern, row['stdout'])
    assert [list(values) for values in found] == row['outcomes']
    assert len(found) == 7 and {code for table, code, actual, wanted in found} == set(expected)
    assert all(actual == wanted == expected[code] for table, code, actual, wanted in found)
for compiler in ['workspace', 'msrv']:
    log = gzip.decompress((root / (compiler + '.log.gz')).read_bytes()).decode()
    assert 'test result: ok. 16 passed;' in log and 'FAILED' not in log and 'error:' not in log
print('Verified 15 baseline misclassifications, 60 successful matrices / 420 recovery cases, workspace logs and artifact hashes.')
