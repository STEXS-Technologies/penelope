import pathlib,subprocess,os,time,json,platform,itertools
out=pathlib.Path(__file__).resolve().parent
profiles=[('commits',False,False,False,'task_blob_commits'),('commits',True,False,False,'task_blob_commits')]
for kind,bench in [('reconstruction','task_reconstruction'),('allocations','task_reconstruction_allocations')]:
 for reformatted,alternating in itertools.product([False,True],repeat=2):profiles.append((kind,False,alternating,reformatted,bench))
with (out/'runs.jsonl').open('w') as log:
 for pair in range(24):
  for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
   for kind,external,alternating,reformatted,bench in profiles:
    env=dict(os.environ)
    for flag in ['PENELOPE_BENCH_EXTERNAL_COMMIT','PENELOPE_BENCH_ALTERNATING_BLOBS','PENELOPE_BENCH_REFORMATTED_SCOPE']:env.pop(flag,None)
    for flag,enabled in [('PENELOPE_BENCH_EXTERNAL_COMMIT',external),('PENELOPE_BENCH_ALTERNATING_BLOBS',alternating),('PENELOPE_BENCH_REFORMATTED_SCOPE',reformatted)]:
     if enabled:env[flag]='1'
    started=time.time();result=subprocess.run(['taskset','-c','0',str(out/(variant+'-'+bench))],env=env,text=True,capture_output=True,check=True)
    rows=[json.loads(line) for line in result.stdout.splitlines()];assert len(rows)==(48 if kind=='allocations' else 3 if external or kind=='reconstruction' else 4)
    log.write(json.dumps(dict(pair=pair,variant=variant,kind=kind,external=external,alternating=alternating,reformatted=reformatted,started=started,rows=rows))+'\n');log.flush()
  print('pair',pair+1,flush=True)
(out/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),cpu=subprocess.check_output(['lscpu'],text=True),rustc=subprocess.check_output(['rustc','-Vv'],text=True),cpu_affinity=0,pairs=24),indent=2)+'\n')
