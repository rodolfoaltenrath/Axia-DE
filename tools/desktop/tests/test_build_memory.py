import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'build_memory.py'
spec = importlib.util.spec_from_file_location('build_memory', SCRIPT)
memory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(memory)


class BuildMemoryTests(unittest.TestCase):
    def test_only_descendants_contribute_to_peaks(self):
        table = {10: (1, 100, 'make'), 20: (10, 200, 'cargo'),
                 30: (20, 300, 'rustc'), 40: (1, 9000, 'rustc')}
        self.assertEqual(memory.sample(10, table), (600, 300))

    @unittest.skipUnless(sys.platform == 'linux', 'build telemetry uses /proc')
    def test_build_failure_is_preserved_and_report_is_written(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'memory.json'
            result = subprocess.run(
                [sys.executable, str(SCRIPT), '--output', str(report), '--',
                 sys.executable, '-c', 'import time; time.sleep(0.3); raise SystemExit(7)'],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            data = json.loads(report.read_text())
            self.assertEqual(data['exit_code'], 7)
            self.assertGreater(data['peak_process_tree_rss_bytes'], 0)
            self.assertEqual(data['peak_single_rustc_rss_bytes'], 0)
