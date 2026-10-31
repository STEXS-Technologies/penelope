from pathlib import Path
import os, subprocess, json, hashlib, re
root=Path('target/runtime-write-interrupt-traces');root.mkdir(exist_ok=False)
p=Path('crates/penelope-process-runtime/src/runtime.rs');candidate=p.read_bytes()
baseline=subprocess.check_output(['git','show','fecc2fa:crates/penelope-process-runtime/src/runtime.rs'])
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
cases={'interrupted':'interrupted_bursts_preserve_bytes_across_short_writes_and_parts','wouldblock':'would_block_bursts_preserve_bytes_across_short_writes_and_parts','deadline':'continuous_interruptions_do_not_reset_the_write_deadline'}
rows=[]
try:
 for variant,source in [('baseline',baseline),('fixed',candidate)]:
  p.write_bytes(source)
  for toolchain,target in [('stable','target'),('1.85.0','target/worker-audit-msrv')]:
   command=['cargo','+'+toolchain,'test','-p','penelope-process-runtime','--lib','--no-run','--message-format=json']
   build=subprocess.run(command,env=dict(env,CARGO_TARGET_DIR=target),capture_output=True,text=True)
   (root/f'build-{variant}-{toolchain}.log').write_text(build.stdout+build.stderr);build.check_returncode()
   artifacts=[json.loads(line) for line in build.stdout.splitlines()]
   binary=next(a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')=='penelope_process_runtime' and a.get('executable'))
   binary_hash=hashlib.sha256(Path(binary).read_bytes()).hexdigest()
   for iteration in range(5):
    for case,test in cases.items():
     trace=root/f'{variant}-{toolchain}-{case}-{iteration}.trace'
     command=['strace','-f','-qq','-e','trace=clock_nanosleep,nanosleep','-o',str(trace),binary,'--exact','runtime::io_tests::'+test,'--nocapture','--test-threads=1']
     run=subprocess.run(command,env=env,capture_output=True,text=True,timeout=10)
     calls=len(re.findall(r'\b(?:clock_nanosleep|nanosleep)\(',trace.read_text()))
     row=dict(variant=variant,toolchain=toolchain,iteration=iteration,case=case,command=command,returncode=run.returncode,stdout=run.stdout,stderr=run.stderr,sleep_calls=calls,trace=trace.name,binary_sha256=binary_hash,source_sha256=hashlib.sha256(source).hexdigest())
     rows.append(row);(root/'processes.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
     assert run.returncode==0 and 'test result: ok. 1 passed; 0 failed;' in run.stdout,row
     if case=='wouldblock':assert calls==8,row
     elif variant=='fixed':assert calls==0,row
     elif case=='interrupted':assert calls==16,row
     else:assert calls>0,row
     if case=='deadline':assert int(re.search(r'interrupted attempts before deadline: (\d+)',run.stdout)[1])>0,row
finally:
 p.write_bytes(candidate)
print('60 traced tests passed on both compilers: interruption sleeps 16 to 0; WouldBlock remains 8; continuous interruption expires.')
