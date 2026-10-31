from pathlib import Path
import subprocess,os,json,time,random,statistics
meta=json.loads(Path('target/replay-cache-metadata.json').read_text());cpu=meta['cpu'];rows=[];values={}
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
for pair in range(24):
 for variant in (['frontier','cached'] if pair%2==0 else ['cached','frontier']):
  binary=meta['variants'][variant];started=time.time_ns()
  result=subprocess.run(['taskset','-c',str(cpu),binary['path']],env=env,text=True,capture_output=True,timeout=60,check=True)
  outputs=list(map(json.loads,result.stdout.splitlines()));assert len(outputs)==15
  for item in outputs:
   assert item['iterations']==1000
   values[f"{item['history']}/{item['profile']}",pair,variant]=item['elapsed_ns']/item['iterations']
  rows.append(dict(pair=pair,variant=variant,cpu=cpu,binary_sha256=binary['binary_sha256'],started_unix_ns=started,stdout=result.stdout,stderr=result.stderr))
  Path('target/replay-cache-confirmation-processes.json').write_text(json.dumps(rows,indent=2)+'\n')
 print('completed pair',pair,flush=True)
summary={}
for metric in sorted({key[0] for key in values}):
 ratios=[values[metric,i,'cached']/values[metric,i,'frontier'] for i in range(24)];rng=random.Random(42)
 boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
 item={'mean_paired_change_pct':100*(statistics.mean(ratios)-1),'bootstrap95_pct':[100*(boot[i]-1) for i in (250,9749)],'mean_ns_per_call':{v:statistics.mean(values[metric,i,v] for i in range(24)) for v in ('frontier','cached')}}
 summary[metric]=item
 print(metric,json.dumps(item),flush=True)
assert len(rows)==48 and len(summary)==15
Path('target/replay-cache-confirmation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
