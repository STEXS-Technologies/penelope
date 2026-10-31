from pathlib import Path
import subprocess,os,json,platform,time,random,statistics
meta=json.loads(Path('target/replay-borrow-build.json').read_text());cpu=min(os.sched_getaffinity(0))
meta.update(cpu=cpu,kernel=platform.platform(),cpu_model=next(l.strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name')),pairs=24,order='alternating AB/BA; latency then separate allocation process for each variant',background='no own builds, tests or graph jobs during timing',warmup=100,iterations=1000)
Path('target/replay-borrow-metadata.json').write_text(json.dumps(meta,indent=2)+'\n');rows=[];values={};alloc={}
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
for pair in range(24):
 for variant in (['baseline','borrowed'] if pair%2==0 else ['borrowed','baseline']):
  for kind,name in [('latency','process_replay'),('allocations','process_replay_allocations')]:
   binary=meta['variants'][variant]['binaries'][name];started=time.time_ns()
   result=subprocess.run(['taskset','-c',str(cpu),binary['path']],env=env,text=True,capture_output=True,timeout=60,check=True)
   outputs=list(map(json.loads,result.stdout.splitlines()));assert len(outputs)==(15 if kind=='latency' else 80)
   for item in outputs:
    if kind=='latency':
     assert item['iterations']==1000
     values[f"{item['history']}/{item['profile']}",pair,variant]=item['elapsed_ns']/item['iterations']
    else:
     alloc[item['profile'],pair,variant,item['sample']]=item
   rows.append(dict(pair=pair,variant=variant,kind=kind,cpu=cpu,binary_sha256=binary['sha256'],started_unix_ns=started,stdout=result.stdout,stderr=result.stderr))
   Path('target/replay-borrow-processes.json').write_text(json.dumps(rows,indent=2)+'\n')
 print('completed pair',pair,flush=True)
summary={}
for metric in sorted({key[0] for key in values}):
 ratios=[values[metric,i,'borrowed']/values[metric,i,'baseline'] for i in range(24)];rng=random.Random(42)
 boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
 item={'mean_paired_change_pct':100*(statistics.mean(ratios)-1),'bootstrap95_pct':[100*(boot[i]-1) for i in (250,9749)],'mean_ns_per_call':{v:statistics.mean(values[metric,i,v] for i in range(24)) for v in ('baseline','borrowed')}}
 summary[metric]=item
 print(metric,json.dumps(item),flush=True)
allocation_summary={}
for profile in ('first','last','end','past','page'):
 for pair in range(24):
  for sample in range(16):
   baseline=alloc[profile,pair,'baseline',sample];borrowed=alloc[profile,pair,'borrowed',sample]
   assert baseline['rows']==borrowed['rows']
   assert baseline['allocations']-borrowed['allocations']==2*baseline['rows']
   assert baseline['requested_bytes']-borrowed['requested_bytes']==baseline['copied_bytes']==borrowed['copied_bytes']
   assert baseline['reallocations']==borrowed['reallocations']
 allocation_summary[profile]={variant:{metric:sorted({item[metric] for (p,i,v,s),item in alloc.items() if p==profile and v==variant}) for metric in ('allocations','reallocations','requested_bytes','rows','copied_bytes')} for variant in ('baseline','borrowed')}
assert len(rows)==96 and len(summary)==15 and len(alloc)==3840
Path('target/replay-borrow-summary.json').write_text(json.dumps({'latency':summary,'allocations':allocation_summary},indent=2)+'\n')
print('verified 96 processes, 15 latency metrics and 3840 allocation samples',flush=True)
