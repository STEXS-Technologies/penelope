from pathlib import Path
import hashlib,json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent;path=root/'crates/penelope-executor/src/timer_input.rs';original=path.read_bytes()
swap=original.replace(b'byte >> 4',b'__HIGH__').replace(b'byte & 15',b'byte >> 4').replace(b'__HIGH__',b'byte & 15')
mutants=[('nibble-order',swap),('uppercase',original.replace(b'0123456789abcdef',b'0123456789ABCDEF')),('byte-order',original.replace(b'digest.0.into_iter().zip(',b'digest.0.into_iter().rev().zip(')),('leading-zero',original.replace(b'&[high, low]',b'&[if high == b\'0\' { b\' \' } else { high }, low]')),('extra-allocation',original.replace(b'String::from_utf8(encoded.to_vec())',b'String::from_utf8(encoded.to_vec().clone())'))]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');results=[]
try:
 for name,source in mutants:
  assert source!=original;path.write_bytes(source)
  with (out/('negative-'+name+'.txt')).open('w') as log:r=subprocess.run(['cargo','test','-p','penelope-executor','--test','timer_input_hex'],env=env,stdout=log,stderr=subprocess.STDOUT)
  text=(out/('negative-'+name+'.txt')).read_text();marker='timer hex compatibility/allocation failure';assert r.returncode!=0 and marker in text,(name,text)
  results.append({'name':name,'rejected':True,'marker':marker})
finally:path.write_bytes(original)
assert path.read_bytes()==original
(out/'negative-results.json').write_text(json.dumps({'results':results,'restored_sha256':hashlib.sha256(original).hexdigest()},indent=2)+'\n')
