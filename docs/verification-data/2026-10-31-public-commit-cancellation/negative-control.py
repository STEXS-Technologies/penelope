from pathlib import Path
import os
import subprocess
import sys

root = Path.cwd()
out = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parent
out.mkdir(parents=True, exist_ok=True)
if (out / 'negative-control.txt').exists():
    raise FileExistsError('use a fresh output directory')
source = root / 'crates/penelope-sqlite/src/process.rs'
original = source.read_bytes()
needle = b'duplicate_input: commit.input.is_some(),'
assert original.count(needle) == 1
env = dict(os.environ, RUSTC_WRAPPER='', RUSTC_WORKSPACE_WRAPPER='', RUST_TEST_THREADS='1', TMPDIR=str(root/'target/qualification-temp'))
try:
    source.write_bytes(original.replace(needle, b'duplicate_input: false,', 1))
    with (out/'negative-control.txt').open('w') as log:
        result = subprocess.run(['cargo', 'test', '-p', 'penelope-sqlite', '--test', 'process', 'canceled_public_commits', '--all-features', '--', '--nocapture'], env=env, stdout=log, stderr=subprocess.STDOUT)
    assert result.returncode != 0
    assert 'lost ACK retry must distinguish durable commit from rolled-back admission' in (out/'negative-control.txt').read_text()
finally:
    source.write_bytes(original)
assert source.read_bytes() == original
print('Negative control rejected a false fresh receipt after lost ACK; production source restored.')
