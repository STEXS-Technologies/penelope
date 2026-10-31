from pathlib import Path
import subprocess,os,json,platform,time,random,statistics,itertools
meta=json.loads(Path('target/payload-borrow-final-build.json').read_text());cpu=min(os.sched_getaffinity(0));orders=list(itertools.permutations(meta['variants']))
meta.update(cpu=cpu,kernel=platform.platform(),cpu_model=next(l.strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name')),pairs=24,order=[list(orders[i%6]) for i in range(24)],background='no own builds, tests or graph jobs during timing',sizes=[0,32,4096,1048576],iterations='1000 below 1 MiB; 128 at 1 MiB',warmup='100 below 1 MiB; 32 at 1 MiB; allocation fixture 16 warmups and 16 samples per condition')
Path('target/payload-borrow-final-metadata.json').write_text(json.dumps(meta,indent=2)+'\n');rows=[];values={};alloc={}
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
for pair in range(24):
 for variant in orders[pair%6]:
  for kind,name in [('latency','process_payloads'),('allocations','process_payload_allocations')]:
   binary=meta['variants'][variant]['binaries'][name];started=time.time_ns()
   result=subprocess.run(['taskset','-c',str(cpu),binary['path']],env=env,text=True,capture_output=True,timeout=60,check=True)
   outputs=list(map(json.loads,result.stdout.splitlines()));assert len(outputs)==(20 if kind=='latency' else 320)
   for data in outputs:
    size=data['size'];case=data['case'];metric=f'{size}/{case}'
    if kind=='latency':
     assert data['iterations']==(128 if size==1048576 else 1000)
     values[metric,pair,variant]=data['elapsed_ns']/data['iterations']
    else:alloc[metric,pair,variant,data['sample']]=data
   rows.append(dict(pair=pair,variant=variant,kind=kind,cpu=cpu,binary_sha256=binary['sha256'],started_unix_ns=started,stdout=result.stdout,stderr=result.stderr))
   Path('target/payload-borrow-final-processes.json').write_text(json.dumps(rows,indent=2)+'\n')
 print('completed pair',pair,flush=True)
summary={}
for comparison,baseline,candidate in [('nested_vs_baseline','baseline','nested'),('borrowed_vs_baseline','baseline','borrowed'),('borrowed_vs_nested','nested','borrowed')]:
 for metric in sorted({key[0] for key in values}):
  ratios=[values[metric,i,candidate]/values[metric,i,baseline] for i in range(24)];rng=random.Random(42)
  boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
  item={'mean_paired_change_pct':100*(statistics.mean(ratios)-1),'bootstrap95_pct':[100*(boot[i]-1) for i in (250,9749)],'mean_ns_per_call':{v:statistics.mean(values[metric,i,v] for i in range(24)) for v in (baseline,candidate)}}
  summary[comparison+'/'+metric]=item
  print(comparison,metric,json.dumps(item),flush=True)
allocation_summary={}
for metric in sorted({key[0] for key in values}):
 for pair in range(24):
  for sample in range(16):
   for variant in ('nested','borrowed'):
    baseline=alloc[metric,pair,'baseline',sample];candidate=alloc[metric,pair,variant,sample]
    removed=baseline['removed_bytes'];assert removed==candidate['removed_bytes']
    assert baseline['allocations']-candidate['allocations']==int(removed>0)
    assert baseline['requested_bytes']-candidate['requested_bytes']==removed
    assert baseline['reallocations']==candidate['reallocations']
 allocation_summary[metric]={variant:{key:sorted({item[key] for (m,p,v,s),item in alloc.items() if m==metric and v==variant}) for key in ('allocations','reallocations','requested_bytes','removed_bytes')} for variant in ('baseline','nested','borrowed')}
assert len(rows)==144 and len(summary)==60 and len(alloc)==23040
Path('target/payload-borrow-final-summary.json').write_text(json.dumps({'latency':summary,'allocations':allocation_summary},indent=2)+'\n')
print('verified 144 processes, 60 latency metrics, 23040 allocation samples',flush=True)
