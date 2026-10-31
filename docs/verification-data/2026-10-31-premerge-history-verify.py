"""Verify message-only rewriting against retained historical commit objects."""
from pathlib import Path
import json,re,subprocess
root=Path(__file__).resolve().parents[2]
m=json.loads((Path(__file__).parent/'2026-10-31-premerge-history-map.json').read_text())
def git(*args):return subprocess.check_output(['git',*args],cwd=root)
mapping={r['old']:r['new'] for r in m['commits']}
assert len(mapping)==m['commit_count']==271
for row in m['commits']:
 old_head,old_message=git('cat-file','commit',row['old']).split(b'\n\n',1)
 new_head,new_message=git('cat-file','commit',row['new']).split(b'\n\n',1)
 expected=[]
 for line in old_head.split(b'\n'):
  if line.startswith(b'parent '):
   parent=line.split(b' ',1)[1].decode();line=b'parent '+mapping.get(parent,parent).encode()
  expected.append(line)
 assert b'\n'.join(expected)==new_head,row['old']
 assert old_message.partition(b'\n')[1:]==new_message.partition(b'\n')[1:],row['old']
 assert old_message.partition(b'\n')[0].decode()==row['old_subject']
 assert new_message.partition(b'\n')[0].decode()==row['new_subject']
 assert re.fullmatch(rb'[a-z]+\([a-z0-9-]+\): .+',new_message.partition(b'\n')[0]),row['new']
 assert git('rev-parse',row['old']+'^{tree}').decode().strip()==row['tree']==git('rev-parse',row['new']+'^{tree}').decode().strip()
assert mapping[m['old_tip']]==m['new_tip']
assert len(git('rev-list',m['base']+'..'+m['new_tip']).splitlines())==m['commit_count']
assert not git('diff-tree','--no-commit-id','-r',m['old_tip'],m['new_tip'])
print('Verified 271 rewritten commits: conventional scoped subjects; identical trees, metadata, message bodies, and mapped parents')
