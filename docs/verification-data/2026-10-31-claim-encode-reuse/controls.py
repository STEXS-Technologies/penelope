"""Compatibility and missing-clear controls, always restoring candidate sources."""
from pathlib import Path
import os,subprocess
out=Path(__file__).resolve().parent
paths=[Path('crates/penelope-sqlite/src')/name for name in ['process.rs','leases.rs']];original={p:p.read_bytes() for p in paths}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
args=['cargo','test','-p','penelope-sqlite','--test','process','variable_length_lease_bytes','--','--nocapture','--test-threads=1']
try:
 for p in paths:p.write_bytes(subprocess.check_output(['git','show','7608a47201e07b6509aca530731745bd919b5f2d:'+str(p)]))
 with (out/'baseline-compatibility.log').open('w') as log:subprocess.run(args,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 for p in paths:p.write_bytes(original[p])
 p=paths[0];s=p.read_text();assert s.count('    bytes.clear();')==1;p.write_text(s.replace('    bytes.clear();','    // control: deliberately keep bytes from the prior lease;'))
 with (out/'missing-clear-control.log').open('w') as log:r=subprocess.run(args,env=env,stdout=log,stderr=subprocess.STDOUT)
 text=(out/'missing-clear-control.log').read_text();assert r.returncode==101 and 'test result: FAILED. 0 passed; 1 failed' in text
 assert 'assertion `left == right` failed' in text and 'lease_encoding.rs' in text
finally:
 for p in paths:p.write_bytes(original[p])
for target,filters in [('--lib',['process::encode_tests::']),('--test',['process','variable_length_lease_bytes'])]:
 label='unit' if target=='--lib' else 'integration'
 with (out/f'restored-{label}.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite',target]+filters+['--','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('Baseline compatibility passes; missing-clear control fails; restored serialization and integration tests pass.')
