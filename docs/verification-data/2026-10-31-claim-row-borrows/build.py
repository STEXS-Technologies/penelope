"""Build exact ordinary latency and separate instrumented allocation executables."""
from pathlib import Path
import os,subprocess,json,hashlib
root=Path.cwd();out=Path(__file__).resolve().parent
src=root/'crates/penelope-sqlite/src/leases.rs';benchsrc=root/'crates/penelope-sqlite/benches/lease_batch.rs'
original=src.read_bytes();fixture=benchsrc.read_bytes();names=[]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
allocator=(root/'crates/penelope-sqlite/benches/process_payload_allocations.rs').read_text();allocator=allocator[allocator.index('use std::{'):allocator.index('fn main()')]
probe='''
pub fn start(iteration:u32) {
 if [10,11].contains(&iteration) {
  ALLOCATIONS.store(0,Ordering::SeqCst);REALLOCATIONS.store(0,Ordering::SeqCst);REQUESTED_BYTES.store(0,Ordering::SeqCst);ENABLED.store(true,Ordering::SeqCst);
 }
}
pub fn finish(timers:bool,batch:u16,fixture:u32,iteration:u32) {
 if [10,11].contains(&iteration) {
  ENABLED.store(false,Ordering::SeqCst);
  let allocations=ALLOCATIONS.load(Ordering::SeqCst);let reallocations=REALLOCATIONS.load(Ordering::SeqCst);let requested_bytes=REQUESTED_BYTES.load(Ordering::SeqCst);
  println!("{}",serde_json::json!({"timers":timers,"batch":batch,"fixture":fixture,"iteration":iteration,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes}));
 }
}
'''
# Probe only the public call, including its worker operation and returned value.
instrumented=fixture.decode().replace('for _ in 0..8 {','for fixture in 0..8 {').replace('let started = Instant::now();','allocation_probe::start(iteration);\n                    let started = Instant::now();')
for call in ['store.claim_due(&timer)','store.claim(&outbox)']:
 target=f'let leases = block_on({call})??;';assert instrumented.count(target)==1
 instrumented=instrumented.replace(target,target+'\n                        allocation_probe::finish(timers,batch,fixture,iteration);')
instrumented+='\n#[allow(unsafe_code, clippy::arithmetic_side_effects)]\nmod allocation_probe {\n'+allocator+probe+'}\n'
(out/'lease_batch_allocations.rs').write_text(instrumented);names.append('lease_batch_allocations.rs')

def build(label, benches):
 with (out/(label+'-build.log')).open('w') as log:
  args=['cargo','bench','-p','penelope-sqlite']
  for name in benches:args+=['--bench',name]
  subprocess.run(args+['--no-run','--message-format=json'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 artifacts={}
 for line in (out/(label+'-build.log')).read_text().splitlines():
  try:r=json.loads(line)
  except json.JSONDecodeError:continue
  if r.get('reason')=='compiler-artifact' and r.get('executable') and r['target']['name'] in benches:artifacts[r['target']['name']]=r['executable']
 assert set(artifacts)==set(benches)
 for bench,path in artifacts.items():
  name=label+'-'+bench;target=out/name;target.write_bytes(Path(path).read_bytes());target.chmod(0o755);names.append(name)
try:
 for variant in ['baseline','candidate']:
  data=subprocess.check_output(['git','show','0a8bb67393becd1d22108db948ab4e83fd35f6a7:crates/penelope-sqlite/src/leases.rs']) if variant=='baseline' else original
  src.write_bytes(data);name=variant+'-leases.rs';(out/name).write_bytes(data);names.append(name)
  benchsrc.write_bytes(fixture);build(variant,['lease_claim','lease_batch'])
  benchsrc.write_text(instrumented);build(variant+'-allocations',['lease_batch'])
finally:src.write_bytes(original);benchsrc.write_bytes(fixture)
for name,path in [('lease_claim.rs','crates/penelope-sqlite/benches/lease_claim.rs'),('lease_batch.rs','crates/penelope-sqlite/benches/lease_batch.rs'),('lease_contracts.rs','crates/penelope-sqlite/tests/support/lease_contracts.rs'),('column-helper.rs','crates/penelope-sqlite/src/tasks.rs')]:
 (out/name).write_bytes(Path(path).read_bytes());names.append(name)
(out/'hashes.json').write_text(json.dumps({name:hashlib.sha256((out/name).read_bytes()).hexdigest() for name in names},indent=2)+'\n')
