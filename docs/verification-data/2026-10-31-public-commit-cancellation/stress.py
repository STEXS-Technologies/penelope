from pathlib import Path
import os
import subprocess
import sys

label = sys.argv[1]
assert label in ['stable', 'msrv']
out = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path(__file__).resolve().parent
out.mkdir(parents=True, exist_ok=True)
log = out / (label + '-stress.txt')
if log.exists():
    raise FileExistsError('use a fresh output directory')
root = Path.cwd()
env = dict(os.environ, RUSTC_WRAPPER='', RUSTC_WORKSPACE_WRAPPER='', RUST_TEST_THREADS='1', TMPDIR=str(root/'target/qualification-temp'))
command = ['cargo']
if label == 'msrv':
    command += ['+1.85.0']
    env['CARGO_TARGET_DIR'] = str(root/'target/worker-audit-msrv')
command += ['test', '-p', 'penelope-sqlite', '--test', 'process', 'canceled_public_commits', '--all-features', '--', '--nocapture']
with log.open('w') as output:
    for iteration in range(20):
        result = subprocess.run(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        output.write(f'iteration={iteration} exit_status={result.returncode}\n' + result.stdout)
        output.flush()
        result.check_returncode()
        assert result.stdout.count('result_unobserved=true exact_state=true reopened_retry=true conflict_unchanged=true') == 16
print(f'{label}: 20 complete matrices, 320 canceled commits and queued replay barriers passed')
