"""Check archived data completeness, source pins, allocation counts and regressions."""
from pathlib import Path
import gzip, hashlib, json, subprocess, tempfile
out = Path(__file__).resolve().parent
meta = json.loads((out / 'campaign.json').read_text())
for name, digest in meta['source_sha256'].items():
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == digest, name
for name, digest in json.loads((out / 'archive-hashes.json').read_text()).items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
runs = gzip.decompress((out / 'runs.jsonl.gz').read_bytes())
with tempfile.TemporaryDirectory() as directory:
    work = Path(directory)
    (work / 'runs.jsonl').write_bytes(runs)
    (work / 'analyze.py').write_bytes((out / 'analyze.py').read_bytes())
    subprocess.run(['python3', str(work / 'analyze.py')], check=True, capture_output=True)
    assert json.loads((work / 'summary.json').read_text()) == json.loads((out / 'summary.json').read_text())
repeat = gzip.decompress((out / 'repeat.log.gz').read_bytes()).decode()
assert repeat.count('test result: ok. 1 passed; 0 failed') == 20
for name in ['workspace.log.gz', 'msrv.log.gz', 'candidate-tests.log.gz', 'baseline-tests.log.gz']:
    text = gzip.decompress((out / name).read_bytes()).decode()
    assert 'test result: FAILED' not in text, name
    assert 'test result: ok.' in text, name
print('Verified source/archive hashes, all campaign statistics, allocation samples and 20 regression runs.')
