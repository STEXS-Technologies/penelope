from pathlib import Path
import collections, json, shutil, subprocess, sys, tempfile, hashlib
root = Path(__file__).resolve().parent
sizes = {0, 32, 4096, 1048576}
cases = {'read', 'missing', 'corrupt_read', 'duplicate', 'corrupt_put'}
allocations = {}
for label in ['baseline', 'candidate']:
    groups = collections.defaultdict(set)
    for repeat in range(4):
        rows = [json.loads(line) for line in (root / f'{label}-alloc-{repeat}.jsonl').read_text().splitlines()]
        assert len(rows) == 320
        assert {(r['size'], r['case'], r['sample']) for r in rows} == {(s, c, n) for s in sizes for c in cases for n in range(16)}
        for r in rows:
            groups[(r['size'], r['case'])].add((r['allocations'], r['reallocations'], r['requested_bytes']))
    assert all(len(v) == 1 for v in groups.values())
    allocations[label] = {k: next(iter(v)) for k, v in groups.items()}
    assert '61 passed; 0 failed' in (root / f'{label}-tests.txt').read_text()
for key, before in allocations['baseline'].items():
    after = allocations['candidate'][key]
    expected = (1, 0, 80) if key[1] in {'read', 'missing', 'corrupt_read'} else (0, 0, 0)
    assert tuple(b - a for a, b in zip(before, after)) == expected
with tempfile.TemporaryDirectory() as directory:
    destination = Path(directory)
    for name in ['timing-0.jsonl', 'timing-1.jsonl', 'summarize.py']:
        shutil.copy2(root / name, destination / name)
    subprocess.run([sys.executable, str(destination / 'summarize.py')], check=True)
    assert (destination / 'summary.json').read_bytes() == (root / 'summary.json').read_bytes()
summary = json.loads((root / 'summary.json').read_text())
assert len(summary) == 40
assert all(r['bootstrap_95_percent'][1] < 0 for r in summary if r['case'] in {'read', 'missing', 'corrupt_read'})
total = reads = 0
for campaign in range(2):
    rows = [json.loads(line) for line in (root / f'timing-{campaign}.jsonl').read_text().splitlines()]
    assert len(rows) == 640
    for block in range(8):
        for position in range(4):
            process = [r for r in rows if r['block'] == block and r['position'] == position]
            assert len(process) == 20 and {(r['size'], r['case']) for r in process} == {(s, c) for s in sizes for c in cases}
    for r in rows:
        assert r['campaign'] == campaign
        assert r['iterations'] == (128 if r['size'] == 1048576 else 1000)
        assert r['warmup'] == (32 if r['size'] == 1048576 else 100)
        total += r['iterations']
        if r['case'] in {'read', 'missing', 'corrupt_read'}:
            reads += r['iterations']
assert total == 1000960 and reads == 600576
for name in ['workspace.txt', 'msrv.txt']:
    log = (root / name).read_text()
    for count in [35, 61, 45]:
        assert f'{count} passed; 0 failed' in log
    assert 'test result: FAILED' not in log
assert 'Finished' in (root / 'clippy.txt').read_text()
hashes = json.loads((root / 'source-hashes.json').read_text())
assert hashlib.sha256((root / 'candidate-payloads.rs').read_bytes()).hexdigest() == hashes['crates/penelope-sqlite/src/payloads.rs']
assert (root / 'baseline-payloads.rs').read_bytes() != (root / 'candidate-payloads.rs').read_bytes()
print('Verified 1,280 timing rows / 1,000,960 calls; 2,560 allocation samples; read latency and allocation tradeoff; qualification logs.')
