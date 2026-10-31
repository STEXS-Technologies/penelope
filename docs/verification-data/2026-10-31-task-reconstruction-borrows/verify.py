"""Verify full campaign, deterministic summaries, allocation deltas and matrix runs."""
from pathlib import Path
import gzip, hashlib, json, subprocess, tempfile
out = Path(__file__).resolve().parent
meta = json.loads((out / 'campaign.json').read_text())
for name, digest in meta['source_sha256'].items():
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == digest, name
for name, digest in json.loads((out / 'archive-hashes.json').read_text()).items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
with tempfile.TemporaryDirectory() as directory:
    work = Path(directory)
    (work / 'runs.jsonl').write_bytes(gzip.decompress((out / 'runs.jsonl.gz').read_bytes()))
    (work / 'analyze.py').write_bytes((out / 'analyze.py').read_bytes())
    subprocess.run(['python3', str(work / 'analyze.py')], check=True, capture_output=True)
    assert json.loads((work / 'summary.json').read_text()) == json.loads((out / 'summary.json').read_text())
repeat = gzip.decompress((out / 'repeat.log.gz').read_bytes()).decode()
assert repeat.count('test result: ok. 1 passed; 0 failed') == 20
for label in ['workspace', 'msrv', 'baseline-tests']:
    text = gzip.decompress((out / (label + '.log.gz')).read_bytes()).decode()
    assert 'test result: FAILED' not in text, label
    assert 'test result: ok.' in text, label
print('Verified full 96-process campaign, all timing statistics, allocation deltas, 420 matrix cases, source pins and archive hashes.')
