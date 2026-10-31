from pathlib import Path
import os,subprocess,json,hashlib
root=Path.cwd();out=Path(__file__).resolve().parent;cpu=min(os.sched_getaffinity(0))
env=dict(os.environ,TMPDIR=str(root/'target/qualification-temp'));env.pop('PENELOPE_BENCH_EXTERNAL_COMMIT',None)
metadata={'cpu':cpu,'campaigns':2,'blocks_per_campaign':12,'processes_per_block':4,'setup_commits':2,'sizes':[0,32,4096,1048576],'iterations':128,'external_commit':False,'storage':'project qualification-temp on btrfs','binary_sha256':{label:hashlib.sha256((out/f'{label}-task_blob_commits').read_bytes()).hexdigest() for label in ['baseline','candidate']}}
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
for campaign in range(2):
 with (out/f'timing-{campaign}.jsonl').open('w') as log:
  for block in range(12):
   order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
   for position,label in enumerate(order):
    result=subprocess.run(['taskset','-c',str(cpu),str(out/f'{label}-task_blob_commits')],env=env,stdout=subprocess.PIPE,text=True,check=True)
    rows=[json.loads(line) for line in result.stdout.splitlines()]
    assert len(rows)==4 and {r['size'] for r in rows}=={0,32,4096,1048576}
    for row in rows:
     assert row['iterations']==128 and row['external_commit'] is False and row['elapsed_ns']>0
     log.write(json.dumps(dict(row,campaign=campaign,block=block,position=position,revision=label))+'\n')
    log.flush()
   print('campaign',campaign,'block',block,'completed',flush=True)
