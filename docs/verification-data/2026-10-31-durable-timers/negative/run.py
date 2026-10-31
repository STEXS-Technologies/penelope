from pathlib import Path
import hashlib,json,os,subprocess
root=Path.cwd();out=root/'docs/verification-data/2026-10-31-durable-timers/negative';out.mkdir(exist_ok=True)
workflow=root/'crates/penelope-executor/benches/support/timer_workload/workflow.rs';clock=root/'crates/penelope-executor/benches/support/timer_workload/ports.rs'
originals={p:p.read_text() for p in [workflow,clock]}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
mutants=[('result',workflow,'let result = value.to_le_bytes();','let result = value.wrapping_add(1).to_le_bytes();','durable business result differs'),('truncated',workflow,'if discovered.is_empty() {','if discovered.is_empty() || commits.len() >= 64 {','business computation differs'),('identity',workflow,'let direct_first = round & 1 == 0;','if let Some((input, _)) = expected.first_mut() { input.input_id = "inp_substituted".parse()?; }\n    let direct_first = round & 1 == 0;','timer identity differs from legacy'),('not-due',clock,'Ok(LogicalTime(100))','Ok(LogicalTime(99))','incomplete timer delivery')]
results=[]
def build(name):
 with (out/(name+'-build.txt')).open('w') as log:
  r=subprocess.run(['cargo','build','--release','--locked','-p','penelope-executor','--bench','durable_timer_workload','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
 return next(a['executable'] for line in r.stdout.splitlines() if (a:=json.loads(line)).get('executable') and a.get('target',{}).get('name')=='durable_timer_workload')
try:
 for name,path,before,after,marker in mutants:
  for p,text in originals.items():p.write_text(text)
  assert originals[path].count(before)==1,name
  path.write_text(originals[path].replace(before,after))
  binary=build(name)
  r=subprocess.run([binary,'--batch','128','--width','128','--work','64','--reopen','true'],env=env,capture_output=True,text=True)
  (out/(name+'.txt')).write_text(r.stdout+r.stderr)
  assert r.returncode!=0 and marker in r.stderr and not r.stdout,(name,r.returncode,r.stderr)
  results.append(dict(name=name,rejected=True,marker=marker))
finally:
 for p,text in originals.items():p.write_text(text)
assert all(p.read_text()==text for p,text in originals.items())
binary=build('restored')
r=subprocess.run([binary,'--batch','128','--width','128','--work','64','--reopen','true'],env=env,capture_output=True,text=True,check=True)
(out/'restored.txt').write_text(r.stdout+r.stderr)
assert json.loads(r.stdout)['consumed']==128
(out/'results.json').write_text(json.dumps(dict(results=results,restored_sha256={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in originals}),indent=2)+'\n')
