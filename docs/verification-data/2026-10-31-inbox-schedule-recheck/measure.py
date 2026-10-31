"""Repeat the previously affected configuration; no builds or source mutation."""
from pathlib import Path
import argparse,hashlib,json,os,platform,subprocess,time
parser=argparse.ArgumentParser();parser.add_argument('--binaries',type=Path,default=Path('target/inbox-retry-fastpath'));parser.add_argument('--output',type=Path,default=Path('target/inbox-schedule-recheck'));args=parser.parse_args()
source=args.binaries.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True);database_root=out/'databases';database_root.mkdir(exist_ok=True)
assert not (out/'raw.jsonl').exists() and not (out/'measurement.json').exists()
metadata=json.loads((source/'metadata.json').read_text());binary_hashes={}
for variant in ['baseline','candidate']:
 name=variant+'-durable_timer_workload';digest=hashlib.sha256((source/name).read_bytes()).hexdigest();assert digest==metadata['binaries'][name];binary_hashes[name]=digest
cpu=min(os.sched_getaffinity(0));env=dict(os.environ,TMPDIR=str(database_root));start=time.time()
with (out/'raw.jsonl').open('w') as log:
 for campaign in range(2):
  for block in range(12):
   order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
   for slot,variant in enumerate(order):
    command=['taskset','-c',str(cpu),str(source/(variant+'-durable_timer_workload')),'--batch','16','--width','9','--work','65536','--rounds','2','--reopen','false']
    rows=[json.loads(line) for line in subprocess.check_output(command,env=env,text=True).splitlines()];assert len(rows)==2
    for row in rows:log.write(json.dumps(row|dict(campaign=campaign,block=block,slot=slot,variant=variant))+'\n')
   log.flush();print(f'campaign={campaign} block={block} complete',flush=True)
(out/'measurement.json').write_text(json.dumps(dict(start=start,end=time.time(),cpu=cpu,platform=platform.platform(),database_root=str(database_root),filesystem=subprocess.check_output(['findmnt','-T',str(database_root),'-n','-o','FSTYPE,SOURCE'],text=True).strip(),ordinary_processes=96,allocation_processes=0,binaries=binary_hashes),indent=2)+'\n')
