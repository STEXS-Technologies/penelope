"""Validate reproduced false acknowledgements, recovery coverage and artifacts."""
from pathlib import Path
import gzip
import hashlib
import json

root = Path(__file__).resolve().parent
meta = json.loads((root / 'evidence.json').read_text())
for name, digest in meta['artifact_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
baseline = gzip.decompress((root / 'baseline-final.log.gz').read_bytes()).decode()
assert 'test result: FAILED. 0 passed; 4 failed;' in baseline
assert baseline.count('left: Ok(TaskRecord') == 4
cases = {f'cache_usage::{source}_cache_rejects_{direction}_usage_until_repaired' for source in ['committed', 'rebuilt'] for direction in ['lower', 'higher']}
rows = [json.loads(line) for line in gzip.decompress((root / 'repeated.jsonl.gz').read_bytes()).decode().splitlines()]
assert len(rows) == 200
assert {(r['compiler'], r['iteration'], r['case']) for r in rows} == {(c, i, t) for c in ['stable', '1.85.0'] for i in range(25) for t in cases}
for row in rows:
    assert row['returncode'] == 0 and 'test result: ok. 1 passed; 0 failed;' in row['stdout']
for compiler in ['workspace', 'msrv']:
    log = gzip.decompress((root / (compiler + '.log.gz')).read_bytes()).decode()
    assert 'test result: ok. 26 passed;' in log and 'test result: ok. 18 passed;' in log
    assert 'FAILED' not in log and 'error:' not in log
print('Verified four baseline false acknowledgements, 200 rejection/repair/retry/reopen cases, workspace logs and artifact hashes.')
