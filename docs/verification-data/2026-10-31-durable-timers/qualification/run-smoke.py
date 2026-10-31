from pathlib import Path
import hashlib,json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
metadata=json.loads((out.parent/'metadata.json').read_text())
stable=next(path for path in (root/'target/release/deps').glob('durable_timer_workload-*') if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest()==metadata['binary_sha256'])
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),CARGO_TARGET_DIR='target/worker-audit-msrv')
invalid=[['--batch','0'],['--batch','129'],['--width','8'],['--width','129'],['--work','16777217'],['--rounds','0'],['--rounds','33'],['--unknown','1'],['--batch'],['--reopen','maybe']]
for args in invalid:
 r=subprocess.run([str(stable),*args],capture_output=True,text=True)
 assert r.returncode and not r.stdout,(args,r.stdout)
with (out/'msrv-bench-build.txt').open('w') as log:
 r=subprocess.run(['cargo','+1.85.0','build','--locked','-p','penelope-executor','--bench','durable_timer_workload','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
binary=next(a['executable'] for line in r.stdout.splitlines() if (a:=json.loads(line)).get('executable') and a.get('target',{}).get('name')=='durable_timer_workload')
r=subprocess.run([binary,'--bench','--batch','128','--width','128','--work','64','--rounds','2','--reopen','true'],env=env,capture_output=True,text=True,check=True)
(out/'msrv-smoke.jsonl').write_text(r.stdout)
rows=[json.loads(line) for line in r.stdout.splitlines()];assert len(rows)==2 and all(row['consumed']==128 and row['verified'] for row in rows)
(out/'smoke.json').write_text(json.dumps(dict(invalid_cases_rejected=len(invalid),msrv_rows=len(rows),msrv_timers=sum(row['consumed'] for row in rows),binary_sha256=hashlib.sha256(Path(binary).read_bytes()).hexdigest()),indent=2)+'\n')
print('smoke passed: 10 invalid configurations rejected; 256 timers consumed on Rust 1.85.0')
