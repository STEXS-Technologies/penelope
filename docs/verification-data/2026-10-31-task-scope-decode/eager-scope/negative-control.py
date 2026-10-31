import pathlib,subprocess,os
src=pathlib.Path('crates/penelope-sqlite/src/task_decode.rs');original=src.read_bytes();old=b'rest.strip_prefix(scope_json)';new=b'rest.get(scope_json.len()..)' ;assert old in original
try:
 src.write_bytes(original.replace(old,new))
 env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(pathlib.Path.cwd()/'target/qualification-temp'))
 with open('target/task-scope-decode/negative-control.log','w') as log:
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--lib','task_decode::tests::replay_scope_decoder_matches_core_json_forms_and_mutated_records','--','--nocapture'],stdout=log,stderr=subprocess.STDOUT,env=env)
 assert result.returncode==101
 log=pathlib.Path('target/task-scope-decode/negative-control.log').read_text();assert 'left: Ok(' in log and 'right: Err(Corrupt)' in log and 'test result: FAILED' in log
 print('Skipping scope bytes without comparison fails differential validation as expected.')
finally:src.write_bytes(original)
