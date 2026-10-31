from pathlib import Path
import subprocess,os,json,hashlib
rows=[]
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(Path('target/qualification-temp').resolve())}
for compiler,target in [('stable','target'),('1.85.0','target/worker-audit-msrv')]:
 p=subprocess.run(['cargo','+'+compiler,'build','-p','penelope-sqlite','--bench','claim_selection','--release','--message-format=json'],env={**env,'CARGO_TARGET_DIR':target},text=True,capture_output=True)
 Path('target/claim-selection-build-'+compiler+'.log').write_text(p.stdout+p.stderr);p.check_returncode()
 binary=next(item['executable'] for item in map(json.loads,p.stdout.splitlines()) if item.get('reason')=='compiler-artifact' and item.get('target',{}).get('name')=='claim_selection' and item.get('executable'))
 for repeat in range(10):
  p=subprocess.run([binary],env=env,text=True,capture_output=True,timeout=30,check=True)
  reports=[json.loads(line) for line in p.stdout.splitlines()]
  assert len(reports)==12
  rows.append(dict(compiler=compiler,repeat=repeat,binary_sha256=hashlib.sha256(Path(binary).read_bytes()).hexdigest(),stdout=p.stdout,stderr=p.stderr))
  Path('target/claim-selection-repeated.json').write_text(json.dumps(rows,indent=2)+'\n')
print('Passed 20 processes / 240 engine selection fixtures')
