#!/usr/bin/env python3
"""Build and statically bound one freestanding fixed-port RISC-V task profile."""
import argparse
from datetime import datetime, timezone
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGET = 'riscv32imafc-unknown-none-elf'
FLAGS = '-C no-redzone=yes -C jump-tables=no'


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


INVENTORY = load('inventory', 'record-task-stack-usage.py')
ANALYSIS = load('analysis', 'analyze-riscv-stack.py')


def run(*arguments, **kwargs):
    return subprocess.run(arguments, check=True, text=True, capture_output=True, **kwargs)


def image_sections(report):
    """Inventory allocated ELF sections without assuming a board memory map."""
    if len(report) != 1 or report[0]['FileSummary']['Format'] != 'elf32-littleriscv':
        raise ValueError('expected one little-endian RISC-V32 ELF image')
    sections = []
    for entry in report[0]['Sections']:
        row = entry['Section']
        flags = row['Flags']['Value']
        size = row['Size']
        if not isinstance(flags, int) or not isinstance(size, int) or flags < 0 or size < 0:
            raise ValueError('invalid ELF section size or flags')
        if flags & 2:  # SHF_ALLOC; symbol/debug tables are not resident sections.
            sections.append({'name': row['Name']['Name'], 'bytes': size,
                             'writable': bool(flags & 1),
                             'executable': bool(flags & 4)})
    if not sections or not any(row['executable'] for row in sections):
        raise ValueError('missing allocated executable sections')
    return {'sections': sections,
            'writable_section_bytes': sum(row['bytes'] for row in sections if row['writable']),
            'readonly_section_bytes': sum(row['bytes'] for row in sections if not row['writable']),
            'scope': 'allocated section byte sums; excludes loader/segment padding, stack and board memory-map placement'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    settings = ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
                'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTFLAGS')
    if any(os.environ.get(key) for key in settings):
        parser.error('custom compiler settings would confound the reference profile')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    usage = output / 'usage'
    usage.mkdir()
    original = ROOT / 'crates/penelope-core/tests/task_allocations.rs'
    entry = ROOT / 'scripts/fixtures/riscv_task_entry.rs'
    source = INVENTORY.fixture_source(original.read_text()) + entry.read_text()
    (output / 'fixture.rs').write_text(source)
    wrapper = output / 'rustc-wrapper.py'
    wrapper.write_text(INVENTORY.WRAPPER)
    wrapper.chmod(0o700)
    with tempfile.TemporaryDirectory(prefix='task-stack-bound-', dir=ROOT / 'target') as temporary:
        directory = Path(temporary)
        shutil.copyfile(output / 'fixture.rs', directory / 'fixture.rs')
        main_source = '#![no_std]\n#![no_main]\n#[path="fixture.rs"]\nmod fixture;\n'
        (directory / 'main.rs').write_text(main_source)
        (output / 'main.rs').write_text(main_source)
        manifest = ('[package]\nname="penelope-stack-fixture"\nversion="0.0.0"\n'
                    'edition="2024"\npublish=false\n[[bin]]\n'
                    'name="penelope-stack-fixture"\npath="main.rs"\n'
                    '[dependencies]\npenelope-core={path=' + json.dumps(str(ROOT / 'crates/penelope-core'))
                    + ',default-features=false}\n[workspace]\n[profile.release]\n'
                    'codegen-units=1\npanic="abort"\n')
        (directory / 'Cargo.toml').write_text(manifest)
        (output / 'Cargo.toml').write_text(manifest)
        shutil.copyfile(ROOT / 'Cargo.lock', directory / 'Cargo.lock')
        run('cargo', '+1.99.0', 'metadata', '--offline', '--format-version=1',
            '--manifest-path', str(directory / 'Cargo.toml'), cwd=ROOT)
        locked = (directory / 'Cargo.lock').read_text()
        baseline = {(p['name'], p['version'], p.get('source')) for p in
                    tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']}
        if any((p['name'], p['version'], p.get('source')) not in baseline
               for p in tomllib.loads(locked)['package'] if p['name'] != 'penelope-stack-fixture'):
            raise ValueError('fixture changed a pinned dependency version')
        (output / 'Cargo.lock').write_text(locked)
        environment = {**os.environ, 'RUSTFLAGS': FLAGS, 'RUSTC_WRAPPER': str(wrapper),
                       'PENELOPE_STACK_USAGE_DIR': str(usage),
                       'CARGO_TARGET_DIR': str(directory / 'build'),
                       'CARGO_PROFILE_RELEASE_CODEGEN_UNITS': '1',
                       'CARGO_PROFILE_RELEASE_PANIC': 'abort'}
        build = run('cargo', '+1.99.0', 'build', '--release', '--offline', '--locked',
                    '--target', TARGET, '--manifest-path', str(directory / 'Cargo.toml'),
                    cwd=ROOT, env=environment)
        (output / 'build.stdout').write_text(build.stdout)
        (output / 'build.stderr').write_text(build.stderr)
        if 'warning:' in build.stderr:
            raise ValueError('reference build emitted warnings')
        binary = directory / 'build' / TARGET / 'release/penelope-stack-fixture'
        shutil.copyfile(binary, output / 'fixture.elf')
    assembly = run('llvm-objdump', '-d', '--no-show-raw-insn', str(output / 'fixture.elf')).stdout
    (output / 'disassembly.txt').write_text(assembly)
    elf_report = run('llvm-readobj', '--elf-output-style=JSON', '--file-headers',
                     '--sections', str(output / 'fixture.elf')).stdout
    (output / 'elf-sections.json').write_text(elf_report)
    image = image_sections(json.loads(elf_report))
    (output / 'image-memory.json').write_text(json.dumps(image, indent=2) + '\n')
    records = []
    for path in sorted(usage.glob('*.su')):
        records.extend(INVENTORY.parse_usage(path.read_text()))
    if not records:
        raise ValueError('missing compiler frame inventory')
    (output / 'frames.json').write_text(json.dumps(records, indent=2) + '\n')
    bound = ANALYSIS.analyze(ANALYSIS.parse_disassembly(assembly), records)
    calibrations = [row for row in records if 'stack_positive_control' in row['symbol']]
    if len(calibrations) != 1 or calibrations[0]['frame_bytes'] < 65536:
        raise ValueError('missing 64 KiB positive stack control')
    bound['positive_control_bytes'] = calibrations[0]['frame_bytes']
    (output / 'bound.json').write_text(json.dumps(bound, indent=2) + '\n')
    metadata = {'schema': 1, 'recorded_at': datetime.now(timezone.utc).isoformat(),
        'revision': run('git', 'rev-parse', 'HEAD', cwd=ROOT).stdout.strip(),
        'tracked_diff': run('git', 'diff', 'HEAD', cwd=ROOT).stdout,
        'rustc': run('rustc', '+1.99.0', '-vV').stdout,
        'objdump': run('llvm-objdump', '--version').stdout,
        'readobj': run('llvm-readobj', '--version').stdout,
        'target': TARGET, 'flags': FLAGS,
        'profile': 'release, one codegen unit, panic abort, freestanding no_std binary, fixed ports',
        'hashes': {str(path.relative_to(ROOT)): INVENTORY.digest(path) for path in
                   (original, entry, Path(__file__), ROOT / 'scripts/analyze-riscv-stack.py',
                    ROOT / 'scripts/record-task-stack-usage.py')},
        'artifact_hashes': {name: INVENTORY.digest(output / name) for name in
                           ('fixture.rs', 'Cargo.lock', 'fixture.elf', 'disassembly.txt', 'bound.json',
                            'elf-sections.json', 'image-memory.json')},
        'limitations': ['starts at _start with a valid 16-byte-aligned RISC-V stack pointer',
            'excludes boot code, interrupts, caller frames and other execution contexts',
            'fixed Journal/Runtime ports and bounded fixture paths only; arbitrary adapters require reanalysis',
            'trusts exact-build LLVM static frame reports and RISC-V ABI call preservation',
            'reference profile differs from default hosted or other target builds',
            'stack bound does not establish a hardware memory map or runtime execution success']}
    (output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps({key: bound[key] for key in
                      ('stack_bound_bytes', 'reachable_functions', 'positive_control_bytes')}))


if __name__ == '__main__':
    main()
