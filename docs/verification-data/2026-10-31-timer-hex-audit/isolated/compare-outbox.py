"""Inspect two saved probe executables; normalize only address relocations."""
from pathlib import Path
import hashlib,json,re,subprocess,sys
out=Path(sys.argv[3]);out.mkdir(exist_ok=True);bodies=[];metadata={}
for variant,binary in zip(['baseline','candidate'],sys.argv[1:3]):
 data=subprocess.check_output(['objdump','-d','--no-show-raw-insn','--demangle',binary],text=True)
 block=next(b for b in re.split(r'\n(?=[0-9a-f]+ <)',data) if re.match(r'[0-9a-f]+ <<lease_batch_services::fixture::Fixture>::invoke_outbox>:',b))
 start=int(block.split()[0],16);lines=[]
 for line in block.splitlines()[1:]:
  line=re.sub(r'^\s*([0-9a-f]+):',lambda m:f'{int(m[1],16)-start:04x}:',line)
  line=re.sub(r'\b[0-9a-f]+ <([^>]+)>',r'<\1>',line)
  line=re.sub(r'[+-]?0x[0-9a-f]+\(%rip\)','RELOC(%rip)',line)
  lines.append(line)
 body='\n'.join(lines)+'\n';bodies.append(body)
 (out/(variant+'-outbox-normalized.txt')).write_text(body)
 metadata[variant]={'address':hex(start),'binary_sha256':hashlib.sha256(Path(binary).read_bytes()).hexdigest(),'normalized_sha256':hashlib.sha256(body.encode()).hexdigest()}
metadata['normalized_equal']=bodies[0]==bodies[1]
(out/'outbox-disassembly.json').write_text(json.dumps(metadata,indent=2)+'\n')
assert metadata['normalized_equal']
