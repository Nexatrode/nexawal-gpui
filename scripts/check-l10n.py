#!/usr/bin/env python3
"""Offline catalog/argument audit; optionally include mobile main checkouts."""
import argparse
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

FORMAT = re.compile(r'%(?:(\d+)\$)?[-+0# ]*\d*(?:\.\d+)?(hh|ll|h|l|z|t|j)?([@diuoxXfFeEgGaAcCsSp])')

def arguments(text):
    return sorted((int(m[1] or index + 1), (m[2] or '') + m[3])
                  for index, m in enumerate(FORMAT.finditer(text.replace('%%', ''))))

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'duplicate JSON key: {key}')
        result[key] = value
    return result

def read_json(path):
    return json.loads(path.read_text(), object_pairs_hook=unique_object)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ios', type=Path)
    parser.add_argument('--android', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    gp = read_json(root / 'assets/l10n.json')
    errors = []
    def check(key, value, where):
        if not value.strip(): errors.append(f'blank {where}: {key}')
        if arguments(key) != arguments(value): errors.append(f'arguments changed {where}: {key}')
        if sorted(re.findall(r'\{\d+\}', key)) != sorted(re.findall(r'\{\d+\}', value)):
            errors.append(f'numbered arguments changed {where}: {key}')
    for locale, strings in gp.items():
        for key in gp['en']:
            if key not in strings: errors.append(f'missing GPUI {locale}: {key}')
            else: check(key, strings[key], f'GPUI {locale}')
    active = set()
    for path in (root / 'src').glob('*.rs'):
        for raw in re.findall(r'l10n::(?:t|interpolate)\(\s*"((?:[^"\\]|\\.)*)"', path.read_text()):
            active.add(json.loads('"' + raw + '"'))
    for key in active - gp['en'].keys(): errors.append(f'active GPUI key not cataloged: {key}')
    print(f'GPUI: {len(gp)} locales, {len(gp["en"])} keys, {len(active)} active literal lookups')
    if args.ios:
        strings = read_json(args.ios / 'nexawal/Localizable.xcstrings')['strings']
        locales = set(strings['How to connect']['localizations'])
        for key, entry in strings.items():
            if entry.get('shouldTranslate') is False or not key: continue
            locs = entry.get('localizations', {})
            if set(locs) != locales: errors.append(f'iOS missing locales: {key}')
            for locale, loc in locs.items(): check(key, loc['stringUnit']['value'], f'iOS {locale}')
        for path in (args.ios / 'nexawal').glob('*.swift'):
            for raw in re.findall(r'L10n\.(?:t|neon|format)\(\s*"((?:[^"\\]|\\.)*)"', path.read_text()):
                key = json.loads('"' + raw + '"')
                if key not in strings: errors.append(f'active iOS key not cataloged: {key}')
        print(f'iOS: {len(locales)} locales, {len(strings)} keys')
    if args.android:
        resources = args.android / 'app/src/main/res'
        def xml(path):
            out = {}
            for item in ET.parse(path).getroot():
                if item.tag != 'string': continue
                name = item.attrib['name']
                if name in out: errors.append(f'duplicate Android key {path}: {name}')
                out[name] = ''.join(item.itertext())
            return out
        english = xml(resources / 'values/strings.xml')
        paths = sorted(resources.glob('values-*/strings.xml'))
        for path in paths:
            strings = xml(path)
            if set(strings) != set(english): errors.append(f'Android key mismatch: {path.parent.name}')
            for key in english.keys() & strings.keys(): check(english[key], strings[key], f'Android {path.parent.name}: {key}')
        print(f'Android: {len(paths) + 1} locales, {len(english)} keys')
    for error in errors: print(error)
    if errors: raise SystemExit(1)
    print('Coverage, duplicate-key, nonblank and printf argument checks passed. Not a native-language sign-off.')

if __name__ == '__main__': main()
