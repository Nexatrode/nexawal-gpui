#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('audit', ROOT / 'scripts/check-l10n.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)

class LocalizationTests(unittest.TestCase):
    def test_arguments_allow_reordering_not_loss_or_type_change(self):
        self.assertEqual(audit.arguments('%@ %lld'), audit.arguments('%2$lld %1$@'))
        self.assertNotEqual(audit.arguments('Fee %@%@'), audit.arguments('Fee'))
        self.assertNotEqual(audit.arguments('%lld'), audit.arguments('%@'))
        self.assertEqual(audit.arguments('100%%'), [])

    def test_duplicate_json_keys_are_rejected(self):
        with self.assertRaises(ValueError):
            json.loads('{"Fee":"Frais", "Fee":"Fee"}', object_pairs_hook=audit.unique_object)

    def test_import_preserves_desktop_keys_and_skips_stale_mobile_copy(self):
        with tempfile.TemporaryDirectory(prefix='nexawal-l10n-test-') as directory:
            directory = Path(directory)
            output = directory / 'l10n.json'
            source = directory / 'Localizable.xcstrings'
            output.write_text(json.dumps({'en': {'Desktop': 'Desktop'}, 'fr': {'Desktop': 'Bureau'}}))
            source.write_text(json.dumps({'strings': {
                'Wallet': {'localizations': {'fr': {'stringUnit': {'value': 'Portefeuille'}}}},
                'Obsolete': {'extractionState': 'stale', 'localizations': {'fr': {'stringUnit': {'value': 'Ancien'}}}},
                'Blank': {'localizations': {'fr': {'stringUnit': {'value': ''}}}},
            }}))
            subprocess.run(['bash', str(ROOT / 'scripts/update-l10n.sh')], check=True,
                           env={**os.environ, 'NEXAWAL_XCSTRINGS': str(source), 'NEXAWAL_L10N_OUTPUT': str(output)},
                           capture_output=True, text=True)
            result = json.loads(output.read_text())
            self.assertEqual(result['fr']['Desktop'], 'Bureau')
            self.assertEqual(result['fr']['Wallet'], 'Portefeuille')
            self.assertNotIn('Obsolete', result['fr'])
            self.assertNotIn('Blank', result['fr'])

if __name__ == '__main__': unittest.main()
