"""Verify both payload campaigns, source pins, allocation deltas and regressions."""
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
first_build=json.loads(read('payload-borrow-build.json'))
final_build=json.loads(read('payload-borrow-final-build.json'))
assert first_build['fixture_sha256']==final_build['fixture_sha256']
assert first_build['compiler']==final_build['compiler']
assert first_build['variants']['baseline']==final_build['variants']['baseline']
assert first_build['variants']['borrowed']==final_build['variants']['nested']
for path,digest in final_build['fixture_sha256'].items():assert meta['source_sha256'][path]==digest
for prefix,build in [('payload-borrow',first_build),('payload-borrow-final',final_build)]:
    for variant,item in build['variants'].items():
        source=gzip.decompress((root/f'{prefix}-source-{variant}.rs.gz').read_bytes())
        assert hashlib.sha256(source).hexdigest()==item['source_sha256']
        pin=meta['baseline'] if variant=='baseline' else (meta['candidate'] if prefix.endswith('final') and variant=='borrowed' else None)
        if pin:assert source==subprocess.check_output(['git','show',pin+':crates/penelope-sqlite/src/payloads.rs'])
checked=0;total_rows=0;total_samples=0
for final in (False,True):
    prefix='payload-borrow-final' if final else 'payload-borrow'
    build=final_build if final else first_build
    run_meta=json.loads(read(prefix+'-metadata.json'))
    rows=json.loads(read(prefix+'-processes.json.gz'));assert len(rows)==(144 if final else 96)
    total_rows+=len(rows)
    values={};alloc={}
    for pair in range(24):
        order=run_meta['order'][pair] if final else (['baseline','borrowed'] if pair%2==0 else ['borrowed','baseline'])
        selected=[row for row in rows if row['pair']==pair]
        assert [(row['variant'],row['kind']) for row in selected]==[(variant,kind) for variant in order for kind in ('latency','allocations')]
    for row in rows:
        variant=row['variant'];pair=row['pair'];kind=row['kind']
        bench='process_payloads' if kind=='latency' else 'process_payload_allocations'
        assert row['binary_sha256']==build['variants'][variant]['binaries'][bench]['sha256']
        assert row['cpu']==run_meta['cpu'] and not row['stderr']
        output=list(map(json.loads,row['stdout'].splitlines()));assert len(output)==(20 if kind=='latency' else 320)
        for data in output:
            size=data['size'];case=data['case'];assert size in (0,32,4096,1048576)
            assert case in ('duplicate','read','missing','corrupt_read','corrupt_put')
            removed=size if case=='duplicate' else (max(size,1) if case.startswith('corrupt_') else 0)
            metric=f'{size}/{case}'
            if kind=='latency':
                assert data['iterations']==(128 if size==1048576 else 1000)
                assert data['warmup']==(32 if size==1048576 else 100)
                assert data['expected_removed_bytes']==removed
                key=(metric,pair,variant);assert key not in values
                values[key]=data['elapsed_ns']/data['iterations']
            else:
                assert 0<=data['sample']<16 and data['removed_bytes']==removed
                key=(metric,pair,variant,data['sample']);assert key not in alloc
                alloc[key]=data
    summary=json.loads(read(prefix+'-summary.json'))
    comparisons=[('nested_vs_baseline/','baseline','nested'),('borrowed_vs_baseline/','baseline','borrowed'),('borrowed_vs_nested/','nested','borrowed')] if final else [('', 'baseline','borrowed')]
    assert len(summary['latency'])==20*len(comparisons)
    for label,baseline,candidate in comparisons:
        for metric in sorted({key[0] for key in values}):
            item=summary['latency'][label+metric]
            ratios=[values[metric,i,candidate]/values[metric,i,baseline] for i in range(24)]
            rng=random.Random(42)
            boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
            assert math.isclose(item['mean_paired_change_pct'],100*(statistics.mean(ratios)-1),abs_tol=1e-10)
            for actual,index in zip(item['bootstrap95_pct'],(250,9749)):
                assert math.isclose(actual,100*(boot[index]-1),abs_tol=1e-10)
            for variant in (baseline,candidate):
                assert math.isclose(item['mean_ns_per_call'][variant],statistics.mean(values[metric,i,variant] for i in range(24)),abs_tol=1e-9)
            checked+=1
    assert len(alloc)==(23040 if final else 15360);total_samples+=len(alloc)
    for metric in sorted({key[0] for key in values}):
        for pair in range(24):
            for sample in range(16):
                baseline=alloc[metric,pair,'baseline',sample]
                for variant in (('nested','borrowed') if final else ('borrowed',)):
                    candidate=alloc[metric,pair,variant,sample];removed=baseline['removed_bytes']
                    assert baseline['allocations']-candidate['allocations']==int(removed>0)
                    assert baseline['requested_bytes']-candidate['requested_bytes']==removed
                    assert baseline['reallocations']==candidate['reallocations']
        for variant in build['variants']:
            for component,expected in summary['allocations'][metric][variant].items():
                assert sorted({item[component] for (m,p,v,s),item in alloc.items() if m==metric and v==variant})==expected
for name in ('payload-borrow-baseline-regression.log.gz','payload-borrow-regression.log.gz','payload-borrow-final-regression.log.gz'):
    output=read(name);cases=re.findall(r'case=(\S+) valid=(true|false) read_ok=(true|false) retry_ok=(true|false)',output)
    assert len(cases)==8 and all(valid==read==retry for _,valid,read,retry in cases)
    assert output.count('boundary_size=')==2 and '2 passed; 0 failed' in output
runs=json.loads(read('payload-borrow-final-repeats.json'));assert len(runs)==20
for run in runs:
    output=read(f"payload-borrow-final-repeat-{run['compiler']}-{run['run']}.log.gz")
    assert run['returncode']==0 and '2 passed; 0 failed' in output
    assert output.count('case=')==8 and output.count('boundary_size=')==2
for name in ('payload-borrow-final-workspace.log.gz','payload-borrow-final-msrv.log.gz'):
    output=read(name)
    assert '24 passed; 0 failed' in output and '51 passed; 0 failed' in output and '26 passed; 0 failed' in output
    assert 'test result: FAILED' not in output
assert 'Finished' in read('payload-borrow-final-clippy.log.gz') and 'error:' not in read('payload-borrow-final-clippy.log.gz')
checks=json.loads(read('payload-borrow-final-checks.json'));assert len(checks)==3 and all(item['returncode']==0 for item in checks)
print(f'Verified source pins, {total_rows} processes, {checked} latency metrics, {total_samples} allocation samples and 200 regression fixtures.')
