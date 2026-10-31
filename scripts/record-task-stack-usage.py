#!/usr/bin/env python3
"""Record static function frames for an explicit no_std fixed-port profile.

This inventory is not a call-chain bound. Opaque callees and callback frames
must be accounted for separately before asserting whole-supervisor stack use.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
WRAPPER = '''#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import sys
arguments = sys.argv[1:]
name = arguments[arguments.index('--crate-name') + 1] if '--crate-name' in arguments else ''
if name in {'penelope_stack_fixture', 'penelope_core', 'sha2', 'digest', 'cpufeatures',
            'block_buffer', 'crypto_common', 'generic_array', 'typenum', 'cfg_if', 'thiserror'}:
    output = Path(os.environ['PENELOPE_STACK_USAGE_DIR'])
    key = name + '-' + hashlib.sha256(json.dumps(arguments).encode()).hexdigest()[:12]
    (output / (key + '.invocation.json')).write_text(json.dumps(arguments, indent=2) + '\\n')
    arguments.extend(['-C', 'codegen-units=1', '-C',
                      'llvm-args=-stack-usage-file=' + str(output / (key + '.su'))])
# Replace the wrapper so Cargo's inherited jobserver descriptors remain open.
os.execv(arguments[0], arguments)
'''


def fixture_source(text):
    anchor = '#[derive(Clone, Copy)]\nstruct StoredBlob'
    if text.count(anchor) != 1 or text.count('fn stack_positive_control()') != 1:
        raise ValueError('portable fixture markers changed')
    first = text.index(anchor)
    last = text.index('fn main()', first)
    return ('use penelope_core::task::{control::*, runtime::*, *};\n'
            'use core::hint::black_box;\n' + text[first:last] + '''
#[unsafe(no_mangle)]
pub fn qualify_task_path(selector: u8) {
    match selector {
        0 => lifecycle(EffectContract::Pure),
        1 => lifecycle(EffectContract::ReconcileRequired),
        2 => invalid_workspace(),
        3 => migration(),
        4 => independent_completion(),
        _ => stack_positive_control(),
    }
}
''')


def parse_usage(text):
    rows = []
    for line in text.splitlines():
        location, count, classification = line.split('\t')
        unit, symbol = location.split(':', 1)
        size = int(count)
        if not unit or not symbol or size < 0 or not classification:
            raise ValueError('invalid compiler stack usage entry')
        rows.append({'unit': unit, 'symbol': symbol, 'frame_bytes': size,
                     'classification': classification})
    return rows


def command(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        parser.error('this profile and wrapper require native Linux x86-64')
    settings = ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
                'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTFLAGS')
    if any(os.environ.get(key) for key in settings):
        parser.error('custom compiler settings would confound this explicit profile')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    usage = output / 'usage'
    usage.mkdir()
    original = ROOT / 'crates/penelope-core/tests/task_allocations.rs'
    source = fixture_source(original.read_text())
    (output / 'fixture.rs').write_text(source)
    wrapper = output / 'rustc-wrapper.py'
    wrapper.write_text(WRAPPER)
    wrapper.chmod(0o700)
    records = []
    with tempfile.TemporaryDirectory(prefix='task-stack-', dir=ROOT / 'target') as temporary:
        directory = Path(temporary)
        shutil.copyfile(output / 'fixture.rs', directory / 'fixture.rs')
        (directory / 'lib.rs').write_text('#![no_std]\n#[path="fixture.rs"]\nmod fixture;\npub use fixture::*;\n')
        manifest = ('[package]\nname="penelope-stack-fixture"\nversion="0.0.0"\n'
                    'edition="2024"\npublish=false\n[lib]\npath="lib.rs"\n'
                    '[dependencies]\npenelope-core={path=' + json.dumps(str(ROOT / 'crates/penelope-core'))
                    + ',default-features=false}\n[workspace]\n[profile.release]\n'
                    'codegen-units=1\npanic="abort"\n')
        (directory / 'Cargo.toml').write_text(manifest)
        shutil.copyfile(ROOT / 'Cargo.lock', directory / 'Cargo.lock')
        environment = {**os.environ, 'RUSTFLAGS': '-C no-redzone=yes',
            'RUSTC_WRAPPER': str(wrapper), 'PENELOPE_STACK_USAGE_DIR': str(usage),
            'CARGO_TARGET_DIR': str(directory / 'target'), 'CARGO_PROFILE_RELEASE_CODEGEN_UNITS': '1',
            'CARGO_PROFILE_RELEASE_PANIC': 'abort'}
        # Preserve locked dependency versions; only add the nonpublishable fixture.
        command('cargo', '+1.99.0', 'metadata', '--offline', '--format-version=1',
                '--manifest-path', str(directory / 'Cargo.toml'), cwd=ROOT, capture_output=True)
        locked = (directory / 'Cargo.lock').read_text()
        (output / 'Cargo.lock').write_text(locked)
        # Reject dependency substitutions rather than silently measuring new versions.
        import tomllib
        baseline = {(p['name'], p['version'], p.get('source')) for p in
                    tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']}
        packages = tomllib.loads(locked)['package']
        if any((p['name'], p['version'], p.get('source')) not in baseline
               for p in packages if p['name'] != 'penelope-stack-fixture'):
            raise ValueError('fixture changed a pinned dependency version')
        build = command('cargo', '+1.99.0', 'build', '--release', '--offline', '--locked',
            '--manifest-path', str(directory / 'Cargo.toml'), '--message-format=json',
            cwd=ROOT, env=environment, capture_output=True)
        (output / 'build.jsonl').write_text(build.stdout)
        (output / 'build.stderr').write_text(build.stderr)
        artifacts = [filename for line in build.stdout.splitlines()
                     if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
                     and entry['target']['name'] == 'penelope_stack_fixture'
                     for filename in entry['filenames'] if filename.endswith('.rlib')]
        if len(artifacts) != 1:
            raise ValueError('expected one analysis fixture library')
        shutil.copyfile(artifacts[0], output / 'fixture.rlib')
        for path in sorted(usage.glob('*.su')):
            records.extend({**row, 'usage_file': path.name} for row in parse_usage(path.read_text()))
    if not records:
        raise ValueError('compiler emitted no function frames')
    decoded = command('c++filt', '--format=rust', input='\n'.join(row['symbol'] for row in records) + '\n',
                      capture_output=True).stdout.splitlines()
    if len(decoded) != len(records):
        raise ValueError('demangler changed record count')
    for row, name in zip(records, decoded):
        row['name'] = name
    calibration = [row for row in records if '::stack_positive_control' in row['name']]
    if len(calibration) != 1 or calibration[0]['classification'] != 'static' or calibration[0]['frame_bytes'] < 65536:
        raise ValueError('missing 64 KiB static-frame calibration')
    (output / 'frames.json').write_text(json.dumps(records, indent=2) + '\n')
    metadata = {'schema': 1, 'recorded_at': datetime.now(timezone.utc).isoformat(),
        'revision': command('git', 'rev-parse', 'HEAD', cwd=ROOT, capture_output=True).stdout.strip(),
        'tracked_diff': command('git', 'diff', 'HEAD', cwd=ROOT, capture_output=True).stdout,
        'rustc': command('rustc', '+1.99.0', '-vV', capture_output=True).stdout,
        'platform': platform.platform(), 'source_fixture_sha256': digest(original),
        'generated_fixture_sha256': digest(output / 'fixture.rs'), 'runner_sha256': digest(Path(__file__)),
        'fixture_lock_sha256': digest(output / 'Cargo.lock'),
        'fixture_rlib_sha256': digest(output / 'fixture.rlib'),
        'profile': 'native x86-64, release, one codegen unit, panic abort, no red zone, no_std library with fixed ports',
        'frame_records': len(records), 'positive_control_bytes': calibration[0]['frame_bytes'],
        'complete_call_chain_bound': False,
        'limitations': ['individual compiler-reported frames, not active call-chain sums',
            'precompiled core/compiler-builtins and opaque callbacks are not fully inventoried',
            'the generated no_std fixture differs from the hosted allocation-counting executable',
            'profile-specific; not a bound for arbitrary adapters, builds or hardware']}
    (output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps({'frame_records': len(records), 'positive_control_bytes': calibration[0]['frame_bytes'],
                      'complete_call_chain_bound': False}))


if __name__ == '__main__':
    main()
