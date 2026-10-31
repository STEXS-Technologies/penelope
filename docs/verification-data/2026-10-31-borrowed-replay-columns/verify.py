"""Verify pinned source, 96 public-port processes, allocation deltas and statistics."""
from pathlib import Path
import gzip
import hashlib
import json
import math
import random
import re
import statistics
import subprocess
root=Path(__file__).resolve().parent

def read(name):
    data=(root/name).read_bytes()
    return gzip.decompress(data).decode() if name.endswith('.gz') else data.decode()
meta=json.loads(read('metadata.json'))
for path,digest in meta['source_sha256'].items():
    assert hashlib.sha256(subprocess.check_output(['git','show',meta['candidate']+':'+path])).hexdigest()==digest,path
build=json.loads(read('replay-borrow-build.json'))
for path,digest in build['fixture_sha256'].items():
    assert meta['source_sha256'][path]==digest,path
for variant,item in build['variants'].items():
    for path,digest in item['source_sha256'].items():
        source=gzip.decompress((root/f'replay-borrow-source-{variant}-{Path(path).name}.gz').read_bytes())
        pin=meta['baseline'] if variant=='baseline' else meta['candidate']
        assert source==subprocess.check_output(['git','show',pin+':'+path])
        assert hashlib.sha256(source).hexdigest()==digest
rows=json.loads(read('replay-borrow-processes.json.gz'));assert len(rows)==96
run_meta=json.loads(read('replay-borrow-metadata.json'))
values={};alloc={}
for pair in range(24):
    selected=[row for row in rows if row['pair']==pair]
    order=['baseline','borrowed'] if pair%2==0 else ['borrowed','baseline']
    assert [(row['variant'],row['kind']) for row in selected]==[(variant,kind) for variant in order for kind in ('latency','allocations')]
for row in rows:
    variant=row['variant'];pair=row['pair'];kind=row['kind']
    bench='process_replay' if kind=='latency' else 'process_replay_allocations'
    assert row['binary_sha256']==build['variants'][variant]['binaries'][bench]['sha256']
    assert row['cpu']==run_meta['cpu'] and not row['stderr']
    output=list(map(json.loads,row['stdout'].splitlines()))
    assert len(output)==(15 if kind=='latency' else 80)
    for data in output:
        profile=data['profile']
        if kind=='latency':
            count=data['history'];assert count in (1,128,4096)
            expected={'first':(0,1),'last':(count-1,1),'end':(count,1),'past':(count+17,1),'page':(0,128)}[profile]
            assert (data['from'],data['limit'])==expected and data['iterations']==1000
            key=(f'{count}/{profile}',pair,variant);assert key not in values
            values[key]=data['elapsed_ns']/1000
        else:
            key=(profile,pair,variant,data['sample']);assert key not in alloc
            assert 0<=data['sample']<16
            expected={'first':(0,1,1),'last':(127,1,1),'end':(128,1,0),'past':(145,1,0),'page':(0,128,128)}[profile]
            assert (data['from'],data['limit'],data['rows'])==expected
            alloc[key]=data
summary=json.loads(read('replay-borrow-summary.json'));assert len(summary['latency'])==15
for metric,item in summary['latency'].items():
    ratios=[values[metric,i,'borrowed']/values[metric,i,'baseline'] for i in range(24)]
    rng=random.Random(42)
    boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
    assert math.isclose(item['mean_paired_change_pct'],100*(statistics.mean(ratios)-1),abs_tol=1e-10)
    for actual,index in zip(item['bootstrap95_pct'],(250,9749)):
        assert math.isclose(actual,100*(boot[index]-1),abs_tol=1e-10)
    for variant in ('baseline','borrowed'):
        assert math.isclose(item['mean_ns_per_call'][variant],statistics.mean(values[metric,i,variant] for i in range(24)),abs_tol=1e-9)
assert len(alloc)==3840
for profile in ('first','last','end','past','page'):
    for pair in range(24):
        for sample in range(16):
            baseline=alloc[profile,pair,'baseline',sample];borrowed=alloc[profile,pair,'borrowed',sample]
            assert baseline['allocations']-borrowed['allocations']==2*baseline['rows']
            assert baseline['requested_bytes']-borrowed['requested_bytes']==baseline['copied_bytes']==borrowed['copied_bytes']
            assert baseline['reallocations']==borrowed['reallocations']
    for variant in ('baseline','borrowed'):
        for metric,expected in summary['allocations'][profile][variant].items():
            assert sorted({data[metric] for (p,i,v,s),data in alloc.items() if p==profile and v==variant})==expected
assert summary['allocations']['page']['baseline']['allocations']==[265]
assert summary['allocations']['page']['borrowed']['allocations']==[9]
for name in ('replay-borrow-baseline-columns.log.gz','replay-borrow-columns.log.gz'):
    cases=re.findall(r'case=(\S+) valid=(true|false) accepted=(true|false)',read(name))
    assert len(cases)==20 and sum(valid=='true' for _,valid,_ in cases)==2
    assert all(valid==accepted for _,valid,accepted in cases)
runs=json.loads(read('replay-borrow-repeats.json'));assert len(runs)==20
for run in runs:
    assert run['returncode']==0
    output=read(f"replay-borrow-repeat-{run['compiler']}-{run['run']}.log.gz")
    assert output.count('case=')==20 and '1 passed; 0 failed' in output
for name in ('replay-borrow-workspace.log.gz','replay-borrow-msrv.log.gz'):
    output=read(name)
    assert '45 passed; 0 failed' in output and 'test result: FAILED' not in output
assert 'error:' not in read('replay-borrow-clippy.log.gz') and 'Finished' in read('replay-borrow-clippy.log.gz')
checks=json.loads(read('replay-borrow-checks.json'));assert len(checks)==3 and all(item['returncode']==0 for item in checks)
print('Verified pinned sources, 96 processes, 15 latency metrics, 3840 allocation samples and 400 regression cases.')
