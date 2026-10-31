"""Verify source pins, raw discovery/write metrics and crash-migration evidence."""
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
build=json.loads(read('inbox-index-build.json'))
assert build['fixture_sha256']==meta['source_sha256']['crates/penelope-sqlite/benches/pending_history.rs']
for variant,item in build['variants'].items():
    source=gzip.decompress((root/f'inbox-index-source-{variant}.rs.gz').read_bytes())
    pin=meta['baseline'] if variant=='baseline' else meta['candidate']
    assert source==subprocess.check_output(['git','show',pin+':crates/penelope-sqlite/src/schema.rs'])
    assert hashlib.sha256(source).hexdigest()==item['source_sha256']
fixture=subprocess.check_output(['git','show',meta['candidate']+':crates/penelope-sqlite/benches/pending_history.rs']).decode()
production=subprocess.check_output(['git','show',meta['candidate']+':crates/penelope-sqlite/src/inbox.rs']).decode()
query=re.search(r'const QUERY: &str = "([^"]+)"',fixture)[1]
assert query in production
run_meta=json.loads(read('inbox-index-metadata.json'));checked=0
for confirmation in (False,True):
    prefix='inbox-index-confirmation' if confirmation else 'inbox-index'
    rows=json.loads(read(prefix+'-processes.json.gz'));assert len(rows)==48
    values={}
    for pair in range(24):
        selected=[row for row in rows if row['pair']==pair]
        assert [row['variant'] for row in selected]==(['baseline','indexed'] if pair%2==0 else ['indexed','baseline'])
    for row in rows:
        variant=row['variant'];pair=row['pair']
        assert row['cpu']==run_meta['cpu'] and not row['stderr']
        assert row['binary_sha256']==build['variants'][variant]['binary_sha256']
        output=list(map(json.loads,row['stdout'].splitlines()));assert [data['retained'] for data in output]==[0,128,1024]
        for data in output:
            n=data['retained'];assert data['schema_version']==(5 if variant=='baseline' else 6)
            assert data['sqlite']=='3.53.2' and data['consumed_rows']==n+16
            assert data['read_calls']==1000 and data['pending_calls']==16
            assert data['pages']=={0:42,128:87,1024:387}[n]+(variant=='indexed')
            for kind in ('empty','pending'):
                engine=data[kind+'_engine']
                assert engine['sorts']==0
                expected=[] if kind=='empty' else [f'inp_pending_{index:03}' for index in range(16)]
                assert engine['identities']==expected
                instructions=11 if kind=='empty' else 137
                if variant=='baseline':instructions=({'empty':{0:11,128:650,1024:5130},'pending':{0:169,128:809,1024:5289}}[kind][n])
                assert engine['vm_steps']==instructions
                index='process_inbox_pending' if variant=='indexed' else 'sqlite_autoindex_process_inbox_1'
                assert engine['plan']==[f'SEARCH process_inbox USING INDEX {index} (process_key=?)']
            counts=[('open',1),('empty',1000),('pending',1000),('pending_accept',16),('pending_commit',16)]+([('seed_accept',n),('seed_commit',n)] if n else [])
            for metric,count in counts:
                key=(f'{n}/{metric}',pair,variant);assert key not in values
                values[key]=data[metric+'_ns']/count
    summary=json.loads(read(prefix+'-summary.json'));assert len(summary)==19
    for metric,item in summary.items():
        ratios=[values[metric,i,'indexed']/values[metric,i,'baseline'] for i in range(24)]
        rng=random.Random(42)
        boot=sorted(statistics.mean(rng.choices(ratios,k=24)) for _ in range(10000))
        assert math.isclose(item['mean_paired_change_pct'],100*(statistics.mean(ratios)-1),abs_tol=1e-10)
        for actual,index in zip(item['bootstrap95_pct'],(250,9749)):
            assert math.isclose(actual,100*(boot[index]-1),abs_tol=1e-10)
        for variant in ('baseline','indexed'):
            assert math.isclose(item['mean_ns_per_call'][variant],statistics.mean(values[metric,i,variant] for i in range(24)),abs_tol=1e-9)
        checked+=1
runs=json.loads(read('inbox-index-repeats.json'));assert len(runs)==30
assert sum(run['cases'] for run in runs if run['kind']=='port')==80
assert sum(run['cases'] for run in runs if run['kind']=='migration')==180
for run in runs:
    assert run['returncode']==0
    output=read(f"inbox-index-repeat-{run['compiler']}-{run['kind']}-{run['run']}.log.gz")
    assert ('4 passed; 0 failed' if run['kind']=='port' else '3 passed; 0 failed') in output
    if run['kind']=='migration':assert output.count('signal=9 rows_preserved=true reopen_version=6')==18
for name in ('inbox-index-workspace.log.gz','inbox-index-msrv.log.gz'):
    output=read(name)
    assert '24 passed; 0 failed' in output and '49 passed; 0 failed' in output and '26 passed; 0 failed' in output
    assert 'test result: FAILED' not in output
assert 'Finished' in read('inbox-index-clippy.log.gz') and 'error:' not in read('inbox-index-clippy.log.gz')
checks=json.loads(read('inbox-index-checks.json'));assert len(checks)==3 and all(item['returncode']==0 for item in checks)
print(f'Verified source pins, 96 processes, {checked} metrics, 80 port fixtures and 180 crash cuts.')
