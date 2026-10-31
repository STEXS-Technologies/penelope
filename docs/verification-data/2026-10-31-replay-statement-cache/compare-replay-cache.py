from pathlib import Path
import subprocess,os,json,platform,time,random,statistics,itertools
meta=json.loads(Path('target/replay-cache-build.json').read_text());cpu=min(os.sched_getaffinity(0));orders=list(itertools.permutations(meta['variants']))
meta.update(cpu=cpu,kernel=platform.platform(),cpu_model=next(l.strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name')),blocks=24,order=[list(orders[i%6]) for i in range(24)],background='no own builds, tests or graph jobs during timing',warmup=100,iterations=1000)
Path('target/replay-cache-metadata.json').write_text(json.dumps(meta,indent=2)+'\n');rows=[];values={}
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
for block in range(24):
 for variant in orders[block%6]:
  binary=meta['variants'][variant];started=time.time_ns()
  result=subprocess.run(['taskset','-c',str(cpu),binary['path']],env=env,text=True,capture_output=True,timeout=60,check=True)
  outputs=list(map(json.loads,result.stdout.splitlines()));assert len(outputs)==15
  for item in outputs:
   assert item['iterations']==1000
   values[f"{item['history']}/{item['profile']}",block,variant]=item['elapsed_ns']/item['iterations']
  rows.append(dict(block=block,variant=variant,cpu=cpu,binary_sha256=binary['binary_sha256'],started_unix_ns=started,stdout=result.stdout,stderr=result.stderr))
  Path('target/replay-cache-processes.json').write_text(json.dumps(rows,indent=2)+'\n')
 print('completed block',block,flush=True)
summary={}
for comparison,baseline,candidate in [('frontier_cost','pre_frontier','frontier'),('cache_gain','frontier','cached'),('net','pre_frontier','cached')]:
 for metric in sorted({key[0] for key in values}):
  ratios=[values[metric,i,candidate]/values[metric,i,baseline] for i in range(24)];rng=random.Random(42)
  boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
  item={'mean_paired_change_pct':100*(statistics.mean(ratios)-1),'bootstrap95_pct':[100*(boot[i]-1) for i in (250,9749)],'mean_ns_per_call':{v:statistics.mean(values[metric,i,v] for i in range(24)) for v in (baseline,candidate)}}
  summary[comparison+'/'+metric]=item
  print(comparison,metric,json.dumps(item),flush=True)
assert len(rows)==72 and len(summary)==45
Path('target/replay-cache-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
