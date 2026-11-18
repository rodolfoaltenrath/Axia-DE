import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'axia_idle_check.py'
spec = importlib.util.spec_from_file_location('axia_idle_check', SCRIPT)
idle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(idle)


class IdleBudgetTests(unittest.TestCase):
    def test_trace_window_excludes_other_syscalls_and_unfinished_lines(self):
        trace = '\n'.join([
            '9.500000 poll([], 0, 5) = 0 <0.005000>',
            '10.000000 poll([], 0, 5) = 0 <0.005000>',
            '10.100000 ppoll([], 0, NULL, NULL, 8) = 0 <0.005000>',
            '10.200000 epoll_wait(4, [], 1, 0) = 0 <0.000100>',
            '10.300000 read(5, "", 2) = 0 <0.000100>',
            '10.400000 poll([], 0, 5 <unfinished ...>',
            '11.000000 poll([], 0, 5) = 0 <0.005000>',
        ])
        self.assertEqual(idle.poll_count(trace, 10, 11), 3)

    def test_session_blocked_for_entire_window_can_have_zero_wakeups(self):
        self.assertEqual(idle.poll_count('9.000000 poll([], 0, -1) = -1 EINTR <22.000000>', 10, 30), 0)

    def test_missing_trace_is_invalid_not_a_zero_activity_pass(self):
        for trace in ('', '9.000000 poll([], 0, 1) = 0 <0.001000>'):
            with self.subTest(trace=trace), self.assertRaises(RuntimeError):
                idle.poll_count(trace, 10, 30)

    def test_medians_enforce_poll_and_memory_budgets(self):
        samples = [{'poll_calls_per_second': p, 'rss_bytes_median': r}
                   for p, r in [(16, 121), (18, 122), (1, 1)]]
        result = idle.evaluate(samples, 3, 15, 120)
        self.assertEqual(result['exceeded'], ['poll_calls_per_second', 'rss_bytes_median'])

    def test_single_outlier_does_not_replace_median(self):
        samples = [{'poll_calls_per_second': p, 'rss_bytes_median': 100}
                   for p in (12, 13, 100)]
        self.assertEqual(idle.evaluate(samples, 3, 15, 120)['exceeded'], [])

    def test_partial_collection_cannot_pass(self):
        with self.assertRaises(RuntimeError):
            idle.evaluate([{'poll_calls_per_second': 1, 'rss_bytes_median': 1}], 3, 15, 120)

    def test_exact_budget_is_accepted(self):
        self.assertEqual(idle.evaluate([{'poll_calls_per_second': 15, 'rss_bytes_median': 120}], 1, 15, 120)['exceeded'], [])

    def test_missing_installation_fails_even_during_calibration_and_preserves_report(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'report.json'
            result = subprocess.run([sys.executable, str(SCRIPT), '--installation',
                                     str(Path(directory) / 'missing'), '--output', str(output),
                                     '--report-only'], capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(json.loads(output.read_text())['status'], 'invalid')

    def test_nonfinite_budgets_are_rejected(self):
        for value in ('nan', 'inf', '-1', '0'):
            with self.subTest(value=value), self.assertRaises(Exception):
                idle.positive(value)
