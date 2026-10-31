from pathlib import Path
import json, shutil, subprocess, sys, tempfile, hashlib
root = Path(__file__).resolve().parent
sizes = {0, 32, 4096, 1048576}
for label, expected in [('baseline', (4, 3, 2232)), ('candidate', (7, 3, 2552))]:
    for repeat in range(4):
        rows = [json.loads(line) for line in (root / f'{label}-alloc-{repeat}.jsonl').read_text().splitlines()]
        samples = [r for r in rows if 'sequence' in r]
        metadata = [r for r in rows if 'sequence' not in r]
        assert len(rows) == 516 and len(samples) == 512 and len(metadata) == 4
        assert {(r['size'], r['sequence']) for r in samples} == {(s, n) for s in sizes for n in range(2, 130)}
        assert all((r['allocations'], r['reallocations'], r['requested_bytes']) == expected and r['external_commit'] is False for r in samples)
        assert {r['size'] for r in metadata} == sizes
        assert all(r['iterations'] == 128 and r['external_commit'] is False for r in metadata)
    assert '44 passed; 0 failed' in (root / f'{label}-tests.txt').read_text()
with tempfile.TemporaryDirectory() as temporary:
    destination = Path(temporary)
    for name in ['timing-0.jsonl', 'timing-1.jsonl', 'summarize.py']:
        shutil.copy2(root / name, destination / name)
    subprocess.run([sys.executable, str(destination / 'summarize.py')], check=True)
    assert (destination / 'summary.json').read_bytes() == (root / 'summary.json').read_bytes()
summary = json.loads((root / 'summary.json').read_text())
assert len(summary) == 8 and all(r['bootstrap_95_percent'][0] < 0 < r['bootstrap_95_percent'][1] for r in summary)
for name in ['workspace.txt', 'msrv.txt']:
    log = (root / name).read_text()
    for count in [35, 61, 44]:
        assert f'{count} passed; 0 failed' in log
    assert 'test result: FAILED' not in log
assert 'Finished' in (root / 'clippy.txt').read_text()
assert '44 passed; 0 failed' in (root / 'restored-tests.txt').read_text()
hashes = json.loads((root / 'source-hashes.json').read_text())
assert hashlib.sha256((root / 'baseline-tasks.rs').read_bytes()).hexdigest() == hashes['crates/penelope-sqlite/src/tasks.rs']
assert (root / 'baseline-tasks.rs').read_bytes() != (root / 'candidate-tasks.rs').read_bytes()
print('Verified: 384 timing rows, 49,152 timed commits, 4,096 allocation samples; no repeatable latency benefit.')
