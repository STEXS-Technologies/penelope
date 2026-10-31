import argparse,hashlib,json,os,platform,random,re,statistics,subprocess,time
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--baseline',required=True)
parser.add_argument('--candidate',required=True)
parser.add_argument('--output',required=True)
parser.add_argument('--pairs',type=int,default=20)
args=parser.parse_args()
root=Path(args.output);root.mkdir(parents=True,exist_ok=True)
paths={'baseline':str(Path(args.baseline).resolve()),'candidate':str(Path(args.candidate).resolve())}
records=[]
def run(label, pair, warmup=False):
    command=['taskset','-c','0',paths[label]]
    began=time.monotonic_ns()
    proc=subprocess.run(command,capture_output=True,text=True,timeout=30)
    elapsed=time.monotonic_ns()-began
    assert proc.returncode==0,(command,proc.stderr,proc.stdout)
    values={}
    for line in proc.stdout.splitlines():
        match=re.fullmatch(r'sqlite pending alternate=(false|true): (\d+) operations in ([0-9.]+)(ns|µs|ms|s) \((\d+) ns/op\)',line)
        assert match,line
        alternate,count,duration,unit,_=match.groups()
        values[alternate]=float(duration)*{'ns':1,'µs':1000,'ms':1000000,'s':1000000000}[unit]/int(count)
    record=dict(label=label,pair=pair,warmup=warmup,command=command,elapsed_ns=elapsed,stdout=proc.stdout,stderr=proc.stderr,ns_per_poll=values)
    records.append(record)
    with (root/'processes.jsonl').open('a') as f:f.write(json.dumps(record)+'\n')
for label in paths:run(label,-1,True)
for pair in range(args.pairs):
    for label in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):run(label,pair)
    print('completed pair',pair+1,flush=True)
summary={}
rng=random.Random(3030)
for kind in ['false','true']:
    by_label={label:[r['ns_per_poll'][kind] for r in records if r['label']==label and not r['warmup']] for label in paths}
    ratios=[(c/b-1)*100 for b,c in zip(by_label['baseline'],by_label['candidate'])]
    boots=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(10000))
    summary[kind]={'baseline_median_ns':statistics.median(by_label['baseline']),
                   'candidate_median_ns':statistics.median(by_label['candidate']),
                   'median_to_median_percent':(statistics.median(by_label['candidate'])/statistics.median(by_label['baseline'])-1)*100,
                   'paired_median_percent':statistics.median(ratios),
                   'paired_bootstrap_95_percent':[boots[250],boots[9749]],
                   'all_paired_percent':ratios}
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
(root/'metadata.json').write_text(json.dumps(dict(pairs=args.pairs,warmups=1,cpu=0,platform=platform.platform(),rustc=subprocess.check_output(['rustc','-Vv'],text=True),binary_sha256={label:hashlib.sha256(Path(path).read_bytes()).hexdigest() for label,path in paths.items()},scope='Public OutboxStore::claim future while clock intentionally blocks worker. SQLite setup, release and completion excluded from per-poll times. Host workloads, CPU frequency and OS caches uncontrolled.'),indent=2)+'\n')
print(json.dumps(summary,indent=2))
