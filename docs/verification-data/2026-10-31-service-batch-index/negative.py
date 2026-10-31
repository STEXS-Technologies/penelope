from pathlib import Path
import json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
helper='crates/penelope-executor/src/lease_batch_index.rs';services='crates/penelope-executor/src/services.rs'
original={p:(root/p).read_bytes() for p in [helper,services]};h=original[helper];s=original[services]
precedence=b'''        for (index, lease) in leases.iter().enumerate() {
            validate(lease)?;
            if duplicate == Some(index) {
                return Err(penelope_ports::PortError::Invariant);
            }
        }'''
reversed_precedence=b'''        for (index, lease) in leases.iter().enumerate() {
            if duplicate == Some(index) {
                return Err(penelope_ports::PortError::Invariant);
            }
            validate(lease)?;
        }'''
mutants=[
 ('action',helper,h.replace(b'action_duplicate.into_iter().chain(token_duplicate).min()',b'token_duplicate'),'lease_batch_identity','earliest_conflict'),
 ('token',helper,h.replace(b'action_duplicate.into_iter().chain(token_duplicate).min()',b'action_duplicate'),'lease_batch_identity','earliest_conflict'),
 ('precedence',helper,h.replace(precedence,reversed_precedence),'lease_batch_identity','public_batches'),
 ('short-validation',helper,h.replace(precedence,precedence.replace(b'leases.iter().enumerate()',b'leases.iter().enumerate().take(16)')),'lease_batch_identity','public_batches'),
 ('allocation',helper,h.replace(b'let tokens = tokens.get(..leases.len())?;',b'let tokens = tokens.get(..leases.len())?.to_vec();'),'lease_batch_allocations',None),
 ('timer-wiring',services,s.replace(b'|lease| lease.validate_for_claim(lease_request),',b'|_lease| Ok(()),'),'lease_batch_identity','public_batches'),
]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');results=[]
try:
 for name,path,source,test,filter_name in mutants:
  for p,data in original.items():(root/p).write_bytes(data)
  assert source!=original[path];(root/path).write_bytes(source)
  command=['cargo','test','-p','penelope-executor','--test',test]
  if filter_name:command.append(filter_name)
  command+=['--','--nocapture']
  with (out/('negative-'+name+'.txt')).open('w') as log:r=subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT)
  text=(out/('negative-'+name+'.txt')).read_text();marker='duplicate index allocated' if name=='allocation' else 'assertion `left == right` failed'
  assert r.returncode!=0 and marker in text,(name,text)
  results.append({'name':name,'rejected':True,'marker':marker})
finally:
 for p,data in original.items():(root/p).write_bytes(data)
assert all((root/p).read_bytes()==data for p,data in original.items())
(out/'negative-results.json').write_text(json.dumps(results,indent=2)+'\n')
