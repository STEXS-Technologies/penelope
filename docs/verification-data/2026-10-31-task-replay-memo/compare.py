import pathlib,subprocess,os,time,json,platform
out=pathlib.Path(__file__).resolve().parent
profiles=[('commits',False,'task_blob_commits'),('commits',True,'task_blob_commits'),('reconstruction',False,'task_reconstruction'),('reconstruction',True,'task_reconstruction'),('allocations',False,'task_reconstruction_allocations'),('allocations',True,'task_reconstruction_allocations')]
with (out/'runs.jsonl').open('w') as log:
 for pair in range(24):
  order=['baseline','candidate'] if pair%2==0 else ['candidate','baseline']
  for variant in order:
   for kind,changed,bench in profiles:
    env=dict(os.environ)
    for flag in ['PENELOPE_BENCH_EXTERNAL_COMMIT','PENELOPE_BENCH_ALTERNATING_BLOBS']:env.pop(flag,None)
    if changed:env['PENELOPE_BENCH_EXTERNAL_COMMIT' if kind=='commits' else 'PENELOPE_BENCH_ALTERNATING_BLOBS']='1'
    started=time.time();process=subprocess.run(['taskset','-c','0',str(out/(variant+'-'+bench))],env=env,text=True,capture_output=True,check=True)
    rows=[json.loads(line) for line in process.stdout.splitlines()]
    assert len(rows)==(48 if kind=='allocations' else 3 if changed or kind=='reconstruction' else 4)
    log.write(json.dumps(dict(pair=pair,variant=variant,kind=kind,changed=changed,started=started,rows=rows))+'\n');log.flush()
  print('pair',pair+1,flush=True)
(out/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),cpu=subprocess.check_output(['lscpu'],text=True),rustc=subprocess.check_output(['rustc','-Vv'],text=True),cpu_affinity=0,pairs=24),indent=2)+'\n')
