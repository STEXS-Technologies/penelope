"""Verify source/qualification and recompute caller-only allocation comparisons."""
from pathlib import Path
import gzip
import hashlib
import json
import subprocess
import tempfile

archive = Path(__file__).resolve().parent
root = archive.parents[2]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(name):
    return gzip.decompress((archive / (name + '.gz')).read_bytes()).decode()


for name, expected in json.loads((archive / 'manifest.json').read_text()).items():
    assert sha((archive / name).read_bytes()) == expected, name
hashes = json.loads((archive / 'source-hashes.json').read_text())
for name, expected in hashes.items():
    assert sha((root / name).read_bytes()) == expected, name
metadata = json.loads((archive / 'metadata.json').read_text())
for name, expected in metadata['harness_sha256'].items():
    assert hashes[name] == expected
name = 'crates/penelope-sqlite/src/process.rs'
baseline = subprocess.check_output(['git', 'show', metadata['source_parent'] + ':' + name], cwd=root)
assert sha(baseline) == metadata['sources']['baseline'][name]
candidate = (root / name).read_bytes()
assert sha(candidate) == metadata['sources']['candidate'][name]


def without_append(data):
    start = data.index(b'    async fn append_outcomes(')
    end = data.index(b'    async fn read_outcomes(', start)
    return data[:start] + data[end:]


assert without_append(baseline) == without_append(candidate)
with tempfile.TemporaryDirectory(prefix='penelope-append-verify-') as directory:
    temporary = Path(directory)
    destination = temporary / name
    destination.parent.mkdir(parents=True)
    destination.write_bytes(baseline)
    patch = temporary / 'runtime.patch'
    patch.write_bytes(gzip.decompress((archive / 'runtime.patch.gz').read_bytes()))
    subprocess.run(['git', 'apply', str(patch)], cwd=temporary, check=True)
    assert destination.read_bytes() == candidate
for name in ['crates/penelope-sqlite/src/worker.rs', 'crates/penelope-sqlite/src/schema.rs', 'crates/penelope-domain/src/implementation.rs', 'crates/penelope-ports/src/implementation.rs', 'Cargo.lock']:
    assert sha(subprocess.check_output(['git', 'show', metadata['source_parent'] + ':' + name], cwd=root)) == hashes[name]
rows = [json.loads(line) for line in read('allocations.jsonl').splitlines()]
assert len(rows) == 256
keys = set()
checksums = {}
for row in rows:
    assert row['scope_len'] in [9, 128] and row['id_len'] in [8, 128] and row['count'] in [1, 128]
    assert row['sample'] in range(8) and row['repeat'] in range(2)
    assert row['variant'] in ['baseline', 'candidate'] and row['verified'] is True
    key = tuple(row[k] for k in ['scope_len', 'id_len', 'count', 'variant', 'repeat', 'sample'])
    assert key not in keys
    keys.add(key)
    fixture = key[:3]
    assert checksums.setdefault(fixture, row['checksum']) == row['checksum']
    assert all(type(row[k]) is int and row[k] >= 0 for k in ['allocations', 'reallocations', 'requested_bytes'])
summary = []
for scope_len in [9, 128]:
    for id_len in [8, 128]:
        for count in [1, 128]:
            result = dict(scope_len=scope_len, id_len=id_len, count=count)
            for label in ['baseline', 'candidate']:
                selected = [r for r in rows if (r['scope_len'], r['id_len'], r['count'], r['variant']) == (scope_len, id_len, count, label)]
                assert len(selected) == 16
                values = {tuple(r[k] for k in ['allocations', 'reallocations', 'requested_bytes']) for r in selected}
                assert len(values) == 1
                result[label] = dict(zip(['allocations', 'reallocations', 'requested_bytes'], values.pop()))
                assert result[label]['reallocations'] == 0
            result['saved_allocations'] = result['baseline']['allocations'] - result['candidate']['allocations']
            result['saved_requested_bytes'] = result['baseline']['requested_bytes'] - result['candidate']['requested_bytes']
            assert result['saved_allocations'] == 2 + (8 * count + 4 if scope_len == 128 else 0) + (2 * count if id_len == 128 else 0)
            assert result['saved_requested_bytes'] > 0
            summary.append(result)
assert summary == json.loads((archive / 'summary.json').read_text())
for label in ['baseline', 'candidate']:
    focused = read(label + '-tests.txt')
    assert 'test result: ok. 2 passed; 0 failed;' in focused
    assert 'append_checks_every_scope_field_before_copying_or_writing_any_rows ... ok' in focused
    assert 'append_matches_commit_rollback_retry_and_bounds_at_empty_and_seeded_frontiers ... ok' in focused
negative = read('negative-control.txt')
assert 'scope rejection cloned strings' in negative and 'rejection_allocations=512' in negative
negative_copy = read('copy-negative-control.txt')
assert 'append admission copied more than one batch' in negative_copy and 'allocations=1549' in negative_copy
for label in ['stable', 'msrv']:
    qualified = read(label + '.txt')
    assert 'test result: FAILED' not in qualified
    for count in [36, 70, 45]:
        assert f'test result: ok. {count} passed; 0 failed;' in qualified
    for test in ['admitted_append_survives_caller_cancellation_and_reopened_exact_retry', 'append_checks_every_scope_field_before_copying_or_writing_any_rows', 'append_matches_commit_rollback_retry_and_bounds_at_empty_and_seeded_frontiers']:
        assert test + ' ... ok' in qualified
    assert 'scope_rejection_allocations=0 repaired=true admission_allocations=775 single_batch=true' in qualified
assert 'Finished' in read('msrv-check.txt') and 'Finished' in read('clippy.txt')
assert read('static-checks.txt').count('exit_status=0') == 2
assert 'Code graph updated' in read('graphify.txt')
print('Verified source hashes, append-only runtime patch, 256 observations/eight exact allocation comparisons, both negative controls and stable/MSRV qualification.')
