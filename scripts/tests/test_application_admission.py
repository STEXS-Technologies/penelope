import contextlib
import copy
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

script=Path(__file__).resolve().parents[1]/'benchmark-application-admission.py'
spec=importlib.util.spec_from_file_location('admission',script)
admission=importlib.util.module_from_spec(spec)
spec.loader.exec_module(admission)

class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.metadata={'campaigns':2,'blocks':2,'rounds':2,'iterations':1,'work':64}
        self.rows=[]
        for campaign in range(2):
            for block in range(2):
                modes=['preaccepted','atomic','atomic','preaccepted'] if block%2==0 else ['atomic','preaccepted','preaccepted','atomic']
                for case in admission.CASES:
                    for position,mode in enumerate(modes):
                        for round_ in range(2):
                            self.rows.append(dict(campaign=campaign,block=block,case=case,position=position,round=round_,admission=mode,work=64,workers=1,iterations_per_worker=1,workflows=1,calls_per_workflow=8,verified=True,schema=1,direct_first=round_%2==0,direct_ns=10,penelope_ns=50 if mode=='atomic' else 100,checksum=7))
    def run_summary(self,rows):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'metadata.json').write_text(json.dumps(self.metadata))
            (root/'samples.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
            with contextlib.redirect_stdout(io.StringIO()):return admission.summarize(root)
    def test_known_half_cost(self):
        for row in self.run_summary(self.rows):
            self.assertAlmostEqual(row['change_percent'],-50)
            self.assertAlmostEqual(row['atomic_ns_per_workflow'],50)
    def test_missing_duplicate_wrong_mode_and_output_fail(self):
        wrong_mode=copy.deepcopy(self.rows);wrong_mode[0]['admission']='atomic'
        wrong_output=copy.deepcopy(self.rows);wrong_output[0]['checksum']=8
        for rows in [self.rows[1:],self.rows+[self.rows[0]],wrong_mode,wrong_output]:
            with self.assertRaises(ValueError):self.run_summary(rows)

if __name__=='__main__':unittest.main()
