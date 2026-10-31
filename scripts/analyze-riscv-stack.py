#!/usr/bin/env python3
"""Conservatively sum a closed RISC-V call graph and static function frames.

Input is llvm-objdump --disassemble --no-show-raw-insn output plus LLVM .su
files from the exact linked binary's build. Reject indirect transfers,
recursion, ambiguous compiler frames, and unsupported stack-pointer writes.
This is a concrete binary/profile analysis, never a bound for arbitrary ports.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import re

SPEC = importlib.util.spec_from_file_location(
    'stack_inventory', Path(__file__).with_name('record-task-stack-usage.py'))
INVENTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INVENTORY)
BRANCHES = {'beq', 'bne', 'blt', 'bge', 'bltu', 'bgeu', 'beqz', 'bnez',
            'blez', 'bgez', 'bltz', 'bgtz', 'bgt', 'ble', 'bgtu', 'bleu'}
CALLS = {'jal', 'jalr', 'call'}
JUMPS = {'j', 'jr', 'tail'}


def parse_disassembly(text):
    functions = {}
    current = None
    for line in text.splitlines():
        header = re.fullmatch(r'([0-9a-f]+) <(.+)>:', line)
        if header:
            current = header[2]
            # Local monomorphizations may share a mangled name across crates.
            # Keep each address distinct; never overwrite another function.
            if current in functions:
                prior = functions.pop(current)
                functions[current + '@0x' + format(prior[0][0], 'x')] = prior
                current += '@0x' + header[1].lstrip('0')
            elif any(key.startswith(current + '@0x') for key in functions):
                current += '@0x' + header[1].lstrip('0')
            functions[current] = []
        elif current and (row := re.match(r'^\s*([0-9a-f]+):\s+(\S+)\s*(.*)$', line)):
            functions[current].append((int(row[1], 16), row[2], row[3]))
    if not functions or any(not rows for rows in functions.values()):
        raise ValueError('missing or empty disassembled function')
    return functions


def destination(arguments, rows=None, index=None):
    match = re.search(r'(0x[0-9a-f]+) <(.+)>', arguments)
    if match:
        return int(match[1], 16)
    relative = re.fullmatch(r'(-?0x[0-9a-f]+)\((\w+)\) <.+>', arguments)
    if relative and rows is not None and index:
        address, operation, previous = rows[index - 1]
        upper = re.fullmatch(r'(\w+), (0x[0-9a-f]+)', previous)
        if (operation == 'auipc' and upper and upper[1] == relative[2]
                and rows[index][0] == address + 4):
            immediate = int(upper[2], 16)
            if immediate >= 1 << 19:
                immediate -= 1 << 20
            return (address + (immediate << 12) + int(relative[1], 0)) & 0xfffffffe
    raise ValueError('unresolved control transfer: ' + arguments)


def call_graph(functions, entry):
    owners = {}
    starts = {}
    for name, rows in functions.items():
        starts[rows[0][0]] = name
        for address, _, _ in rows:
            if address in owners:
                raise ValueError('overlapping disassembly')
            owners[address] = name
    graph = {}
    pending = [entry]
    while pending:
        name = pending.pop()
        if name in graph:
            continue
        if name not in functions:
            raise ValueError('missing function: ' + name)
        children = set()
        for index, (_, operation, arguments) in enumerate(functions[name]):
            if operation in {'ecall', 'ebreak', 'unimp', 'mret', 'sret', 'uret'}:
                raise ValueError('unsupported trap or privilege transfer: ' + operation)
            if operation in CALLS | JUMPS | BRANCHES:
                address = destination(arguments, functions[name], index)
                if address not in owners:
                    raise ValueError('transfer outside inventoried instructions')
                target = owners[address]
                if target != name:
                    if address not in starts:
                        raise ValueError('transfer into another function interior')
                    children.add(target)
                elif operation in CALLS:
                    raise ValueError('recursive call within function')
            elif operation == 'ret':
                if arguments:
                    raise ValueError('unsupported return operands')
        graph[name] = sorted(children)
        pending.extend(children)
    return graph


def disassembled_frame(rows):
    """Track SP on all CFG paths for precompiled functions lacking LLVM .su.

    Register-sized or realigned SP adjustments are rejected. Calls preserve SP
    in the RISC-V ABI; their separate frames are added by the call-chain sum.
    """
    indices = {row[0]: i for i, row in enumerate(rows)}
    states = {}
    pending = [(0, 0)]
    maximum = 0
    while pending:
        index, depth = pending.pop()
        if index in states:
            if states[index] != depth:
                raise ValueError('inconsistent SP at CFG merge or stack-growing loop')
            continue
        states[index] = depth
        _, operation, arguments = rows[index]
        operands = [value.strip() for value in arguments.split(',')]
        if operands and operands[0] == 'sp' and operation not in BRANCHES | {'sw', 'sd', 'sh', 'sb', 'fsw', 'fsd'}:
            if operation != 'addi' or len(operands) != 3 or operands[1] != 'sp':
                raise ValueError('unsupported stack-pointer write')
            depth -= int(operands[2], 0)
            if depth < 0 or depth % 16:
                raise ValueError('unbalanced or unaligned RISC-V stack')
        maximum = max(maximum, depth)
        if operation == 'ret':
            if depth:
                raise ValueError('return with unbalanced stack')
            continue
        if operation in JUMPS | BRANCHES:
            address = destination(arguments)
            if address in indices:
                pending.append((indices[address], depth))
            if operation in JUMPS:
                continue
        if index + 1 < len(rows):
            pending.append((index + 1, depth))
        elif operation not in CALLS:
            raise ValueError('unexpected fallthrough beyond function')
    return maximum


def analyze(functions, records, entry='_start'):
    graph = call_graph(functions, entry)
    compiler = {}
    for row in records:
        compiler.setdefault(row['symbol'], set()).add(
            (row['frame_bytes'], row['classification']))
    frames = {}
    for name in graph:
        symbol = name.split('@0x')[0]
        if symbol in compiler:
            alternatives = compiler[symbol]
            instances = sum(key.split('@0x')[0] == symbol for key in functions)
            if len(alternatives) > instances:
                raise ValueError('conflicting compiler frames; use a fresh build directory')
            if any(classification != 'static' for _, classification in alternatives):
                raise ValueError('nonstatic compiler frame')
            # Conservatively assign the largest same-name frame to every local
            # copy; LLVM .su does not encode the final linker address.
            size = max(size for size, _ in alternatives)
            frames[name] = {'bytes': size, 'source': 'LLVM stack usage'}
        else:
            frames[name] = {'bytes': disassembled_frame(functions[name]),
                            'source': 'disassembly CFG SP tracking'}
    active = set()
    totals = {}
    chains = {}

    def bound(name):
        if name in active:
            raise ValueError('recursive call graph')
        if name in totals:
            return totals[name]
        active.add(name)
        child = max(graph[name], key=bound) if graph[name] else None
        totals[name] = frames[name]['bytes'] + (bound(child) if child else 0)
        chains[name] = [name] + (chains[child] if child else [])
        active.remove(name)
        return totals[name]

    total = bound(entry)
    return {'entry': entry, 'stack_bound_bytes': total,
            'reachable_functions': len(graph), 'frames': frames, 'edges': graph,
            'worst_conservative_chain': chains[entry],
            'method': 'sum maximum static frame along every acyclic reachable call chain',
            'scope': 'provided disassembly and exact-build compiler frame records; excludes entry caller, interrupts and other execution contexts'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('disassembly', type=Path)
    parser.add_argument('usage_directory', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    records = []
    for path in sorted(args.usage_directory.glob('*.su')):
        records.extend(INVENTORY.parse_usage(path.read_text()))
    if not records:
        raise ValueError('no compiler frame records')
    result = analyze(parse_disassembly(args.disassembly.read_text()), records)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: result[key] for key in
                      ('stack_bound_bytes', 'reachable_functions')}))


if __name__ == '__main__':
    main()
