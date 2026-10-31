from pathlib import Path
import collections, json, re
root = Path(__file__).resolve().parent
for toolchain in ['stable', 'msrv']:
    text = (root / f'{toolchain}-repeated.txt').read_text()
    assert len(re.findall(r'^REPEAT \d+$', text, re.M)) == 20
    assert text.count('test result: ok. 1 passed; 0 failed') == 20
    rows = re.findall(r'case=(\d+) round=(\d+) model=(\S+) stopped_identity_preserved=true retirement_tombstone_durable=true', text)
    assert len(rows) == 320
    counts = collections.Counter((case, round_) for case, round_, model in rows)
    assert counts == collections.Counter({(str(case), str(round_)): 20 for case in range(2) for round_ in range(8)})
    assert all(model in ['retire-first', 'stop-first'] for _, _, model in rows)
    assert '45 passed; 0 failed' in (root / f'{toolchain}.txt').read_text()
    print(toolchain, dict(collections.Counter(model for _, _, model in rows)))
assert '45 passed; 0 failed' in (root / 'restored.txt').read_text()
mutation = json.loads((root / 'mutation.json').read_text())
assert mutation['actual_exit'] == mutation['expected_exit'] == 101 and mutation['restored_equal_head']
mutant = (root / 'mutant.txt').read_text()
assert 'left: Ok(TaskRetirement' in mutant and 'right: Err(Conflict)' in mutant
assert '0 passed; 1 failed' in mutant
assert 'Finished' in (root / 'clippy.txt').read_text()
print('Verified 640 race histories / 1,280 contending requests, both toolchain suites, and mutation rejection.')
