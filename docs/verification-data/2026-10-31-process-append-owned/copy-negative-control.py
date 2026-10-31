from pathlib import Path
import os,subprocess,sys
root=Path.cwd()
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
out.mkdir(parents=True,exist_ok=True)
if (out/'copy-negative-control.txt').exists():raise FileExistsError('use a fresh output directory')
p=root/'crates/penelope-sqlite/src/process.rs';original=p.read_bytes()
start=original.index(b'        // This freshly validated request is already owned.')
end=original.index(b'    async fn read_outcomes(',start)
replacement=b'        ProcessStore::commit(self, &commit).await.map(|_receipt| ())\n    }\n'
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',RUST_TEST_THREADS='1',TMPDIR=str(root/'target/qualification-temp'))
try:
 p.write_bytes(original[:start]+replacement+original[end:])
 with (out/'copy-negative-control.txt').open('w') as log:
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process_append_allocations','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT)
 assert result.returncode!=0
 assert 'append admission copied more than one batch' in (out/'copy-negative-control.txt').read_text()
finally:p.write_bytes(original)
assert p.read_bytes()==original
print('Second-copy negative control rejected; runtime restored.')
