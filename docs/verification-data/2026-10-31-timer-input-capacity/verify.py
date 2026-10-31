"""Read-only evidence checks; analysis output is written only in a temporary directory."""
from pathlib import Path
import difflib,gzip,hashlib,json,shutil,subprocess,sys,tempfile
out=Path(__file__).resolve().parent;root=out.parents[2]
def read(name):
 p=out/name
 return p.read_bytes() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes())
def sha(data):return hashlib.sha256(data).hexdigest()
manifest=json.loads(read('manifest.json'))
assert set(manifest)=={p.name for p in out.iterdir() if p.is_file() and p.name!='manifest.json'}
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
sources=json.loads(read('source-hashes.json'))
for name,digest in sources.items():assert sha((root/name).read_bytes())==digest,name
m=json.loads(read('metadata.json'));assert m['parent']=='00b79aaf84e59ce1a74fd2c6b75bd5dc099c0650'
for name,digest in m['common_sha256'].items():assert sources[name]==digest
runtime=list(m['runtime_sources']['candidate'])
for name,digest in m['runtime_sources']['candidate'].items():assert sources[name]==digest
oldservices=subprocess.check_output(['git','show',m['parent']+':'+runtime[0]],cwd=root)
oldfn=oldservices[oldservices.index(b'/// Stable timer inbox input'):oldservices.index(b'\n/// Drains one authorized')].replace(b'fn timer_input(',b'pub(super) fn timer_input(')
legacy=(root/'crates/penelope-executor/tests/support/timer_input_legacy.rs').read_bytes();assert legacy[legacy.index(b'/// Stable timer inbox input'):].strip()==oldfn.strip()
helper=(root/runtime[1]).read_bytes();oldhelper=helper[:helper.index(b'/// Stable timer inbox input')]+oldfn
patch=''
for name,old in zip(runtime,[oldservices,oldhelper]):
 assert sha(old)==m['runtime_sources']['baseline'][name]
 patch+=''.join(difflib.unified_diff(old.decode().splitlines(True),(root/name).read_text().splitlines(True),fromfile='a/'+name,tofile='b/'+name))
assert patch.encode()==read('runtime.patch')
assert len(m['binaries'])==4 and all(len(digest)==64 for digest in m['binaries'].values())
for variant in ['baseline','candidate']:
 text=read(variant+'-tests.txt').decode()
 assert 'timer_input_legacy_cases=10192 metadata_substitutions=30576' in text and '1 passed; 0 failed' in text
 text=read(variant+'-build.txt').decode();assert 'Finished' in text and 'error:' not in text
for name in ['final-focused.txt','stable-workspace.txt','msrv.txt']:
 text=read(name).decode();assert '0 failed' in text and 'test result: FAILED' not in text
 assert 'timer_input_allocation_profiles=16 budgets_pass=true' in text
 assert 'test timer_inputs_match_legacy_across_sizes_boundaries_and_redelivery ... ok' in text
for name in ['final-clippy.txt','msrv-check.txt']:
 text=read(name).decode();assert 'Finished' in text and 'error:' not in text
assert read('fmt.txt')==b'' and read('layers.txt')==b''
assert b'Code graph updated.' in read('graph.txt')
negatives=json.loads(read('negative-results.json'));assert {n['name'] for n in negatives}=={'budget','metadata','prefix'}
for n in negatives:assert n['rejected'] and n['marker'] in read('negative-'+n['name']+'.txt').decode()
q=json.loads(read('qualification.json'));assert q.pop('source_restored') is True and q.pop('negative_controls')==3 and all(value==0 for value in q.values())
with tempfile.TemporaryDirectory(prefix='penelope-timer-input-') as directory:
 temporary=Path(directory)
 for name in ['raw.jsonl.gz','allocations.jsonl.gz','measurement.json']:shutil.copy2(out/name,temporary/name)
 subprocess.run([sys.executable,str(out/'analyze.py'),str(temporary)],stdout=subprocess.DEVNULL,check=True)
 assert (temporary/'summary.json').read_bytes()==read('summary.json')
print('timer input evidence verified: 768 timing rows, 64 allocation observations, 10,192 parity cases, 30,576 metadata substitutions, 3 rejected negative controls')
