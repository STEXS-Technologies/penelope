import subprocess,json,os,statistics,random,re,time,hashlib
from pathlib import Path
cpu=min(os.sched_getaffinity(0))
rows=[]
for pair in range(20):
 for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
  started=time.time_ns()
  p=subprocess.run(['taskset','-c',str(cpu),'target/token-cache-'+variant],text=True,capture_output=True,check=True,timeout=60,env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())})
  rows.append(dict(pair=pair,variant=variant,cpu=cpu,started_unix_ns=started,stdout=p.stdout,stderr=p.stderr,binary_sha256=hashlib.sha256(Path('target/token-cache-'+variant).read_bytes()).hexdigest()))
  Path('target/token-cache-results.json').write_text(json.dumps(rows,indent=2)+'\n')
 for metric in ['timers=false batch=1','timers=false batch=16','timers=true batch=1','timers=true batch=16']:
  pass
 print('completed pair',pair,flush=True)
summary={}
for timers in ['false','true']:
 for batch in [1,16]:
  values={}
  for row in rows:
   line=next(s for s in row['stdout'].splitlines() if s.startswith(f'timers={timers} batch={batch} '))
   match=re.fullmatch(r'timers=(false|true) batch=(1|16) iterations=400 elapsed_ns=(\d+)',line)
   assert match,line
   values[row['pair'],row['variant']]=int(match[3])
  ratios=[values[i,'candidate']/values[i,'baseline'] for i in range(20)]
  rng=random.Random(42)
  boot=sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
  data=dict(mean_paired_change_pct=100*(statistics.mean(ratios)-1),bootstrap95_pct=[100*(boot[i]-1) for i in [250,9749]],mean_ns_per_batch={v:statistics.mean(values[i,v]/400 for i in range(20)) for v in ['baseline','candidate']})
  summary[f'timers={timers} batch={batch}']=data
  print(timers,batch,data,flush=True)
Path('target/token-cache-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
