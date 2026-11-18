import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from urllib.error import HTTPError, URLError

SCRIPT = Path(__file__).resolve().parents[1] / 'copr_publish.py'
spec = importlib.util.spec_from_file_location('copr_publish', SCRIPT)
copr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(copr)


class Clock:
    def __init__(self):
        self.time = 0
        self.delays = []

    def now(self):
        return self.time

    def sleep(self, delay):
        self.time += delay
        self.delays.append(delay)


def http_error(code):
    return HTTPError('https://copr.example/api_3/build/42', code, 'test', {}, io.BytesIO())


class CoprPublishTests(unittest.TestCase):
    def watch(self, responses, **kwargs):
        self.clock = Clock()
        self.fetch = Mock(side_effect=responses)
        copr.watch(42, fetch=self.fetch, sleep=self.clock.sleep,
                   now=self.clock.now, **kwargs)

    def test_accepted_build_is_recorded_before_polling_and_never_resubmitted(self):
        client = SimpleNamespace(config={'copr_url': 'https://copr.example', 'connection_attempts': 5},
                                 build_proxy=SimpleNamespace(create_from_file=Mock(return_value=SimpleNamespace(id=42))))
        clock = Clock()
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'build.json'
            responses = iter([URLError('offline'), 'running', http_error(503), 'succeeded'])

            def fetch(base, build_id, timeout):
                self.assertEqual(json.loads(report.read_text())['build_id'], 42)
                self.assertEqual((base, build_id), ('https://copr.example', 42))
                response = next(responses)
                if isinstance(response, Exception):
                    raise response
                return response

            def wait(build_id, base):
                copr.watch(build_id, base, fetch=fetch, sleep=clock.sleep, now=clock.now)

            with patch.dict('os.environ', {}, clear=True):
                copr.publish(client, 'owner/project', Path('checked.src.rpm'), report, wait=wait)
            self.assertEqual(json.loads(report.read_text())['url'], 'https://copr.example/coprs/build/42')
        client.build_proxy.create_from_file.assert_called_once_with('owner', 'project', 'checked.src.rpm')
        self.assertEqual(client.config['connection_attempts'], 1)
        self.assertEqual(clock.delays, [15, 30, 15])

    def test_upload_error_is_not_retried_or_reported_as_success(self):
        upload = Mock(side_effect=URLError('ambiguous upload response'))
        client = SimpleNamespace(config={}, build_proxy=SimpleNamespace(create_from_file=upload))
        wait = Mock()
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'build.json'
            with self.assertRaises(URLError):
                copr.publish(client, 'owner/project', Path('checked.src.rpm'), report, wait=wait)
            self.assertFalse(report.exists())
        upload.assert_called_once()
        wait.assert_not_called()

    @unittest.skipUnless(importlib.util.find_spec('copr'), 'Copr SDK is installed in the publication job')
    def test_installed_sdk_upload_contract_with_mock_http_transport(self):
        from copr.v3 import Client
        client = Client({'copr_url': 'https://copr.example', 'login': 'test',
                         'username': 'test', 'token': 'not-a-real-token', 'connection_attempts': 5})
        response = Mock(status_code=200)
        response.json.return_value = {'id': 42}
        wait = Mock()
        with tempfile.TemporaryDirectory() as directory, patch.dict('os.environ', {}, clear=True):
            source = Path(directory) / 'checked.src.rpm'
            source.write_bytes(b'test upload payload')
            report = Path(directory) / 'build.json'
            with source.open('rb') as handle, patch('copr.v3.proxies.build.open', return_value=handle, create=True), \
                    patch('copr.v3.requests.requests.request', return_value=response) as request:
                copr.publish(client, 'owner/project', source, report, wait=wait)
            request.assert_called_once()
            self.assertEqual(request.call_args.kwargs['method'], 'POST')
            self.assertEqual(request.call_args.kwargs['url'], 'https://copr.example/api_3/build/create/upload')
            self.assertEqual(json.loads(report.read_text())['build_id'], 42)
        wait.assert_called_once_with(42, 'https://copr.example')

    def test_failed_and_canceled_builds_fail_immediately(self):
        for state in ['failed', 'canceled', 'unknown']:
            with self.subTest(state=state), self.assertRaises(RuntimeError):
                self.watch([state])
            self.assertEqual(self.clock.delays, [])

    def test_permanent_http_errors_are_not_retried(self):
        for code in [401, 403, 404]:
            with self.subTest(code=code), self.assertRaisesRegex(RuntimeError, f'HTTP {code}'):
                self.watch([http_error(code)])
            self.fetch.assert_called_once()
            self.assertEqual(self.clock.delays, [])

    def test_transient_rate_limit_timeout_and_bad_json_recover(self):
        self.watch([http_error(429), TimeoutError(), json.JSONDecodeError('bad', '', 0), 'succeeded'])
        self.assertEqual(self.clock.delays, [15, 30, 60])

    def test_persistent_outage_stops_with_existing_id(self):
        with self.assertRaisesRegex(RuntimeError, '--build-id 42'):
            self.watch([URLError('offline')] * 8)
        self.assertEqual(self.fetch.call_count, 8)
        self.assertLessEqual(max(self.clock.delays), 120)

    def test_total_deadline_is_bounded(self):
        with self.assertRaisesRegex(RuntimeError, 'Timed out'):
            self.watch([URLError('offline')] * 8, timeout=20)
        self.assertEqual(self.clock.time, 20)

    def test_skipped_is_terminal_like_copr_cli(self):
        self.watch(['skipped'])
        self.assertEqual(self.clock.delays, [])

    def test_resume_needs_no_credentials_or_source_package(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict('os.environ', {}, clear=True):
            report = Path(directory) / 'build.json'
            with patch('sys.argv', [str(SCRIPT), '--build-id', '42', '--report', str(report)]), patch.object(copr, 'watch') as wait:
                copr.main()
            wait.assert_called_once_with(42)
            self.assertEqual(json.loads(report.read_text())['build_id'], 42)


if __name__ == '__main__':
    unittest.main()
