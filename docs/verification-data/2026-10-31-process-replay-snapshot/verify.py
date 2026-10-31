"""Verify snapshot interleavings, qualification logs and unchanged read code."""
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
source = 'crates/penelope-sqlite/src/process.rs'
baseline = subprocess.check_output(['git', 'show', f"{manifest['source_parent']}:{source}"], cwd=root)
assert (root / source).read_bytes() == baseline + b'\n#[cfg(test)]\n#[path = "process_snapshot_tests.rs"]\nmod snapshot_tests;\n'
changed = subprocess.check_output([
    'git', 'diff', '--name-only', manifest['source_parent'], '--',
    ':(glob)crates/**/src/**', 'src', 'Cargo.toml', 'Cargo.lock',
], cwd=root, text=True).splitlines()
assert set(changed) <= {source, 'crates/penelope-sqlite/src/process_snapshot_tests.rs'}
profiles = ['empty', 'whole', 'page', 'end']
counts = collections.Counter()
for tool in ['stable', '1.85.0']:
    log = gzip.decompress((out / f'repeated-{tool}.txt.gz').read_bytes()).decode()
    runs = re.split(r'Run [0-9]+/20\n', log)
    assert len(runs) == 21 and not runs[0]
    for run in runs[1:]:
        found = re.findall(r'profile=(empty|whole|page|end) append_committed_during_read=true pinned_old_page=true later_new_page=true reopened=true', run)
        assert found == profiles
        assert 'test result: ok. 1 passed;' in run and 'test result: FAILED' not in run
        counts.update((tool, profile) for profile in found)
    qualification = gzip.decompress((out / f'sqlite-{tool}.txt.gz').read_bytes()).decode()
    for count in [36, 66, 45]:
        assert f'test result: ok. {count} passed;' in qualification
    assert 'test result: FAILED' not in qualification
negative = gzip.decompress((out / 'negative-control.txt.gz').read_bytes()).decode()
assert 'test result: FAILED' in negative and 'Err` value: Invariant' in negative
clippy = gzip.decompress((out / 'clippy.txt.gz').read_bytes()).decode()
assert 'Finished' in clippy and 'error:' not in clippy
checks = gzip.decompress((out / 'static-checks.txt.gz').read_bytes()).decode()
assert checks.count('exit status: 0') == 2
summary = [dict(tool=tool, profile=profile, interleavings=count)
           for (tool, profile), count in sorted(counts.items())]
assert summary == json.loads((out / 'summary.json').read_text())
print('Verified 160 forced append-during-replay interleavings, all four profiles, both SQLite suites (36 unit/66 process/45 task), negative control and source hashes.')
