from pathlib import Path
import os, subprocess, json, hashlib, re
p=Path('crates/penelope-process-runtime/src/registry.rs')
candidate=p.read_bytes()
baseline=subprocess.check_output(['git','show','a0cd454:crates/penelope-process-runtime/src/registry.rs'])
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
rows=[]
try:
 for variant, source in [('baseline',baseline),('fixed',candidate)]:
  p.write_bytes(source)
  for toolchain,target in [('stable','target'),('1.85.0','target/worker-audit-msrv')]:
   command=['cargo','+'+toolchain,'test','-p','penelope-process-runtime','--test','worker_protocol_allocations','--no-run','--message-format=json']
   build=subprocess.run(command,env=dict(env,CARGO_TARGET_DIR=target),capture_output=True,text=True)
   Path(f'target/registry-buffers-build-{variant}-{toolchain}.log').write_text(build.stdout+build.stderr)
   build.check_returncode()
   artifacts=[json.loads(line) for line in build.stdout.splitlines()]
   binary=next(a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')=='worker_protocol_allocations' and a.get('executable'))
   digest=hashlib.sha256(Path(binary).read_bytes()).hexdigest()
   for iteration in range(10):
    run=subprocess.run([binary],env=env,capture_output=True,text=True,timeout=10)
    match=re.search(r'repeated public fences=(\[[^\]]+\])',run.stdout)
    counts=json.loads(match[1]) if match else None
    row=dict(variant=variant,toolchain=toolchain,iteration=iteration,command=[binary],binary_sha256=digest,source_sha256=hashlib.sha256(source).hexdigest(),returncode=run.returncode,counts=counts,stdout=run.stdout,stderr=run.stderr)
    rows.append(row)
    Path('target/registry-buffers-allocations-repeated.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
    assert run.returncode==0 and counts==([29]*3 if variant=='baseline' else [24]*3),row
finally:
 p.write_bytes(candidate)
print('40 isolated real-worker allocation probes passed; 120 fence counts: baseline 29, fixed 24 on both toolchains.')
