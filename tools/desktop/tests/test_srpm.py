import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('srpm', HERE / 'srpm.py')
srpm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(srpm)


class SnapshotTests(unittest.TestCase):
    def test_release_is_embedded_and_other_sections_survive(self):
        source = 'Name: axia-de\nVersion: 0.8.7\nRelease: 3%{?dist}\n\nSummary: Axia\n'
        result, release = srpm.snapshot_spec(source, '0.8.7', '20261102210000123456', 'abcdef012345')
        self.assertEqual(release, '3.20261102210000123456.gitabcdef012345')
        self.assertIn(f'Release:        {release}%{{?dist}}\n\nSummary: Axia\n', result)
        self.assertNotIn('axia_snapshot', result)

    def test_version_mismatch_is_refused(self):
        with self.assertRaises(ValueError):
            srpm.snapshot_spec('Version: 0.8.6\nRelease: 3%{?dist}\n', '0.8.7', '20261102210000123456', 'abcdef012345')

    def test_snapshot_release_cannot_be_injected(self):
        for stamp, commit in [('1\n%post\necho bad', 'abcdef012345'), ('20261102210000123456', 'abc\n%post')]:
            with self.assertRaises(ValueError):
                srpm.snapshot_spec('Version: 0.8.7\nRelease: 3%{?dist}\n', '0.8.7', stamp, commit)


if __name__ == '__main__':
    unittest.main()
