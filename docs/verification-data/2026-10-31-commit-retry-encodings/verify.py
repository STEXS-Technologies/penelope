"""Validate source-pinned compatibility and corruption evidence."""
from pathlib import Path
import gzip,hashlib,json,subprocess
out=Path(__file__).resolve().parent;root=out.parents[2]
def sha(data):return hashlib.sha256(data).hexdigest()
def read(folder,name):
 p=folder/name
 return p.read_bytes() if p.exists() else gzip.decompress((folder/(name+'.gz')).read_bytes())
manifest=json.loads(read(out,'manifest.json'))
assert set(manifest)=={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'}
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
sources=json.loads(read(out,'source-hashes.json'))
for path,digest in sources.items():assert sha((root/path).read_bytes())==digest,path
metadata=json.loads(read(out,'metadata.json'));assert metadata['production_changed'] is False and metadata['cases']==60 and metadata['negative_controls']==12
runtime='crates/penelope-sqlite/src/process.rs';assert sha(subprocess.check_output(['git','show',metadata['parent']+':'+runtime],cwd=root))==sources[runtime]
negative=out/'negative';results=json.loads(read(negative,'results.json'))
assert len(results['results'])==12 and {r['name'] for r in results['results']}=={component+'-'+fault for component in ['outcome','action','input'] for fault in ['checksum','semantic','canonical-only','bound']}
for result in results['results']:
 text=read(negative,result['name']+'.txt').decode();component=result['name'].split('-')[0]
 assert result['rejected'] and result['test'] in text and 'test result: FAILED' in text and component+'/' in text,result['name']
assert results['restored_sha256']==sources[runtime]
markers=['commit_retry_encoding_cases=48','commit_retry_bound_cases=12']
for log in [read(out,'focused.txt'),read(negative,'restored.txt')]:
 text=log.decode();assert '2 passed; 0 failed' in text and 'test result: FAILED' not in text
 for marker in markers:assert marker in text
q=out/'qualification';results=json.loads(read(q,'results.json'))
assert set(results)=={'workspace','msrv','clippy','msrv-check','fmt','layers','graph'} and all(r['exit_code']==0 for r in results.values())
for name in ['workspace','msrv']:
 text=read(q,name+'.txt').decode();assert '78 passed; 0 failed' in text and 'test result: FAILED' not in text
 for marker in ['test commit_retry_encodings::retries_preserve_noncanonical_constituents_and_reject_changed_evidence ... ok','test commit_retry_encodings::retries_bound_noncanonical_constituents_before_semantic_comparison ... ok','test durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs ... ok']:assert marker in text,marker
for name in ['clippy','msrv-check']:assert b'Finished' in read(q,name+'.txt') and b'error:' not in read(q,name+'.txt')
assert read(q,'fmt.txt')==read(q,'layers.txt')==b'' and b'Code graph updated.' in read(q,'graph.txt')
supplemental=json.loads(read(out,'supplemental.json'))
assert set(supplemental)=={'no-std','no-std-serde','focused'} and all(r['exit_code']==0 for r in supplemental.values())
for name in ['no-std','no-std-serde']:assert b'Finished' in read(out,name+'.txt') and b'error:' not in read(out,name+'.txt')
print('commit retry evidence verified: 48 encoding/corruption cases, 12 exact-boundary cases, 12 rejected constituent-specific controls, unchanged runtime source, stable/MSRV qualification')
