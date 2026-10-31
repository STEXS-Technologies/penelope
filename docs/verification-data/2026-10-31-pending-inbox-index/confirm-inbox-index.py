from pathlib import Path
import subprocess,os,json,time,random,statistics
meta=json.loads(Path('target/inbox-index-metadata.json').read_text());cpu=meta['cpu'];rows=[];values={}
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
for pair in range(24):
 for variant in (['baseline','indexed'] if pair%2==0 else ['indexed','baseline']):
  binary=meta['variants'][variant];started=time.time_ns()
  result=subprocess.run(['taskset','-c',str(cpu),binary['path']],env=env,text=True,capture_output=True,timeout=60,check=True)
  outputs=list(map(json.loads,result.stdout.splitlines()));assert [item['retained'] for item in outputs]==[0,128,1024]
  for data in outputs:
   n=data['retained'];assert data['schema_version']==(5 if variant=='baseline' else 6)
   assert data['consumed_rows']==n+16 and data['read_calls']==1000 and data['pending_calls']==16
   assert data['empty_engine']['identities']==[]
   assert data['pending_engine']['identities']==[f'inp_pending_{i:03}' for i in range(16)]
   for metric,count in [('open',1),('empty',1000),('pending',1000),('pending_accept',16),('pending_commit',16)]+([('seed_accept',n),('seed_commit',n)] if n else []):
    values[f'{n}/{metric}',pair,variant]=data[metric+'_ns']/count
  rows.append(dict(pair=pair,variant=variant,cpu=cpu,binary_sha256=binary['binary_sha256'],started_unix_ns=started,stdout=result.stdout,stderr=result.stderr))
  Path('target/inbox-index-confirmation-processes.json').write_text(json.dumps(rows,indent=2)+'\n')
 print('completed pair',pair,flush=True)
summary={}
for metric in sorted({key[0] for key in values}):
 ratios=[values[metric,i,'indexed']/values[metric,i,'baseline'] for i in range(24)];rng=random.Random(42)
 boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
 item={'mean_paired_change_pct':100*(statistics.mean(ratios)-1),'bootstrap95_pct':[100*(boot[i]-1) for i in (250,9749)],'mean_ns_per_call':{v:statistics.mean(values[metric,i,v] for i in range(24)) for v in ('baseline','indexed')}}
 summary[metric]=item
 print(metric,json.dumps(item),flush=True)
assert len(rows)==48 and len(summary)==19
Path('target/inbox-index-confirmation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
