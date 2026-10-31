"""Verify historical workflow evidence without running timing or changing sources."""
from pathlib import Path
import copy,gzip,hashlib,importlib.util,json,subprocess,sys
sys.dont_write_bytecode=True
out=Path(__file__).resolve().parent;root=out.parents[2]
def read(directory,name):
 p=directory/name
 return p.read_bytes() if p.exists() else gzip.decompress((directory/(name+'.gz')).read_bytes())
def sha(data):return hashlib.sha256(data).hexdigest()
manifest=json.loads((out/'manifest.json').read_text())
assert set(manifest)=={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'}
for path,digest in manifest.items():assert sha((out/path).read_bytes())==digest,path
metadata=json.loads(read(out,'metadata.json'))
assert metadata['git_head']=='a44da2ece54056bddd07ff69a3700610aa107ebe'
for path,digest in metadata['source_sha256'].items():assert sha((root/path).read_bytes())==digest,path
assert (root/'scripts/benchmark-durable-timers.py').read_bytes()==(out/'benchmark-durable-timers.py').read_bytes()
for path,digest in json.loads((out/'source-hashes.json').read_text()).items():assert sha((root/path).read_bytes())==digest,path
for path,digest in json.loads((out/'runtime-source-hashes.json').read_text()).items():
 assert sha((root/path).read_bytes())==digest,path
 assert sha(subprocess.check_output(['git','show',metadata['git_head']+':'+path],cwd=root))==digest,path
spec=importlib.util.spec_from_file_location('durable_timer_summary',out/'benchmark-durable-timers.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
rows=[json.loads(line) for line in read(out,'samples.jsonl').decode().splitlines()]
summary=module.summarize(rows,metadata)
assert (json.dumps(summary,indent=2)+'\n').encode()==read(out,'summary.json')
assert summary['sample_count']==288 and summary['timer_count']==13920 and len(summary['profiles'])==24
for change in ['missing','duplicate','unverified','phase-sum','order','checksum','truncated']:
 broken=copy.deepcopy(rows)
 if change=='missing':broken.pop()
 elif change=='duplicate':broken.append(copy.deepcopy(broken[0]))
 elif change=='unverified':broken[0]['verified']=False
 elif change=='phase-sum':broken[0]['managed_ns']+=1
 elif change=='order':broken[0]['direct_first']=not broken[0]['direct_first']
 elif change=='checksum':broken[0]['checksum']^=1
 elif change=='truncated':next(row for row in broken if row['batch']==128)['consumed']=64
 try:module.summarize(broken,metadata)
 except ValueError:pass
 else:raise AssertionError('malformed evidence accepted: '+change)
negative=json.loads(read(out/'negative','results.json'))
assert {r['name'] for r in negative['results']}=={'result','truncated','identity','not-due'}
for result in negative['results']:
 assert result['rejected'] and result['marker'] in read(out/'negative',result['name']+'.txt').decode()
for path,digest in negative['restored_sha256'].items():assert sha((root/path).read_bytes())==digest
assert json.loads(read(out/'negative','restored.txt'))['consumed']==128
q=out/'qualification';results=json.loads(read(q,'results.json'))
assert set(results)=={'workspace','msrv','clippy','msrv-check','fmt','layers','graph'} and all(r['exit_code']==0 for r in results.values())
for name in ['workspace','msrv']:
 text=read(q,name+'.txt').decode()
 assert '0 failed' in text and 'test result: FAILED' not in text
 assert 'test durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs ... ok' in text
for name in ['clippy','msrv-check']:assert 'Finished' in read(q,name+'.txt').decode() and 'error:' not in read(q,name+'.txt').decode()
assert read(q,'fmt.txt')==read(q,'layers.txt')==b'' and b'Code graph updated.' in read(q,'graph.txt')
smoke=json.loads(read(q,'smoke.json'));assert smoke['invalid_cases_rejected']==10 and smoke['msrv_rows']==2 and smoke['msrv_timers']==256
for row in [json.loads(line) for line in read(q,'msrv-smoke.jsonl').decode().splitlines()]:
 assert row['verified'] and row['batch']==128 and row['width']==128 and row['reopen'] and row['consumed']==128
print('durable timer evidence verified: 24 profiles, 288 workflows, 13,920 timers, exact summaries, 7 malformed datasets rejected, 4 workload fault injections rejected, Rust 1.85 smoke, unchanged production sources')
