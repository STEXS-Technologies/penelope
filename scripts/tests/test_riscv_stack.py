"""Fail-closed checks for the concrete RISC-V stack analysis."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    'riscv_stack', Path(__file__).resolve().parents[1] / 'analyze-riscv-stack.py')
STACK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STACK)


def frame(symbol, size, classification='static'):
    return {'symbol': symbol, 'frame_bytes': size, 'classification': classification}


class RiscvStack(unittest.TestCase):
    def test_counts_nested_and_branch_alternatives(self):
        functions = {
            '_start': [(0, 'jal', '0x10 <a>'), (4, 'jal', '0x20 <b>'), (8, 'ret', '')],
            'a': [(16, 'jal', '0x30 <c>'), (20, 'ret', '')],
            'b': [(32, 'ret', '')], 'c': [(48, 'ret', '')]}
        result = STACK.analyze(functions, [frame('_start', 16), frame('a', 32),
                                          frame('b', 96), frame('c', 80)])
        self.assertEqual(result['stack_bound_bytes'], 128)
        self.assertEqual(result['worst_conservative_chain'], ['_start', 'a', 'c'])

    def test_resolves_signed_auipc_call_from_instructions(self):
        rows = [(0x2000, 'auipc', 'ra, 0xfffff'),
                (0x2004, 'jalr', '-0x10(ra) <ignored_label>')]
        self.assertEqual(STACK.destination(rows[1][2], rows, 1), 0xff0)
        with self.assertRaises(ValueError):
            STACK.destination('0x0(a0) <f>', rows, 1)

    def test_preserves_duplicate_local_symbols_and_uses_largest_frame(self):
        functions = STACK.parse_disassembly("""00000000 <_start>:
 0: jal 0x10 <f>
 4: jal 0x20 <f>
 8: ret
00000010 <f>:
 10: ret
00000020 <f>:
 20: ret
""")
        self.assertEqual(len(functions), 3)
        result = STACK.analyze(functions, [frame('_start', 16), frame('f', 32), frame('f', 64)])
        self.assertEqual(result['stack_bound_bytes'], 80)
        self.assertEqual(result['reachable_functions'], 3)

    def test_tracks_precompiled_frame_across_cfg(self):
        rows = [(0, 'addi', 'sp, sp, -0x20'), (4, 'beqz', 'a0, 0xc <f+0xc>'),
                (8, 'sw', 'a0, 0x0(sp)'), (12, 'addi', 'sp, sp, 0x20'),
                (16, 'ret', '')]
        self.assertEqual(STACK.disassembled_frame(rows), 32)

    def test_rejects_unbalanced_merge_and_stack_growth(self):
        for rows in [
            [(0, 'beqz', 'a0, 0x8 <f+0x8>'), (4, 'addi', 'sp, sp, -0x10'),
             (8, 'j', '0xc <f+0xc>'), (12, 'j', '0xc <f+0xc>')],
            [(0, 'addi', 'sp, sp, -0x10'), (4, 'j', '0x0 <f>')]]:
            with self.assertRaises(ValueError):
                STACK.disassembled_frame(rows)

    def test_rejects_unknown_stack_writes_and_bad_alignment(self):
        for operation, operands in [('sub', 'sp, sp, a0'), ('mv', 'sp, s0'),
                                     ('addi', 'sp, sp, -0x8')]:
            with self.assertRaises(ValueError):
                STACK.disassembled_frame([(0, operation, operands), (4, 'ret', '')])

    def test_rejects_opaque_transfers_and_traps(self):
        for operation, operands in [('jr', 'a0'), ('jalr', '0x0(a0)'), ('ecall', '')]:
            with self.assertRaises(ValueError):
                STACK.analyze({'_start': [(0, operation, operands)]}, [frame('_start', 0)])

    def test_rejects_missing_and_interior_targets(self):
        for target in ('0x40 <missing>', '0x14 <f+0x4>'):
            with self.assertRaises(ValueError):
                STACK.analyze({'_start': [(0, 'jal', target)],
                               'f': [(16, 'nop', ''), (20, 'ret', '')]},
                              [frame('_start', 0), frame('f', 0)])

    def test_rejects_recursion(self):
        with self.assertRaises(ValueError):
            STACK.analyze({'_start': [(0, 'jal', '0x10 <f>')],
                           'f': [(16, 'jal', '0x0 <_start>')]},
                          [frame('_start', 0), frame('f', 0)])

    def test_rejects_nonstatic_or_conflicting_inventory(self):
        for records in [[frame('_start', 16, 'dynamic')],
                        [frame('_start', 16), frame('_start', 32)]]:
            with self.assertRaises(ValueError):
                STACK.analyze({'_start': [(0, 'ret', '')]}, records)


if __name__ == '__main__':
    unittest.main()
