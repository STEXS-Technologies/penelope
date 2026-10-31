"""Check repeated race logs, hashes, and unchanged production sources."""
from pathlib import Path
import collections
import gzip
import hashlib
import json
import re
import subprocess

out = Path(__file__).resolve().parent
root = out.parents[2]
manifest = json.loads((out / 'manifest.json').read_text())
for name, digest in manifest['files_sha256'].items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
for name, digest in manifest['sources_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
assert not subprocess.check_output([
    'git', 'diff', '--name-only', manifest['source_parent'], '--',
    ':(glob)crates/**/src/**', 'Cargo.toml', 'Cargo.lock',
], cwd=root).strip()
counts = collections.Counter()
for tool in ['stable', '1.85.0']:
    log = gzip.decompress((out / f'repeated-{tool}.txt.gz').read_bytes()).decode()
    assert log.count('test result: ok. 1 passed;') == 20
    assert 'test result: FAILED' not in log
    rows = re.findall(r'case=(competing-(?:empty|next)-frontier) admitted=2 accepted=1 serial_model=([01]) exact_state=true reopened_retry=true', log)
    assert len(rows) == 320
    for case, model in rows:
        counts[(tool, case, model)] += 1
    for case in ['competing-empty-frontier', 'competing-next-frontier']:
        assert sum(counts[(tool, case, model)] for model in ['0', '1']) == 160
    process = gzip.decompress((out / f'process-{tool}.txt.gz').read_bytes()).decode()
    assert 'test result: ok. 65 passed;' in process
    assert 'test result: FAILED' not in process
negative = gzip.decompress((out / 'negative-control.txt.gz').read_bytes()).decode()
assert 'case=competing-empty-frontier admitted=2 accepted=2' in negative
assert 'test result: FAILED' in negative
clippy = gzip.decompress((out / 'clippy.txt.gz').read_bytes()).decode()
assert 'Finished' in clippy and 'error:' not in clippy
checks = gzip.decompress((out / 'static-checks.txt.gz').read_bytes()).decode()
assert checks.count('exit status: 0') == 2
summary = [dict(tool=tool, case=case, serial_model=int(model), count=count)
           for (tool, case, model), count in sorted(counts.items())]
assert summary == json.loads((out / 'summary.json').read_text())
print('Verified 640 gated races, 1,280 competing requests, 1,280 serial reference histories, reopen retries, both 65-test suites and source hashes.')
