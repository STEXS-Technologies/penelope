"""Verify the large-limit admission reproduction and complete qualification."""
from pathlib import Path
import gzip, hashlib, json, re
root = Path(__file__).resolve().parent
def load(name):
    data = (root/name).read_bytes()
    return gzip.decompress(data) if name.endswith('.gz') else data
def parsed(name): return json.loads(load(name))
manifest = parsed('sha256.json')
assert set(manifest) == {p.name for p in root.iterdir() if p.is_file() and p.name != 'sha256.json'}
for name,digest in manifest.items(): assert hashlib.sha256((root/name).read_bytes()).hexdigest() == digest, name
metadata = parsed('metadata.json')
limits = [4096,16384,16647160,16647161,4294967295]
assert metadata['limits'] == limits and metadata['baseline_cargo_returncode'] == 101
assert metadata['repeated_processes'] == 50 and metadata['configurations'] == 250
assert limits[2] == (2**32-1)//258 and limits[3] == limits[2]+1
pattern = r'record_limit=(\d+) first_commit=(Ok\(AtomicProcessCommitReceipt \{ committed_through_sequence: 0, duplicate_input: false \}\)|Err\(QuotaExceeded\))'
def outcomes(log): return [(int(limit),result.startswith('Ok')) for limit,result in re.findall(pattern,log)]
baseline = load('baseline.log.gz').decode()
assert outcomes(baseline) == list(zip(limits,[True,True,True,False,False]))
assert 'test result: FAILED. 0 passed; 1 failed;' in baseline
fixed = load('fixed.log.gz').decode()
assert outcomes(fixed) == [(limit,True) for limit in limits]
assert 'test result: ok. 1 passed; 0 failed;' in fixed
source = load('candidate-process.rs.gz')
assert b'.saturating_mul(' in source and b'.checked_mul(' in load('baseline-process.rs.gz')
rows = parsed('repeated.json.gz')
assert len(rows) == 50
assert {(r['compiler'],r['repeat']) for r in rows} == {(c,i) for c in ['stable','1.85.0'] for i in range(25)}
for row in rows:
    assert row['returncode'] == 0 and not row['stderr']
    assert outcomes(row['stdout']) == [(limit,True) for limit in limits]
    assert 'test result: ok. 1 passed; 0 failed;' in row['stdout']
for compiler in ['stable','1.85.0']:
    assert len({r['binary_sha256'] for r in rows if r['compiler'] == compiler}) == 1
for filename in ['workspace.log.gz','msrv.log.gz']:
    log = load(filename).decode()
    for marker,count in [(r'Running unittests src/lib.rs \([^\n]*penelope_sqlite-',23),(r'Running tests/process.rs \(',40),(r'Running tests/tasks.rs \(',26)]:
        section = re.split(marker,log,maxsplit=1)[1].split('     Running ',1)[0]
        assert f'test result: ok. {count} passed;' in section
    assert 'test result: FAILED' not in log and 'error: test failed' not in log
assert 'Finished' in load('clippy.log.gz').decode()
assert not load('fmt.log.gz')
assert 'Code graph updated.' in load('graph.log.gz').decode()
print('Verified baseline false rejections, 50 isolated processes / 250 limit configurations, and both compiler suites')
