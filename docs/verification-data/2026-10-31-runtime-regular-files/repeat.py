import os, re, subprocess, json, hashlib
from pathlib import Path
cases = ['worker_fifo','worker_fifo_open','worker_directory','worker_symlink','worker_procfd','manifest_fifo','manifest_fifo_open','lock_fifo']
env = dict(os.environ, TMPDIR=str(Path('target/qualification-temp').resolve()))
with open('target/runtime-regular-files-repeated.jsonl','w') as out:
 for toolchain, log in [('stable','workspace'),('msrv','msrv')]:
  contents = Path(f'target/runtime-regular-files-{log}.log').read_text()
  binary = re.search(r'Running tests/runtime.rs \(([^)]+)\)', contents).group(1)
  digest = hashlib.sha256(Path(binary).read_bytes()).hexdigest()
  for iteration in range(25):
   for case in cases:
    command = [binary,'--exact','filesystem_paths::filesystem_case_helper','--nocapture']
    run = subprocess.run(command,env=dict(env,PENELOPE_FILESYSTEM_CASE=case),capture_output=True,text=True,timeout=10)
    row = dict(toolchain=toolchain,binary_sha256=digest,iteration=iteration,case=case,command=command,returncode=run.returncode,stdout=run.stdout,stderr=run.stderr)
    out.write(json.dumps(row)+'\n'); out.flush()
    assert run.returncode == 0 and 'test result: ok. 1 passed; 0 failed;' in run.stdout, row
print('400 isolated filesystem cases passed across stable and MSRV.')
