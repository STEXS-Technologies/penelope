from pathlib import Path
import hashlib,json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
command=['cargo','build','--release','--locked','-p','penelope-executor','--bench','lease_batch_services','--bench','lease_batch_service_allocations','--message-format=json']
with (out/'benchmark-build.txt').open('w') as log:r=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
artifacts=[json.loads(s) for s in r.stdout.splitlines()];results={}
for name in ['lease_batch_services','lease_batch_service_allocations']:
 binary=next(a['executable'] for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==name)
 def run(args):
  return [json.loads(s) for s in subprocess.check_output([binary,*args],text=True).splitlines()]
 full=run([]);cargo=run(['--bench']);timer=run(['--bench','--timers-only']);outbox=run(['--outbox-only','--bench'])
 assert len(full)==len(cargo)==72 and len(timer)==len(outbox)==36
 assert all(r['timers'] for r in timer) and all(not r['timers'] for r in outbox)
 def stable(rows):
  return sorted([json.dumps({k:v for k,v in row.items() if k not in ['elapsed_ns','ns_per_call']},sort_keys=True) for row in rows])
 assert stable(full)==stable(cargo)==stable(timer+outbox)
 for args in [['--unknown'],['--timers-only','--outbox-only'],['--timers-only','--timers-only']]:
  r=subprocess.run([binary,*args],capture_output=True,text=True)
  assert r.returncode!=0 and not r.stdout and 'unknown or repeated service path' in r.stderr
 results[name]={'profiles':72,'timer_profiles':36,'outbox_profiles':36,'cargo_bench_argument':True,'same_checksums_and_allocations':True,'invalid_options_rejected':3,'binary_sha256':hashlib.sha256(Path(binary).read_bytes()).hexdigest()}
(out/'benchmark-smoke.json').write_text(json.dumps(results,indent=2)+'\n')
print('benchmark isolation smoke passed: both binaries, 72 default profiles, 36 per service, cargo bench argument, 6 invalid argument cases')
