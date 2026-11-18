"""Build boundaries that keep app growth out of the shell compilation unit."""
from pathlib import Path
import shlex
import subprocess
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[3]


class RustBuildPartitionTests(unittest.TestCase):
    def test_native_benchmark_reuses_shell_artifacts_without_overwriting_c_archive(self):
        result = subprocess.run(
            ['make', '-n', '-B', '-C', str(ROOT / 'server'), 'build/axia-bench',
             'build/bench-rust/release/libaxia_bench.a', 'lordbench-test', 'GPU=1'],
            capture_output=True, text=True, check=True,
        )
        cargo = []
        for line in result.stdout.splitlines():
            words = shlex.split(line)
            if 'cargo' in words and '--target-dir' in words:
                cargo.append(words[words.index('cargo'):])
        shell = next(args for args in cargo if 'build' in args and '-p' not in args)
        native = next(args for args in cargo if '--bin' in args and 'axia-bench' in args)
        native_test = next(args for args in cargo if '--test' in args and 'lordbench-native' in args)
        base = next(args for args in cargo if '-p' in args and '--features' not in args)
        target = lambda args: args[args.index('--target-dir') + 1]
        self.assertEqual(target(shell), target(native))
        self.assertEqual(target(shell), target(native_test))
        self.assertNotEqual(target(shell), target(base))
        self.assertLess(cargo.index(shell), cargo.index(native))

    def test_apps_have_no_production_dependency_on_shell_or_other_apps(self):
        crates = list((ROOT / 'rust').glob('axia-app-*/Cargo.toml'))
        self.assertTrue(crates)
        for manifest in crates:
            with self.subTest(crate=manifest.parent.name):
                data = tomllib.loads(manifest.read_text())
                dependencies = data.get('dependencies', {})
                self.assertNotIn('axia-shell', dependencies)
                self.assertFalse(any(name.startswith('axia-app-') for name in dependencies))

    def test_rpm_platform_checks_do_not_compile_rust_test_profiles(self):
        # Exercise Make's expanded graph, including recipes added in included .mk files.
        result = subprocess.run(
            ['make', '-n', '-C', str(ROOT / 'server'), 'test-platform', 'SKIP_RUST_TESTS=1'],
            capture_output=True, text=True, check=True,
        )
        self.assertNotRegex(result.stdout, r'(?m)^\s*\S*cargo\s+test\b')
        self.assertIn('./build/switcher-ffi-test', result.stdout)
        self.assertIn('./build/settings-hw-test', result.stdout)
