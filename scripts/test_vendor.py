import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import vendor


class VendorTests(unittest.TestCase):
    def test_bad_download_preserves_existing_asset(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            target = path / 'voice.bin'
            target.write_bytes(b'old')
            entry = {'path': 'voice.bin', 'size': 4, 'sha256': hashlib.sha256(b'good').hexdigest()}
            with patch('urllib.request.urlopen', return_value=io.BytesIO(b'evil')):
                with self.assertRaises(ValueError):
                    vendor.fetch(entry, 'https://example.invalid', path)
            self.assertEqual(target.read_bytes(), b'old')
            self.assertEqual(list(path.iterdir()), [target])

    def test_verified_download_is_published(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            entry = {'path': 'voice.bin', 'size': 4, 'sha256': hashlib.sha256(b'good').hexdigest()}
            with patch('urllib.request.urlopen', return_value=io.BytesIO(b'good')):
                vendor.fetch(entry, 'https://example.invalid', path)
            self.assertEqual((path / 'voice.bin').read_bytes(), b'good')
            with patch('urllib.request.urlopen', side_effect=AssertionError('must reuse verified asset')):
                vendor.fetch(entry, 'https://example.invalid', path)
