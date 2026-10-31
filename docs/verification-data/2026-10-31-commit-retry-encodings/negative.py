"""Temporarily break each constituent verifier; always restore exact runtime bytes."""
from pathlib import Path
import hashlib,json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent/'negative';out.mkdir(exist_ok=True)
path=root/'crates/penelope-sqlite/src/process.rs';original=path.read_text();start=original.index('fn verify_committed_batch(');end=original.index('\n#[cfg(test)]',start);body=original[start:end]
segments=[('outcome','ProcessOutcome','outcome',body.index('    {\n        let mut statement')),('action','penelope_domain::ProcessAction','action',body.index('    if !commit.actions.is_empty()')),('input','ProcessInput','input',body.index('    if let Some(input)'))]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');results=[]
try:
 for index,(component,kind,value,begin) in enumerate(segments):
  finish=segments[index+1][3] if index<2 else len(body);part=body[begin:finish];condition=f'if decode::<{kind}>(bytes, checksum)? != *{value} {{';assert part.count(condition)==1
  faults={
   'checksum':part.replace(condition,f'if serde_json::from_slice::<{kind}>(bytes).map_err(|_codec| PortError::Invariant)? != *{value} {{'),
   'semantic':part.replace(condition,'if false {'),
   'canonical-only':part.replace(condition,f'if bytes != serde_json::to_vec({value}).map_err(|_codec| PortError::Invariant)? {{'),
   'bound':part.replace('column_bytes(row, 0, limits.max_record_bytes)','column_bytes(row, 0, u32::MAX)'),
  }
  for fault,edited in faults.items():
   assert edited!=part
   changed=body[:begin]+edited+body[finish:];path.write_text(original[:start]+changed+original[end:])
   test='commit_retry_encodings::'+('retries_bound_noncanonical_constituents_before_semantic_comparison' if fault=='bound' else 'retries_preserve_noncanonical_constituents_and_reject_changed_evidence')
   result=subprocess.run(['cargo','test','--locked','-p','penelope-sqlite','--test','process',test,'--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
   name=component+'-'+fault;(out/(name+'.txt')).write_text(result.stdout)
   assert result.returncode and 'test result: FAILED' in result.stdout and test in result.stdout,(name,result.stdout)
   # The panic must identify the intended constituent rather than an earlier case.
   assert component+'/' in result.stdout,(name,result.stdout)
   results.append(dict(name=name,test=test,rejected=True));print(name+' rejected',flush=True)
finally:path.write_text(original)
assert path.read_text()==original
result=subprocess.run(['cargo','test','--locked','-p','penelope-sqlite','--test','process','commit_retry_encodings','--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,check=True);(out/'restored.txt').write_text(result.stdout)
(out/'results.json').write_text(json.dumps(dict(results=results,restored_sha256=hashlib.sha256(path.read_bytes()).hexdigest()),indent=2)+'\n')
