from pathlib import Path
import hashlib,json,os,re,subprocess,sys
root=Path.cwd();out=Path(__file__).resolve().parent;toolchain=sys.argv[1];assert toolchain in ['stable','msrv']
files=['crates/penelope-executor/tests/timer_batch_recovery.rs','crates/penelope-executor/tests/support/timer_batch_faults.rs','crates/penelope-executor/tests/support/timer_input_legacy.rs','crates/penelope-executor/src/services.rs','crates/penelope-executor/src/timer_input.rs','crates/penelope-executor/src/lease_batch_index.rs','crates/penelope-sqlite/src/leases.rs','crates/penelope-sqlite/src/process.rs','crates/penelope-sqlite/src/worker.rs','Cargo.lock']
sha=lambda b:hashlib.sha256(b).hexdigest();sources={p:sha((root/p).read_bytes()) for p in files}
faults=['PayloadBefore','PayloadAfter','AcceptBefore','AcceptAfter','AckBefore','AckAfter','ClockAfterAccept','ReceiptMismatch'];expected={f'{n}-{w}-{i}-{fault}' for w,n in [(9,17),(128,128)] for i in [0,n//2,n-1] for fault in faults}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');env.pop('PENELOPE_TIMER_BATCH_CASE',None)
if toolchain=='msrv':env['CARGO_TARGET_DIR']=str(root/'target/worker-audit-msrv')
command=['cargo']+(['+1.85.0'] if toolchain=='msrv' else [])+['test','-p','penelope-executor','--test','timer_batch_recovery','--','--nocapture'];results=[]
for repeat in range(2):
 name=f'{toolchain}-{repeat}.txt'
 with (out/name).open('w') as log:subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 text=(out/name).read_text();cases=re.findall(r'timer_batch_case=(\S+) verified=true',text);assert len(cases)==48 and set(cases)==expected and '1 passed; 0 failed' in text
 results.append({'repeat':repeat,'log':name,'cases':48,'verified':True})
assert sources=={p:sha((root/p).read_bytes()) for p in files}
rustc=['rustc']+(['+1.85.0'] if toolchain=='msrv' else [])+['-Vv']
(out/(toolchain+'-results.json')).write_text(json.dumps({'toolchain':toolchain,'rustc':subprocess.check_output(rustc,text=True),'source_sha256':sources,'runs':results},indent=2)+'\n')
