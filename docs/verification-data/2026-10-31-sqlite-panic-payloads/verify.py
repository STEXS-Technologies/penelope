"""Check complete panic-payload regression evidence and source hashes."""
from pathlib import Path
import gzip, hashlib, json, re
root = Path(__file__).resolve().parent
def load(name):
    data = (root/name).read_bytes()
    return gzip.decompress(data) if name.endswith('.gz') else data
def parsed(name): return json.loads(load(name))
manifest = parsed('sha256.json')
assert set(manifest) == {p.name for p in root.iterdir() if p.is_file() and p.name != 'sha256.json'}
for name, digest in manifest.items():
    assert hashlib.sha256((root/name).read_bytes()).hexdigest() == digest, name
metadata = parsed('metadata.json')
assert metadata['baseline_cargo_exit_codes'] == [101,101]
assert metadata['repeated_processes'] == 100 and metadata['scenarios'] == 400
names = ['worker::panic_tests::operation_panic_payload_destructor_cannot_strand_accepted_work', 'worker::panic_tests::notification_panic_payload_destructor_cannot_strand_accepted_work']
for name, filename in zip(reversed(names), ['baseline.log.gz','operation-baseline.log.gz']):
    log = load(filename).decode()
    assert name in log
    assert 'fatal runtime error: thread result panicked on drop, aborting' in log
    assert 'signal: 6, SIGABRT' in log
baseline = load('baseline-worker.rs.gz')
parent = load('parent-worker.rs.gz')
assert baseline == parent + b'\n#[cfg(test)]\n#[path = "worker_panic_tests.rs"]\nmod panic_tests;\n'
candidate = load('candidate-worker.rs.gz')
assert b'fn contain_panic<T>' in candidate and b'std::mem::forget(secondary)' in candidate
assert b'contain_panic(|| operation(connection))' in candidate
assert b'contain_panic(|| waker.wake())' in candidate
assert '2 passed; 0 failed;' in load('fixed.log.gz').decode()
rows = parsed('repeated.json.gz')
assert len(rows) == 100
assert {(r['compiler'],r['repeat'],r['case']) for r in rows} == {(c,i,n) for c in ['stable','1.85.0'] for i in range(25) for n in names}
for row in rows:
    assert row['returncode'] == 0 and not row['stderr']
    assert 'test result: ok. 1 passed; 0 failed;' in row['stdout']
    assert row['case'] in row['stdout']
for compiler in ['stable','1.85.0']:
    assert len({r['binary_sha256'] for r in rows if r['compiler'] == compiler}) == 1
for filename in ['workspace.log.gz','msrv.log.gz']:
    log = load(filename).decode()
    suites = [(r'Running unittests src/lib.rs \([^\n]*penelope_sqlite-',23), (r'Running tests/process.rs \(',39), (r'Running tests/tasks.rs \(',26)]
    for marker,count in suites:
        section = re.split(marker,log,maxsplit=1)[1].split('     Running ',1)[0]
        assert f'test result: ok. {count} passed;' in section
    assert 'test result: FAILED' not in log and 'error: test failed' not in log
    assert 'pending/checkpoint poll=0, committed checkpoint ACK=0' in log
assert 'Finished' in load('clippy.log.gz').decode()
assert not load('fmt.log.gz')
assert 'Code graph updated.' in load('graph.log.gz').decode()
print('Verified two baseline SIGABRT reproductions, 100 isolated processes / 400 scenarios, and both compiler suites')
