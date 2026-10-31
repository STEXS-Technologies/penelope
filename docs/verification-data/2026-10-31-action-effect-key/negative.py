from pathlib import Path
import os,subprocess,json
root=Path.cwd();out=Path(__file__).resolve().parent
names=['crates/penelope-domain/src/implementation.rs','crates/penelope-sqlite/src/leases.rs','crates/penelope-sqlite/src/process.rs','crates/penelope-executor/src/services.rs']
original={p:(root/p).read_bytes() for p in names}
parent=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
env=os.environ|{'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(root/'target/qualification-temp'),'RUST_TEST_THREADS':'1'}
results={}
def run(name,command,marker):
 result=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 (out/('negative-'+name+'.txt')).write_bytes(result.stdout)
 assert result.returncode!=0 and marker.encode() in result.stdout
 results[name]=result.returncode
try:
 for p in names:(root/p).write_bytes(subprocess.check_output(['git','show',parent+':'+p]))
 run('public-budget',['cargo','test','-p','penelope-sqlite','--test','lease_claim_profile_allocations','--','outbox'],'outbox: allocations=')
 for p,data in original.items():(root/p).write_bytes(data)
 p=names[0];text=original[p].decode();needle='bytes.extend_from_slice(&payload_digest.0);';assert text.count(needle)==1
 (root/p).write_text(text.replace(needle,'bytes.extend_from_slice(&[0; 32]);'))
 run('encoding',['cargo','test','-p','penelope-domain','--test','effect_key_encoding','--','--nocapture'],'test result: FAILED')
 (root/p).write_bytes(original[p])
 p=names[1];text=original[p].decode();needle='|| stored_key.as_blob().map_err(|_type| PortError::Invariant)? != action.canonical_effect_key_bytes()';assert text.count(needle)==1
 (root/p).write_text(text.replace(needle,''))
 run('integrity',['cargo','test','-p','penelope-sqlite','--test','process','damaged_index_bindings_never_return_an_unpersisted_lease','--','--nocapture'],'test result: FAILED')
finally:
 for p,data in original.items():(root/p).write_bytes(data)
(out/'negative-results.json').write_text(json.dumps(results,indent=2)+'\n')
print(results)
