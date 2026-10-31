import argparse,hashlib,json,os,platform,random,statistics,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--output',required=True);p.add_argument('--pairs',type=int,default=20);p.add_argument('--samples',type=int,default=32);p.add_argument('--dispatch-samples',type=int,default=16);args=p.parse_args()
root=Path(args.output);root.mkdir(parents=True,exist_ok=False)
paths={label:{bench:str(Path(f'target/fence-wait-{label}-{"fence" if bench=="fence-remote" else bench}').resolve()) for bench in ['fence','fence-remote','dispatch']} for label in ['baseline','candidate']}
records=[]
def run(label,bench,pair,warmup):
 command=['taskset','-c','0',paths[label][bench]]
 env={**os.environ,'TMPDIR':'/home/ac/projects/penelope/target/qualification-temp','PENELOPE_FENCE_SAMPLES':str(args.samples),'PENELOPE_DISPATCH_SAMPLES':str(args.dispatch_samples)}
 env.pop('PENELOPE_FENCE_WORKER_CPU',None)
 if bench=='fence-remote':env['PENELOPE_FENCE_WORKER_CPU']='1'
 began=time.monotonic_ns();result=subprocess.run(command,capture_output=True,text=True,env=env,timeout=90);elapsed=time.monotonic_ns()-began
 assert result.returncode==0,(command,result.stdout,result.stderr)
 data=json.loads(result.stdout);values={}
 if bench.startswith('fence'):
  assert data['samples_per_profile']==args.samples
  for key,rows in data['profiles_ns'].items():
   assert len(rows)==args.samples and all(v>0 for v in rows)
   values[bench+'/'+key]=statistics.median(rows)
 else:
  for profile in data['profiles']:
   rows=[r for r in profile['samples'] if not r['warmup']];assert len(rows)==args.dispatch_samples
   for field in ['setup_ns','intent_commit_ns','dispatch_start_return_ns','dispatch_to_host_checkpoint_ack_ns','dispatch_to_durable_completion_and_fence_ns']:
    values['dispatch/'+profile['profile']+'/'+field]=statistics.median(r[field] for r in rows)
 record=dict(label=label,bench=bench,pair=pair,warmup=warmup,command=command,elapsed_ns=elapsed,stdout=result.stdout,stderr=result.stderr,median_ns=values)
 records.append(record)
 with (root/'processes.jsonl').open('a') as f:f.write(json.dumps(record)+'\n')
for label in paths:
 for bench in paths[label]:run(label,bench,-1,True)
for pair in range(args.pairs):
 for label in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
  for bench in paths[label]:run(label,bench,pair,False)
 print('completed pair',pair+1,flush=True)
summary={};rng=random.Random(3030)
for key in sorted({key for r in records for key in r['median_ns']}):
 by_label={label:{r['pair']:r['median_ns'][key] for r in records if r['label']==label and key in r['median_ns'] and not r['warmup']} for label in paths}
 assert all(len(v)==args.pairs for v in by_label.values())
 ratios=[100*(by_label['candidate'][i]/by_label['baseline'][i]-1) for i in range(args.pairs)]
 boots=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(10000))
 summary[key]=dict(baseline_median_ns=statistics.median(by_label['baseline'].values()),candidate_median_ns=statistics.median(by_label['candidate'].values()),paired_median_percent=statistics.median(ratios),paired_bootstrap_95_percent=[boots[250],boots[9749]],all_paired_percent=ratios)
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
(root/'metadata.json').write_text(json.dumps(dict(pairs=args.pairs,fence_samples=args.samples,dispatch_samples=args.dispatch_samples,warmups_per_binary=1,cpu=0,worker_cpu_by_case={'fence':None,'fence-remote':1,'dispatch':None},cpu_topology={str(cpu):{field:Path(f'/sys/devices/system/cpu/cpu{cpu}/topology/{field}').read_text().strip() for field in ['core_id','physical_package_id','thread_siblings_list']} for cpu in [0,1]},platform=platform.platform(),rustc=subprocess.check_output(['rustc','-Vv'],text=True),binary_sha256={label:{bench:hashlib.sha256(Path(path).read_bytes()).hexdigest() for bench,path in binaries.items()} for label,binaries in paths.items()},scope='Process medians paired by alternating execution order. Fresh local WAL/FULL fixtures. Own compilation/tests excluded from timing; host load, frequency and cache state uncontrolled.'),indent=2)+'\n')
print(json.dumps(summary,indent=2))
