"""Audit the complete campaign log; partial mode never reports completion."""
import argparse
import hashlib
import json
from pathlib import Path
import re


def audit(text, targets, require_complete):
    rounds = []
    active = None
    phase = None
    success = False
    properties = {
        'metadata_state_machine', 'immutable_plan_admission',
        'correction_matches_closed_form', 'promotion_interleavings',
        'finalized_partial_handoff',
    }
    for line in text.splitlines():
        match = re.fullmatch(r'\[(\d+)/(\d+)\] (pure transition/restart/timer drill|public-boundary malformed-input drill)', line)
        if match:
            iteration, total = int(match[1]), int(match[2])
            assert total == 3
            if match[3].startswith('pure'):
                assert iteration == len(rounds) + 1
                assert active is None
                if rounds:
                    assert len(rounds[-1]['targets']) == len(targets)
                rounds.append({'iteration': iteration, 'property_results': [], 'targets': []})
                phase = 'properties'
            else:
                assert iteration == len(rounds) and phase == 'properties'
                assert set(rounds[-1]['property_results']) == properties
                phase = 'fuzz'
            continue
        match = re.fullmatch(r'test (\w+) \.\.\. ok', line)
        if match and phase == 'properties' and match[1] in properties:
            assert match[1] not in rounds[-1]['property_results']
            rounds[-1]['property_results'].append(match[1])
        assert not re.fullmatch(r'test .* \.\.\. FAILED', line)
        assert not line.startswith('test result: FAILED')
        match = re.fullmatch(r'(fuzz_\w+) malformed-input drill \((\d+) runs\)', line)
        if match:
            assert phase == 'fuzz' and active is None
            assert match[1] == targets[len(rounds[-1]['targets'])]
            assert int(match[2]) == 100000
            active = {'target': match[1], 'requested_runs': int(match[2]), 'seed': None, 'instrumented': False}
            continue
        match = re.fullmatch(r'INFO: Seed: (\d+)', line)
        if match:
            assert active is not None and active['seed'] is None
            assert int(match[1]) == rounds[-1]['iteration']
            active['seed'] = int(match[1])
        match = re.match(r'INFO: Loaded \d+ modules\s+\((\d+) inline 8-bit counters\)', line)
        if match:
            assert active is not None and int(match[1]) > 0
            active['instrumented'] = True
        match = re.fullmatch(r'Done (\d+) runs in (\d+) second\(s\)', line)
        if match:
            assert active is not None and active['seed'] is not None and active['instrumented']
            assert int(match[1]) == active['requested_runs']
            active.update(completed_runs=int(match[1]), elapsed_seconds=int(match[2]))
            rounds[-1]['targets'].append(active)
            active = None
        if line == 'pure chaos drill passed (3 repeated iterations)':
            assert len(rounds) == 3 and active is None
            assert all(len(item['targets']) == len(targets) for item in rounds)
            success = True
    if require_complete:
        assert success, 'campaign is incomplete; no completion claim permitted'
    return {'complete': success, 'expected_rounds': 3, 'expected_targets_per_round': len(targets),
            'completed_target_runs': sum(len(item['targets']) for item in rounds),
            'active_target': active, 'rounds': rounds,
            'terminal_exit_requirement': 'Caller must independently confirm runner exit 0.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--partial', action='store_true')
    args = parser.parse_args()
    script = Path('scripts/run_bounded_fuzz.sh').read_text()
    targets = re.search(r'fuzz_targets=\((.*?)\)', script, re.S)
    assert targets is not None
    names = re.findall(r'\bfuzz_\w+\b', targets[1])
    assert len(names) == len(set(names)) == 22
    raw = Path('target/timeline-foundation/qualification-chaos.log').read_bytes()
    result = audit(raw.decode(), names, not args.partial)
    result['log_sha256'] = hashlib.sha256(raw).hexdigest()
    result['runner_sha256'] = hashlib.sha256(Path('scripts/run_pure_chaos_drill.sh').read_bytes()).hexdigest()
    result['fuzz_runner_sha256'] = hashlib.sha256(script.encode()).hexdigest()
    Path('target/timeline-foundation/qualification-chaos-summary.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: value for key, value in result.items() if key != 'rounds'}))


if __name__ == '__main__':
    main()
