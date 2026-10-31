"""Read-only checks for durable batch fault/reopen/retry evidence."""
from pathlib import Path
import gzip,hashlib,json,re,subprocess
out=Path(__file__).resolve().parent;root=out.parents[2]
def read(name):
 p=out/name
 return p.read_bytes() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes())
def sha(data):return hashlib.sha256(data).hexdigest()
manifest=json.loads(read('manifest.json'));assert set(manifest)=={p.name for p in out.iterdir() if p.is_file() and p.name!='manifest.json'}
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
sources=json.loads(read('source-hashes.json'))
for name,digest in sources.items():assert sha((root/name).read_bytes())==digest,name
parent='c69cebc8594094f80fee54d5fdc9c88bb7ef3f1d'
assert subprocess.check_output(['git','diff',parent,'--',':(glob)crates/*/src/**',':(glob)crates/*/Cargo.toml','Cargo.lock'],cwd=root)==b''
assert subprocess.check_output(['git','ls-files','--others','--exclude-standard','--',':(glob)crates/*/src/**'],cwd=root)==b''
old=subprocess.check_output(['git','show','00b79aaf84e59ce1a74fd2c6b75bd5dc099c0650:crates/penelope-executor/src/services.rs'],cwd=root)
function=old[old.index(b'/// Stable timer inbox input'):old.index(b'\n/// Drains one authorized')].replace(b'fn timer_input(',b'pub(super) fn timer_input(')
legacy=(root/'crates/penelope-executor/tests/support/timer_input_legacy.rs').read_bytes();assert legacy[legacy.index(b'/// Stable timer inbox input'):].strip()==function.strip()
faults=['PayloadBefore','PayloadAfter','AcceptBefore','AcceptAfter','AckBefore','AckAfter','ClockAfterAccept','ReceiptMismatch']
expected={f'{n}-{w}-{i}-{fault}' for w,n in [(9,17),(128,128)] for i in [0,n//2,n-1] for fault in faults}
runs=0
for toolchain in ['stable','msrv']:
 m=json.loads(read(toolchain+'-results.json'));assert m['toolchain']==toolchain and len(m['runs'])==2
 if toolchain=='msrv':assert 'rustc 1.85.0' in m['rustc']
 for name,digest in m['source_sha256'].items():assert sources[name]==digest
 for repeat,r in enumerate(m['runs']):
  assert r=={'repeat':repeat,'log':f'{toolchain}-{repeat}.txt','cases':48,'verified':True}
  text=read(r['log']).decode();cases=re.findall(r'timer_batch_case=(\S+) verified=true',text)
  assert len(cases)==48 and set(cases)==expected and '1 passed; 0 failed' in text
  assert 'timer_batch_recovery_cases=48 durable_reopen=true stable_inputs=true exact_partial_reports=true consume_once=true public_discovery=true' in text;runs+=1
assert runs==4
negative=json.loads(read('negative-results.json'));assert {r['name'] for r in negative['results']}=={'report','receipt','identity','expiry','continue-after-failure','consume-twice'}
for r in negative['results']:
 assert r['rejected'] and r['case'] in expected
 text=read('negative-'+r['name']+'.txt').decode();assert 'panicked at' in text and r['marker'] in text and 'test result: FAILED' in text
for name,digest in negative['restored_sha256'].items():assert sources[name]==digest==sha(subprocess.check_output(['git','show',parent+':'+name],cwd=root))
for name in ['stable-workspace.txt','msrv-executor.txt']:
 text=read(name).decode();assert '0 failed' in text and 'test result: FAILED' not in text
 assert 'test durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs ... ok' in text
for name in ['final-clippy.txt','msrv-check.txt']:
 text=read(name).decode();assert 'Finished' in text and 'error:' not in text
assert read('fmt.txt')==b'' and read('layers.txt')==b'' and b'Code graph updated.' in read('graph.txt')
q=json.loads(read('qualification.json'));assert q.pop('parent')==parent and q.pop('production_unchanged') is True
assert q.pop('explicit_cases')==192 and q.pop('consumed_timer_inputs')==13920 and q.pop('negative_controls')==6
assert all(value==0 for value in q.values())
print('timer batch evidence verified: 4 runs, 192 fault/reopen/retry cases, 13,920 timer consumptions and duplicate commit retries, 6 rejected negative controls; production unchanged')
