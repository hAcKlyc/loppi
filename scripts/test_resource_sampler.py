import csv
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import sample_macos_resources as sampler
import report_macos_resources as report
from audit_recording_coverage import coverage


class Sampling(unittest.TestCase):
    def test_attribution_requires_bundle_and_coalition_and_checks_pid_reuse(self):
        entries = "10\t1\ta\t/test/hear.app/Contents/MacOS/hear-desktop\n20\t1\tb\t/test/hear.app/Contents/MacOS/hear-core\n21\t20\tc\t/test/hear.app/Contents/MacOS/hear-core\n30\t1\td\t/System/com.apple.WebKit.WebContent\n31\t1\te\t/System/com.apple.WebKit.WebContent\n"
        cache = {}
        with patch.object(sampler, "command", return_value=entries), patch.object(sampler, "coalition", side_effect=lambda pid: "ours" if pid in (10,30) else "foreign"):
            _, roles, unknown = sampler.inventory(Path('/test/hear.app'), cache)
        self.assertFalse(unknown)
        self.assertEqual(roles, {10:'hear-desktop',20:'hear-core',21:'hear-capture',30:'hear-webview'})
        with patch.object(sampler, "command", return_value=entries.replace('30\t1\td', '30\t1\treused')), patch.object(sampler, "coalition", return_value='foreign'):
            _, roles, _ = sampler.inventory(Path('/test/hear.app'), cache)
        self.assertNotIn(30, roles)
        self.assertNotIn((30,'d'), cache)

    def test_coverage_reports_holes_and_duplicates_without_counting_them_twice(self):
        result=coverage([(0,10),(5,15),(20,35)],0,30)
        self.assertEqual(result['covered_ms'],25)
        self.assertEqual(result['missing_ms'],5)
        self.assertEqual(result['overlap_ms'],5)
        self.assertEqual(result['first_100_gaps'],[[15,20]])
        self.assertEqual(coverage([],0,30)['missing_ms'],30)

    def test_rotation_is_bounded_and_reused_pids_do_not_form_one_memory_curve(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'sample.csv'
            for _ in range(9):
                path.write_text('x'*20)
                sampler.rotate(path, maximum=10, keep=2)
            self.assertEqual(len(list(Path(directory).iterdir())), 2)
            with path.open('w',newline='') as f:
                writer=csv.writer(f);writer.writerow(sampler.HEADER)
                for second,birth,mem in [(0,'first',100),(1,'first',200),(5,'second',50)]:
                    writer.writerow([f'2026-10-16T00:00:0{second}Z','hear-capture','present','/test',42,1,1,mem,mem,0,0,0,0,mem,1,birth])
            groups,times,absent=report.read_samples([path])
            self.assertEqual(len(groups),2)
            result=report.summarize(groups,times,absent)
            self.assertEqual(len(result['processes']),2)
            self.assertEqual({p['birth'] for p in result['processes']},{'first','second'})


if __name__ == '__main__':
    unittest.main()
