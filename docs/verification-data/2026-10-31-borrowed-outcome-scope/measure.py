import sys
from pathlib import Path
import subprocess,os,json,time,platform
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
if (out/'raw.jsonl').exists() or (out/'raw.jsonl.gz').exists(): raise FileExistsError('timing output already exists')
cpu=min(os.sched_getaffinity(0))
env=dict(os.environ,TMPDIR=str(Path.cwd()/'target/qualification-temp'))
rows=[]
start=time.time()
with (out/'raw.jsonl').open('w') as log:
 for campaign in range(2):
  for block in range(6):
   order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
   for slot,label in enumerate(order):
    for scope in ['short','long']:
     local=env.copy()
     local.pop('PENELOPE_REPLAY_LONG_SCOPE',None)
     if scope=='long':local['PENELOPE_REPLAY_LONG_SCOPE']='1'
     r=subprocess.run(['taskset','-c',str(cpu),str(out/f'{label}-process_replay')],env=local,capture_output=True,text=True,check=True)
     data=[json.loads(line) for line in r.stdout.splitlines()]
     assert len(data)==15
     for row in data:
      row.update(campaign=campaign,block=block,slot=slot,variant=label,scope=scope)
      log.write(json.dumps(row)+'\n')
     log.flush()
with (out/'allocations.jsonl').open('w') as log:
 for label in ['baseline','candidate']:
  for scope in ['short','long']:
   local=env.copy();local.pop('PENELOPE_REPLAY_LONG_SCOPE',None)
   if scope=='long':local['PENELOPE_REPLAY_LONG_SCOPE']='1'
   r=subprocess.run(['taskset','-c',str(cpu),str(out/f'{label}-process_replay_allocations')],env=local,capture_output=True,text=True,check=True)
   data=[json.loads(line) for line in r.stdout.splitlines()];assert len(data)==80
   for row in data:
    row.update(variant=label,scope=scope);log.write(json.dumps(row)+'\n')
(out/'measurement.json').write_text(json.dumps({'cpu':cpu,'platform':platform.platform(),'started_unix':start,'ended_unix':time.time(),'timing_processes':96,'profiles_per_process':15,'timed_calls_per_profile':1000,'warmups_per_profile':100,'filesystem':subprocess.check_output(['stat','-f','-c','%T',env['TMPDIR']],text=True).strip()},indent=2)+'\n')
