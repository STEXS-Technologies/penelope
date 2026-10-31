import subprocess,json,os,statistics,random
from pathlib import Path
cpu=min(os.sched_getaffinity(0))
rows=[]
for pair in range(20):
 for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
  p=subprocess.run(['taskset','-c',str(cpu),'target/claim-cache-'+variant],text=True,capture_output=True,check=True,timeout=30)
  row=dict(pair=pair,variant=variant,cpu=cpu,stdout=p.stdout)
  rows.append(row)
Path('target/claim-cache-results.json').write_text(json.dumps(rows,indent=2)+'\n')
for timers in ['false','true']:
 values={}
 for row in rows:
  line=next(s for s in row['stdout'].splitlines() if s.startswith('timers='+timers+' '))
  values[row['pair'],row['variant']]=int(line.split('elapsed_ns=')[1])
 ratios=[values[i,'candidate']/values[i,'baseline'] for i in range(20)]
 rng=random.Random(42)
 boots=sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
 print(timers, 'paired_mean_change_pct',100*(statistics.mean(ratios)-1),'bootstrap95',100*(boots[250]-1),100*(boots[9749]-1),flush=True)
