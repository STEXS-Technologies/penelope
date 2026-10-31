"""Check both measurement campaigns, regressions, source pins and statistics."""
import gzip
import hashlib
import itertools
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

here = pathlib.Path(__file__).resolve().parent
manifest = json.loads((here / 'manifest.json').read_text())
for name, expected in manifest['sha256'].items():
    assert hashlib.sha256((here / name).read_bytes()).hexdigest() == expected, name

def unpack(name):
    return gzip.decompress((here / (name + '.gz')).read_bytes()).decode()

def memo_rows(log):
    return [line for line in log.splitlines() if 'damage=' in line and 'four_rejections_unchanged=true repaired_reopened=true' in line]

for compiler in ['stable', '1.85.0']:
    for repeat in range(10):
        log = unpack(f'repeat-{compiler}-{repeat:02}-task_replay_memo.log')
        assert len(memo_rows(log)) == 9 and '1 passed; 0 failed' in log
        log = unpack(f'repeat-{compiler}-{repeat:02}-task_cache_revision.log')
        observations = [line for line in log.splitlines() if 'mode=' in line and ' accepted=' in line]
        assert len(observations) == 48 and '3 passed; 0 failed' in log
        assert all('accepted=false unchanged=true' in line for line in observations)
negative = unpack('negative-control.log')
assert 'append length-two' in negative and 'left: Ok(' in negative and 'test result: FAILED' in negative
baseline = unpack('baseline-control.log')
assert len(memo_rows(baseline)) == 9 and '1 passed; 0 failed' in baseline
for name in ['workspace', 'msrv', 'clippy', 'final-stable-regression', 'final-msrv-regression']:
    log = unpack(name + '.log')
    assert 'error:' not in log and 'test result: FAILED' not in log

hashes = json.loads((here / 'hashes.json').read_text())
primary_hashes = json.loads((here / 'primary-hashes.json').read_text())
for name, value in primary_hashes.items():
    assert hashes[name] == value
for variant in ['pre_guard', 'baseline', 'candidate']:
    assert hashlib.sha256(gzip.decompress((here / (variant + '-tasks.rs.gz')).read_bytes())).hexdigest() == hashes[variant + '-tasks.rs']
prior = json.loads((here / 'pre-guard-provenance-manifest.json').read_text())
provenance = json.loads((here / 'confirm-provenance.json').read_text())
assert hashes['pre_guard-task_blob_commits'] == prior['binary_and_source_hashes']['baseline'] == provenance['sha256']['pre_guard']
assert hashes['baseline-task_blob_commits'] == prior['binary_and_source_hashes']['cached'] == provenance['sha256']['guarded']
assert hashes['candidate-task_blob_commits'] == provenance['sha256']['optimized']
assert provenance['guarded_reused_hash_identical'] is True
assert provenance['benchmark_sha256'] == prior['source_hashes']['crates/penelope-sqlite/benches/task_blob_commits.rs']
assert json.loads((here / 'rebuild-verified.json').read_text()) == dict(primary_artifacts_identical=8, pre_guard_binary_identical=True, final_source_restored=True)
assert manifest['source_hashes']['crates/penelope-sqlite/src/tasks.rs'] == hashes['candidate-tasks.rs']

runs = [json.loads(line) for line in unpack('runs.jsonl').splitlines()]
assert len(runs) == 288
profiles = [('commits', False), ('commits', True), ('reconstruction', False), ('reconstruction', True), ('allocations', False), ('allocations', True)]
for pair in range(24):
    group = runs[pair * 12:pair * 12 + 12]
    order = ['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline']
    assert [r['pair'] for r in group] == [pair] * 12
    assert [r['variant'] for r in group] == [order[0]] * 6 + [order[1]] * 6
    assert [(r['kind'], r['changed']) for r in group] == profiles * 2
    for run in group:
        rows = run['rows']
        if run['kind'] == 'commits':
            assert [r['size'] for r in rows] == ([0, 32, 4096] if run['changed'] else [0, 32, 4096, 1048576])
            assert all(r['external_commit'] == run['changed'] and r['iterations'] == 128 and r['elapsed_ns'] > 0 for r in rows)
        elif run['kind'] == 'reconstruction':
            assert [r['history'] for r in rows] == [2, 16, 128]
            assert all(r['alternating'] == run['changed'] and r['iterations'] == 250 and r['elapsed_ns'] > 0 for r in rows)
        else:
            assert len(rows) == 48
            assert [(r['history'], r['sample']) for r in rows] == [(history, sample) for history in [2, 16, 128] for sample in range(16)]
            assert all(r['alternating'] == run['changed'] and (r['allocations'], r['reallocations'], r['requested_bytes']) == (1, 0, 96) for r in rows)
assert sum(len(r['rows']) for r in runs if r['kind'] != 'allocations') == 624
assert sum(len(r['rows']) for r in runs if r['kind'] == 'allocations') == 4608
assert sum(row['iterations'] for run in runs if run['kind'] == 'commits' for row in run['rows']) == 43008
assert sum(row['iterations'] for run in runs if run['kind'] == 'reconstruction' for row in run['rows']) == 72000

confirm = [json.loads(line) for line in unpack('confirm-runs.jsonl').splitlines()]
assert len(confirm) == 144
orders = list(itertools.permutations(['pre_guard', 'guarded', 'optimized']))
for block in range(24):
    group = confirm[block * 6:block * 6 + 6]
    profiles = [False, True] if block % 2 == 0 else [True, False]
    assert [r['block'] for r in group] == [block] * 6
    assert [r['variant'] for r in group] == list(orders[block % 6]) * 2
    assert [r['external_commit'] for r in group] == [profiles[0]] * 3 + [profiles[1]] * 3
    for run in group:
        assert [r['size'] for r in run['rows']] == ([0, 32, 4096] if run['external_commit'] else [0, 32, 4096, 1048576])
        assert all(r['external_commit'] == run['external_commit'] and r['iterations'] == 128 and r['elapsed_ns'] > 0 for r in run['rows'])
assert sum(len(r['rows']) for r in confirm) == 504

with tempfile.TemporaryDirectory(prefix='penelope-replay-verify-') as directory:
    temporary = pathlib.Path(directory)
    for name in ['runs.jsonl', 'confirm-runs.jsonl']:
        (temporary / name).write_text(unpack(name))
    for script, result in [('analyze.py', 'analysis.json'), ('analyze-confirm.py', 'confirm-analysis.json')]:
        shutil.copyfile(here / script, temporary / script)
        subprocess.run([sys.executable, str(temporary / script)], stdout=subprocess.DEVNULL, check=True)
        assert json.loads((temporary / result).read_text()) == json.loads((here / result).read_text()), result
assert len(json.loads((here / 'analysis.json').read_text())['latency']) == 13
assert len(json.loads((here / 'confirm-analysis.json').read_text())) == 21
print('Verified 432 processes, 34 latency statistics, 4,608 allocation samples, 180 memo-damage fixtures and 960 external-history cases.')
