"""python scripts/publish_artifacts_test.py — the Publish workflow's script, without a server."""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('publish', Path(__file__).parent / 'publish-artifacts.py')
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)

ENV = {'FTP_HOST': 'example.test', 'FTP_USERNAME': 'test', 'FTP_PASSWORD': 'test-only', 'FTP_DIRECTORY': '/www/downloads/VMA/'}


class PublishTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name) / 'artifacts'
        self.source = Path(temp.name) / 'source'
        (self.source / 'docs').mkdir(parents=True)
        (self.source / 'Cargo.toml').write_text('[package]\nname = "vma"\nversion = "2.3.0"\n')
        for rel in publish.DOCUMENTS.values():
            (self.source / rel).write_bytes(b'%PDF')

    def file(self, rel, data=b'artifact'):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def packages(self, version='2.3.0'):
        for p in publish.PLATFORMS:
            self.file(f'vma-{p}/vma-{version}-{p}.' + ('zip' if p.startswith('windows') else 'tar.gz'))

    def test_collects_every_platform_and_the_manuals(self):
        self.packages()
        self.file('other/notes.txt')
        files, version = publish.collect_files(self.root, self.source)
        self.assertEqual(version, (2, 3, 0))
        self.assertEqual(len(files), 8)
        self.assertEqual(list(files)[-3:], ['README.pdf', 'VMA-Manual.pdf', 'VMA-Manual.hu.pdf'])

    def test_rejects_a_missing_platform(self):
        self.packages()
        (self.root / 'vma-linux-x64/vma-2.3.0-linux-x64.tar.gz').unlink()
        with self.assertRaisesRegex(ValueError, 'platform'):
            publish.collect_files(self.root, self.source)

    def test_rejects_another_version_than_cargo_toml(self):
        self.packages('2.2.0')
        with self.assertRaisesRegex(ValueError, 'Cargo.toml version'):
            publish.collect_files(self.root, self.source)

    def test_rejects_two_versions(self):
        self.packages()
        self.file('old/vma-2.2.0-linux-x64.tar.gz')
        with self.assertRaisesRegex(ValueError, 'exactly one version'):
            publish.collect_files(self.root, self.source)

    def test_rejects_a_missing_manual_before_connecting(self):
        self.packages()
        (self.source / 'docs/VMA-Manual.hu.pdf').unlink()
        with patch.dict(os.environ, ENV), patch.object(publish.ftplib, 'FTP_TLS') as ftp:
            with self.assertRaisesRegex(ValueError, 'VMA-Manual.hu.pdf'):
                publish.upload(self.root, self.source)
            ftp.assert_not_called()

    def test_uploads_with_tls_checks_sizes_and_removes_older_packages(self):
        self.packages()
        ftp = MagicMock()
        def size(name):
            return 4 if name.endswith('.pdf') else 8
        ftp.size.side_effect = size
        ftp.nlst.return_value = ['vma-2.1.0-linux-x64.tar.gz', 'vma-2.3.0-linux-x64.tar.gz', 'README.pdf', 'index.php', 'vma-2.4.0-linux-x64.tar.gz']
        with patch.dict(os.environ, ENV), patch.object(publish.ftplib, 'FTP_TLS') as cls:
            cls.return_value.__enter__.return_value = ftp
            publish.upload(self.root, self.source)
            self.assertTrue(cls.call_args.kwargs['context'].check_hostname)
        ftp.prot_p.assert_called_once()
        stored = [c.args[0] for c in ftp.storbinary.call_args_list]
        self.assertEqual(len(stored), 8)
        self.assertEqual(stored[-1], 'STOR VMA-Manual.hu.pdf')
        ftp.delete.assert_called_once_with('vma-2.1.0-linux-x64.tar.gz')


if __name__ == '__main__':
    unittest.main()
