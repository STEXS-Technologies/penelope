import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from application_workloads_summary import CASES, summarize

class SummaryTests(unittest.TestCase):
    def setUp(self):
        self.metadata={"repeats":2,"rounds":2,"iterations":1,"workers":[1],"work":[64],"mix":dict.fromkeys(CASES,1)}
        self.rows=[dict(repeat=repeat,workers=1,work=64,case=case,round=round_,schema=1,verified=True,calls_per_workflow=calls,iterations_per_worker=1,workflows=1,direct_first=round_%2==0,direct_ns=100,penelope_ns=200,checksum=7) for repeat in range(2) for round_ in range(2) for case,calls in CASES.items()]
    def test_known_twofold_cost_and_declared_mix(self):
        result=summarize(self.rows,self.metadata)
        for row in result['profiles']+result['weighted_estimates']:
            self.assertAlmostEqual(row['overhead_percent'],100)
            self.assertEqual(row['paired_added_median_ns_per_workflow'],100)
    def test_missing_duplicate_unverified_and_substituted_outputs_fail(self):
        bad=copy.deepcopy(self.rows);bad[0]['verified']=False
        substituted=copy.deepcopy(self.rows);substituted[0]['checksum']=8
        for rows in [self.rows[1:],self.rows+[self.rows[0]],bad,substituted]:
            with self.assertRaises(ValueError):summarize(rows,self.metadata)
    def test_mixed_admission_modes_fail(self):
        mixed=copy.deepcopy(self.rows)
        mixed[0]['admission']='atomic'
        with self.assertRaises(ValueError):summarize(mixed,self.metadata)
    def test_invalid_mix_fails(self):
        self.metadata['mix']={case:0 for case in CASES}
        with self.assertRaises(ValueError):summarize(self.rows,self.metadata)

if __name__=='__main__':unittest.main()
