#!/usr/bin/env python3
"""Compare preaccepted and atomic inbox admission in checked durable workflows."""
import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
CASES = ['durable', 'durable_replay']
MODES = ['preaccepted', 'atomic']

def summarize(directory):
    metadata=json.loads((directory/'metadata.json').read_text())
    rows=[json.loads(line) for line in (directory/'samples.jsonl').read_text().splitlines()]
    expected={(c,b,case,p,r) for c in range(metadata['campaigns']) for b in range(metadata['blocks']) for case in CASES for p in range(4) for r in range(metadata['rounds'])}
    seen=set()
    for row in rows:
        key=(row['campaign'],row['block'],row['case'],row['position'],row['round'])
        if key not in expected or key in seen:raise ValueError(f'duplicate/unexpected sample: {key}')
        seen.add(key)
        order=['preaccepted','atomic','atomic','preaccepted'] if row['block']%2==0 else ['atomic','preaccepted','preaccepted','atomic']
        if (row['admission']!=order[row['position']] or row['work']!=metadata['work'] or row['workers']!=1
            or row['iterations_per_worker']!=metadata['iterations'] or row['workflows']!=metadata['iterations']
            or row['calls_per_workflow']!=8 or row['verified'] is not True or row['schema']!=1
            or row['direct_first']!=(row['round']%2==0) or row['direct_ns']<=0 or row['penelope_ns']<=0):raise ValueError('invalid workload sample')
    if seen!=expected:raise ValueError('incomplete campaign')
    if len({row['checksum'] for row in rows})!=1:raise ValueError('business outputs differ')
    summary=[]
    for campaign in range(metadata['campaigns']):
        for case in CASES:
            subset=[row for row in rows if row['campaign']==campaign and row['case']==case]
            ratios=[]
            for block in range(metadata['blocks']):
                logs={mode:[math.log(row['penelope_ns']/row['workflows']) for row in subset if row['block']==block and row['admission']==mode] for mode in MODES}
                ratios.append(statistics.mean(logs['atomic'])-statistics.mean(logs['preaccepted']))
            rng=random.Random(f'admission-v1-{campaign}-{case}')
            bootstrap=sorted(statistics.mean(rng.choices(ratios,k=len(ratios))) for _ in range(20000))
            summary.append({'campaign':campaign,'case':case,'blocks':metadata['blocks'],
                'preaccepted_ns_per_workflow':math.exp(statistics.mean(math.log(row['penelope_ns']/row['workflows']) for row in subset if row['admission']=='preaccepted')),
                'atomic_ns_per_workflow':math.exp(statistics.mean(math.log(row['penelope_ns']/row['workflows']) for row in subset if row['admission']=='atomic')),
                'change_percent':100*math.expm1(statistics.mean(ratios)),
                'block_bootstrap_95_percent':[100*math.expm1(bootstrap[499]),100*math.expm1(bootstrap[19499])]})
    (directory/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    for row in summary:print(row)
    return summary

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'target'/('application-admission-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')))
    parser.add_argument('--summarize-only',type=Path)
    parser.add_argument('--campaigns',type=int,default=2)
    parser.add_argument('--blocks',type=int,default=6)
    parser.add_argument('--rounds',type=int,default=2)
    parser.add_argument('--iterations',type=int,default=4)
    parser.add_argument('--work',type=int,default=65536)
    args=parser.parse_args()
    if args.summarize_only:
        summarize(args.summarize_only.resolve());return
    if not (2<=args.campaigns<=8 and 2<=args.blocks<=32 and args.blocks%2==0 and 2<=args.rounds<=64 and args.rounds%2==0 and 1<=args.iterations<=1024 and 0<=args.work<=16777216):parser.error('invalid campaign dimensions')
    directory=args.output.resolve();directory.mkdir(parents=True,exist_ok=False);(directory/'databases').mkdir()
    env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(directory/'databases'))
    with (directory/'build.txt').open('w') as log:
        built=subprocess.run(['cargo','build','--release','--locked','-p','penelope-executor','--bench','application_workloads','--message-format=json'],cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
    binary=next(Path(item['executable']) for line in built.stdout.splitlines() if (item:=json.loads(line)).get('executable') and item.get('target',{}).get('name')=='application_workloads')
    cpus=sorted(os.sched_getaffinity(0)) if hasattr(os,'sched_getaffinity') else [0]
    sources=[Path(__file__),ROOT/'scripts/benchmark-application-workloads.py',ROOT/'scripts/application_workloads_summary.py',ROOT/'crates/penelope-executor/benches/application_workloads.rs',*sorted((ROOT/'crates/penelope-executor/benches/support/application_workloads').glob('*.rs')),ROOT/'Cargo.lock']
    metadata={'schema':1,'campaigns':args.campaigns,'blocks':args.blocks,'rounds':args.rounds,'iterations':args.iterations,'work':args.work,'workers':1,'cases':CASES,'cpu':cpus[0],'platform':platform.platform(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'git_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha256':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}}
    if platform.system()=='Linux':metadata['filesystem']=subprocess.check_output(['findmnt','-T',str(directory),'-n','-o','FSTYPE,SOURCE'],text=True).strip()
    (directory/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
    with (directory/'samples.jsonl').open('w') as log:
        for campaign in range(args.campaigns):
            for block in range(args.blocks):
                order=['preaccepted','atomic','atomic','preaccepted'] if block%2==0 else ['atomic','preaccepted','preaccepted','atomic']
                for case in CASES:
                    for position,mode in enumerate(order):
                        command=[str(binary),'--case',case,'--admission',mode,'--work',str(args.work),'--rounds',str(args.rounds),'--iterations',str(args.iterations),'--workers','1']
                        if platform.system()=='Linux':command=['taskset','-c',str(cpus[0]),*command]
                        result=subprocess.run(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,text=True,check=True)
                        rows=[json.loads(line) for line in result.stdout.splitlines()]
                        if len(rows)!=args.rounds:raise ValueError('incomplete binary output')
                        for row in rows:log.write(json.dumps(dict(row,campaign=campaign,block=block,position=position))+'\n')
                        log.flush()
                print(f'campaign={campaign} block={block} complete',flush=True)
    summarize(directory)

if __name__=='__main__':main()
