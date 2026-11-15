import argparse,hashlib,json,os,pathlib,shutil,subprocess
parser=argparse.ArgumentParser()
parser.add_argument('--before',type=pathlib.Path,required=True)
parser.add_argument('--after',type=pathlib.Path,default=pathlib.Path.cwd())
parser.add_argument('--out',type=pathlib.Path,required=True)
args=parser.parse_args()
out=args.out.resolve();out.mkdir(parents=True,exist_ok=True)
roots={'before':args.before.resolve(),'after':args.after.resolve()}
manifest={}
for label,root in roots.items():
 env=os.environ.copy()
 if label=='before':env['CARGO_TARGET_DIR']=str(out/'baseline-target')
 command=['cargo','bench','-p','penelope-timeline','--bench','timeline_operations','--locked','--no-run','--message-format=json']
 result=subprocess.run(command,cwd=root,env=env,text=True,capture_output=True,check=True)
 (out/(label+'-build.log')).write_text(result.stderr)
 paths=[json.loads(line)['executable'] for line in result.stdout.splitlines() if json.loads(line).get('executable')]
 assert len(paths)==1, paths
 binary=out/(label+'-operations');shutil.copyfile(paths[0],binary);binary.chmod(0o755)
 manifest[label]={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_parent':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()}
(out/'binaries.json').write_text(json.dumps(manifest,indent=2)+'\n')
for round_index in range(5):
 for label in (['before','after'] if round_index%2==0 else ['after','before']):
  result=subprocess.run([str(out/(label+'-operations'))],text=True,capture_output=True,check=True)
  (out/f'{label}-{round_index}.log').write_text(result.stdout+result.stderr)
print('Alternating benchmark runs complete:',out,flush=True)
