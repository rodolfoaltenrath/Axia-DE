import importlib.machinery
import importlib.util
import os
from pathlib import Path
import shutil
import tempfile
import unittest

HERE = Path(__file__).resolve().parents[1]
loader = importlib.machinery.SourceFileLoader('fedora_session', str(HERE / 'fedora-session.in'))
spec = importlib.util.spec_from_loader(loader.name, loader)
session = importlib.util.module_from_spec(spec)
loader.exec_module(session)


@unittest.skipUnless(shutil.which('xdg-user-dirs-update'), 'xdg-user-dirs is not installed')
class UserDirectoryTests(unittest.TestCase):
    def test_new_account_gets_standard_folders(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = dict(os.environ, HOME=tmp, XDG_CONFIG_HOME=str(root / '.config'),
                       LC_ALL='C', LANG='C')
            session.initialize_user_dirs(env)
            config = root / '.config/user-dirs.dirs'
            self.assertTrue(config.is_file())
            for name in ['Downloads', 'Documents', 'Desktop', 'Pictures', 'Music', 'Videos']:
                self.assertTrue((root / name).is_dir(), name)
            before = config.read_bytes()
            session.initialize_user_dirs(env)
            self.assertEqual(before, config.read_bytes())

    def test_existing_custom_and_disabled_folders_are_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / '.config/user-dirs.dirs'
            config.parent.mkdir()
            content = 'XDG_DOCUMENTS_DIR="$HOME/Custom/Missing"\nXDG_DOWNLOAD_DIR="$HOME"\n'
            config.write_text(content)
            session.initialize_user_dirs(dict(os.environ, HOME=tmp, XDG_CONFIG_HOME=str(config.parent)))
            self.assertEqual(content, config.read_text())
            self.assertFalse((root / 'Downloads').exists())

    def test_existing_folders_keep_their_names_and_contents(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ['Music', 'Pictures', 'Documentos']:
                (root / name).mkdir()
                (root / name / 'keep.txt').write_text('keep')
            env = dict(os.environ, HOME=tmp, XDG_CONFIG_HOME=str(root / '.config'),
                       LC_ALL='C', LANG='C')
            session.initialize_user_dirs(env)
            config = (root / '.config/user-dirs.dirs').read_text()
            for key, name in [('MUSIC', 'Music'), ('PICTURES', 'Pictures'), ('DOCUMENTS', 'Documentos')]:
                self.assertIn(f'XDG_{key}_DIR="$HOME/{name}"', config)
                self.assertEqual('keep', (root / name / 'keep.txt').read_text())
            self.assertFalse((root / 'Documents').exists())


if __name__ == '__main__':
    unittest.main()
