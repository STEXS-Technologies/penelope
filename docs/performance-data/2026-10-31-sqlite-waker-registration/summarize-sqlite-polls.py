import argparse,json,random,re,statistics
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('directory')
args=parser.parse_args()
root=Path(args.directory)
records=[json.loads(line) for line in (root/'processes.jsonl').read_text().splitlines()]
metadata=json.loads((root/'metadata.json').read_text())
for label in ['baseline','candidate']:
    selected=[r for r in records if r['label']==label]
    assert len([r for r in selected if r['warmup']])==1
    assert sorted(r['pair'] for r in selected if not r['warmup'])==list(range(metadata['pairs']))
for record in records:
    parsed={}
    for line in record['stdout'].splitlines():
        match=re.fullmatch(r'sqlite pending alternate=(false|true): (\d+) operations in ([0-9.]+)(ns|µs|ms|s) \((\d+) ns/op\)',line)
        assert match,line
        alternate,count,duration,unit,_=match.groups()
        parsed[alternate]=float(duration)*{'ns':1,'µs':1000,'ms':1000000,'s':1000000000}[unit]/int(count)
    assert parsed==record['ns_per_poll']
summary={}
rng=random.Random(3030)
for kind in ['false','true']:
    by_label={label:[r['ns_per_poll'][kind] for r in records if r['label']==label and not r['warmup']] for label in ['baseline','candidate']}
    ratios=[(c/b-1)*100 for b,c in zip(by_label['baseline'],by_label['candidate'])]
    boots=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(10000))
    summary[kind]={'baseline_median_ns':statistics.median(by_label['baseline']),
                   'candidate_median_ns':statistics.median(by_label['candidate']),
                   'median_to_median_percent':(statistics.median(by_label['candidate'])/statistics.median(by_label['baseline'])-1)*100,
                   'paired_median_percent':statistics.median(ratios),
                   'paired_bootstrap_95_percent':[boots[250],boots[9749]],
                   'all_paired_percent':ratios}
assert summary==json.loads((root/'summary.json').read_text())
print(json.dumps(summary,indent=2))
