from pathlib import Path
import subprocess,os,json,hashlib,re
source=Path('crates/penelope-process-runtime/src/runtime.rs');candidate=source.read_bytes()
baseline=subprocess.check_output(['git','show','HEAD:'+str(source)])
rows=[]
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(Path('target/qualification-temp').resolve())}
try:
 for variant,content in [('baseline',baseline),('candidate',candidate)]:
  source.write_bytes(content)
  for compiler,target in [('stable','target'),('1.85.0','target/worker-audit-msrv')]:
   run=subprocess.run(['cargo','+'+compiler,'test','-p','penelope-process-runtime','--test','worker_protocol_allocations','--no-run','--message-format=json'],env={**env,'CARGO_TARGET_DIR':target},text=True,capture_output=True)
   Path(f'target/start-header-build-{variant}-{compiler}.log').write_text(run.stdout+run.stderr)
   run.check_returncode()
   binary=next(item['executable'] for item in map(json.loads,run.stdout.splitlines()) if item.get('reason')=='compiler-artifact' and item.get('target',{}).get('name')=='worker_protocol_allocations' and item.get('executable'))
   for repeat in range(10):
    p=subprocess.run([binary],env=env,text=True,capture_output=True,timeout=30,check=True)
    match=re.search(r'supervisor dispatch=(\d+)',p.stdout);assert match,p.stdout
    rows.append(dict(variant=variant,compiler=compiler,repeat=repeat,dispatch_allocations=int(match[1]),stdout=p.stdout,stderr=p.stderr,binary_sha256=hashlib.sha256(Path(binary).read_bytes()).hexdigest(),source_sha256=hashlib.sha256(content).hexdigest()))
    Path('target/start-header-allocations.json').write_text(json.dumps(rows,indent=2)+'\n')
finally:
 source.write_bytes(candidate)
for compiler in ['stable','1.85.0']:
 counts={v:{r['dispatch_allocations'] for r in rows if r['compiler']==compiler and r['variant']==v} for v in ['baseline','candidate']}
 assert all(len(v)==1 for v in counts.values()),counts
 assert next(iter(counts['baseline']))-next(iter(counts['candidate']))==1,counts
 print(compiler,counts,flush=True)
print('40 real-worker allocation probes passed')
