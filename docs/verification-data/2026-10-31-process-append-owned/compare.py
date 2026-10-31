from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import sys
root = Path.cwd()
out = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
out.mkdir(parents=True,exist_ok=True)
if (out/'metadata.json').exists() or (out/'allocations.jsonl').exists():
    raise FileExistsError('use a fresh output directory')
parent = '5a04a54f1141c351b9a5a3b9c2d9ce30200a2699'
name = 'crates/penelope-sqlite/src/process.rs'
source = root/name
original = source.read_bytes()
env = dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',RUST_TEST_THREADS='1',TMPDIR=str(root/'target/qualification-temp'))
harness = ['crates/penelope-sqlite/benches/process_append_allocations.rs','crates/penelope-sqlite/benches/support/caller_allocator.rs','crates/penelope-sqlite/benches/support/process_append_fixture.rs']
metadata={'source_parent':parent,'sources':{},'harness_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in harness},'binaries':{},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
    for label in ['baseline','candidate']:
        source.write_bytes(subprocess.check_output(['git','show',parent+':'+name]) if label=='baseline' else original)
        metadata['sources'][label]={name:hashlib.sha256(source.read_bytes()).hexdigest()}
        with (out/(label+'-tests.txt')).open('w') as log:
            subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','append_','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        if label=='baseline':
            with (out/'negative-control.txt').open('w') as log:
                result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process_append_allocations','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT)
            assert result.returncode!=0
            assert 'scope rejection cloned strings' in (out/'negative-control.txt').read_text()
        with (out/(label+'-build.txt')).open('w') as log:
            result=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','process_append_allocations','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
        artifacts=[json.loads(line) for line in result.stdout.splitlines()]
        binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')=='process_append_allocations')
        destination=out/(label+'-probe');shutil.copy2(binary,destination)
        metadata['binaries'][destination.name]=hashlib.sha256(destination.read_bytes()).hexdigest()
finally:
    source.write_bytes(original)
assert source.read_bytes()==original
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
with (out/'allocations.jsonl').open('w') as log:
    for repeat in range(2):
        for label in ['baseline','candidate']:
            result=subprocess.run([str(out/(label+'-probe'))],env=env,capture_output=True,text=True,check=True)
            rows=[json.loads(line) for line in result.stdout.splitlines()]
            assert len(rows)==64
            for row in rows:
                row.update(variant=label,repeat=repeat)
                log.write(json.dumps(row)+'\n')
print('Restored runtime; both variants passed behavior tests and produced 256 caller-thread allocation observations.')
