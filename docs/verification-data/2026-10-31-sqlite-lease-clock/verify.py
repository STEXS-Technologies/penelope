"""Validate lock-order reproduction, repeated outcomes, full qualification and hashes."""
from pathlib import Path
import gzip
import hashlib
import json

root = Path(__file__).resolve().parent
meta = json.loads((root / 'evidence.json').read_text())
for name, digest in meta['artifact_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
baseline = gzip.decompress((root / 'baseline-final.log.gz').read_bytes()).decode()
assert 'test result: FAILED. 1 passed; 2 failed;' in baseline
assert 'reclaim=true clock_samples_while_blocked=1 returned_leases=0 expiries=[]' in baseline
assert 'reclaim=false clock_samples_while_blocked=1 returned_leases=1 expiries=[LogicalTime(20)]' in baseline
names = ['new_outbox_claim_samples_clock_after_contended_writer_lock', 'expired_outbox_lease_is_reclaimed_using_time_after_writer_lock', 'clock_failure_and_expiry_overflow_release_the_writer_lock']
rows = [json.loads(line) for line in gzip.decompress((root / 'repeated.jsonl.gz').read_bytes()).decode().splitlines()]
assert len(rows) == 150
assert {(r['compiler'], r['iteration'], r['case']) for r in rows} == {(c, i, n) for c in ['stable', '1.85.0'] for i in range(25) for n in names}
for row in rows:
    assert row['returncode'] == 0 and 'test result: ok. 1 passed; 0 failed;' in row['stdout']
    if row['case'] != names[2]:
        assert 'clock_samples_while_blocked=0 returned_leases=1 expiries=[LogicalTime(110)]' in row['stdout']
for compiler in ['workspace', 'msrv']:
    log = gzip.decompress((root / (compiler + '.log.gz')).read_bytes()).decode()
    assert 'test result: ok. 21 passed;' in log and 'test result: ok. 26 passed;' in log
    assert 'public_type_inventory_accepts_only_static_thread_local_declarations ... ok' in log
    assert 'FAILED' not in log and 'error:' not in log
print('Verified both baseline failures, 150 repeated test processes / 200 scenarios, workspace logs and artifact hashes.')
