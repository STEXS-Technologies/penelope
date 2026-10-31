"""Verify source pins, raw process outputs and all 60 paired statistics."""
from pathlib import Path
import gzip
import hashlib
import json
import math
import random
import statistics
import subprocess
root = Path(__file__).resolve().parent

def read(name):
    path = root / name
    data = path.read_bytes()
    return gzip.decompress(data).decode() if name.endswith('.gz') else data.decode()

meta=json.loads(read('metadata.json'))
for path,expected in meta['source_sha256'].items():
    source=subprocess.check_output(['git','show',meta['candidate']+':'+path])
    assert hashlib.sha256(source).hexdigest()==expected,path
build=json.loads(read('replay-cache-build.json'))
fixture=subprocess.check_output(['git','show',meta['candidate']+':crates/penelope-sqlite/benches/process_replay.rs'])
assert hashlib.sha256(fixture).hexdigest()==build['fixture_sha256']
for variant,item in build['variants'].items():
    source=gzip.decompress((root/('replay-cache-source-'+variant+'.rs.gz')).read_bytes())
    assert hashlib.sha256(source).hexdigest()==item['source_sha256']
    pin=item['pin'] or meta['candidate']
    assert source==subprocess.check_output(['git','show',pin+':crates/penelope-sqlite/src/process.rs'])
main_meta=json.loads(read('replay-cache-metadata.json'))
checked=0
for confirmation in (False,True):
    prefix='replay-cache-confirmation' if confirmation else 'replay-cache'
    rows=json.loads(read(prefix+'-processes.json.gz'))
    assert len(rows)==(48 if confirmation else 72)
    values={}
    for block in range(24):
        selected=[row for row in rows if row.get('pair',row.get('block'))==block]
        order=(['frontier','cached'] if block%2==0 else ['cached','frontier']) if confirmation else main_meta['order'][block]
        assert [row['variant'] for row in selected]==order
    for row in rows:
        variant=row['variant'];block=row.get('pair',row.get('block'))
        assert row['cpu']==main_meta['cpu']
        assert row['binary_sha256']==build['variants'][variant]['binary_sha256']
        assert not row['stderr']
        output=list(map(json.loads,row['stdout'].splitlines()))
        assert len(output)==15
        for data in output:
            count=data['history'];profile=data['profile'];assert count in (1,128,4096)
            expected={'first':(0,1),'last':(count-1,1),'end':(count,1),'past':(count+17,1),'page':(0,128)}[profile]
            assert (data['from'],data['limit'])==expected and data['iterations']==1000
            key=(f'{count}/{profile}',block,variant);assert key not in values
            values[key]=data['elapsed_ns']/1000
    saved=json.loads(read(prefix+'-summary.json'))
    comparisons=[('', 'frontier','cached')] if confirmation else [('frontier_cost/','pre_frontier','frontier'),('cache_gain/','frontier','cached'),('net/','pre_frontier','cached')]
    assert len(saved)==15*len(comparisons)
    for label,baseline,candidate in comparisons:
        for metric in sorted({key[0] for key in values}):
            ratios=[values[metric,i,candidate]/values[metric,i,baseline] for i in range(24)]
            rng=random.Random(42)
            boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
            item=saved[label+metric]
            assert math.isclose(item['mean_paired_change_pct'],100*(statistics.mean(ratios)-1),abs_tol=1e-10)
            for actual,index in zip(item['bootstrap95_pct'],(250,9749)):
                assert math.isclose(actual,100*(boot[index]-1),abs_tol=1e-10)
            for variant in (baseline,candidate):
                assert math.isclose(item['mean_ns_per_call'][variant],statistics.mean(values[metric,i,variant] for i in range(24)),abs_tol=1e-9)
            checked+=1
runs=json.loads(read('replay-cache-repeats.json'));assert len(runs)==20
for run in runs:
    assert run['returncode']==0
    assert '1 passed; 0 failed' in read(f"replay-cache-repeat-{run['compiler']}-{run['run']}.log.gz")
for name in ('replay-cache-workspace.log.gz','replay-cache-msrv.log.gz'):
    output=read(name)
    assert '44 passed; 0 failed' in output and 'test result: FAILED' not in output
assert 'error:' not in read('replay-cache-clippy.log.gz')
assert 'Finished' in read('replay-cache-clippy.log.gz')
print(f'Verified pinned sources, 120 measurement processes, {checked} statistics, and 20 warm-cache regressions.')
