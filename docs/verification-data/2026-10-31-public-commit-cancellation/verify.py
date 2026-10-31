"""Read-only verification of source, qualification and all cancellation matrices."""
from pathlib import Path
import collections
import gzip
import hashlib
import itertools
import json
import re
import subprocess

archive = Path(__file__).resolve().parent
root = archive.parents[2]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def log(name):
    return gzip.decompress((archive / (name + '.gz')).read_bytes()).decode()


for name, expected in json.loads((archive / 'manifest.json').read_text()).items():
    assert sha((archive / name).read_bytes()) == expected, name
metadata = json.loads((archive / 'metadata.json').read_text())
for name, expected in metadata['source_sha256'].items():
    assert sha((root / name).read_bytes()) == expected, name
for name in metadata['unchanged_sources']:
    original = subprocess.check_output(['git', 'show', metadata['source_parent'] + ':' + name], cwd=root)
    assert sha(original) == metadata['source_sha256'][name], name
expected_profiles = set(itertools.product(['false', 'true'], repeat=4))
pattern = re.compile(r'lost_ack=LostAckCase \{ preaccepted: (false|true), after_completion: (false|true), seeded: (false|true), fail_sql: (false|true) \} admitted=2 result_unobserved=true exact_state=true reopened_retry=true conflict_unchanged=true')
summary = {'toolchains': {}, 'focused_cancellation_cases': 0, 'forced_sql_rollbacks': 0}
for label in ['stable', 'msrv']:
    stress = log(label + '-stress.txt')
    assert re.findall(r'iteration=(\d+) exit_status=(\d+)', stress) == [(str(i), '0') for i in range(20)]
    sections = re.split(r'iteration=\d+ exit_status=0\n', stress)
    assert len(sections) == 21 and sections[0] == ''
    for section in sections[1:]:
        profiles = pattern.findall(section)
        assert len(profiles) == 16 and set(profiles) == expected_profiles
        assert 'test result: ok. 1 passed; 0 failed;' in section
        assert 'test result: FAILED' not in section
    counts = collections.Counter(pattern.findall(stress))
    assert set(counts.values()) == {20} and len(counts) == 16
    qualified = log(label + '.txt')
    assert 'test result: FAILED' not in qualified
    for count in [36, 67, 45]:
        assert f'test result: ok. {count} passed; 0 failed;' in qualified
    assert 'canceled_public_commits_preserve_atomic_state_and_reopened_retry_semantics ... ok' in qualified
    summary['toolchains'][label] = {'repetitions': 20, 'cases': 320, 'forced_sql_rollbacks': 160}
    summary['focused_cancellation_cases'] += sum(counts.values())
    summary['forced_sql_rollbacks'] += sum(count for profile, count in counts.items() if profile[3] == 'true')
assert summary['focused_cancellation_cases'] == metadata['focused_cancellation_cases'] == 640
assert summary['forced_sql_rollbacks'] == 320
negative = log('negative-control.txt')
assert 'lost ACK retry must distinguish durable commit from rolled-back admission' in negative
assert 'left: false' in negative and 'right: true' in negative
assert 'test result: FAILED. 0 passed; 1 failed;' in negative
assert 'Finished' in log('clippy.txt')
assert log('static-checks.txt').count('exit_status=0') == 2
assert 'Code graph updated' in log('graphify.txt')
assert summary == json.loads((archive / 'summary.json').read_text())
print('Verified unchanged runtime and source/archive hashes, 40 complete matrices / 640 canceled commits / 320 SQL rollbacks, both-toolchain qualification and receipt negative control.')
