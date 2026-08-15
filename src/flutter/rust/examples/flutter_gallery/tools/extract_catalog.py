#!/usr/bin/env python3
"""Pulls the Gallery's catalogue out of upstream Dart into JSON.

The titles, subtitles and descriptions are read from the English localisation
rather than retyped, so the Rust port shows upstream's own wording. Anything
retyped would drift.
"""

import json
import re
from collections import Counter

ROOT = 'K:/flutter/dev/integration_tests/new_gallery/lib'

STRING_RE = re.compile(
    r"String get (\w+) =>\s*"
    r"(?:'((?:[^'\\]|\\.)*)'"
    r"|\"((?:[^\"\\]|\\.)*)\")\s*;",
    re.S)

DEMO_RE = re.compile(
    r"GalleryDemo\(\s*"
    r"title: ([^,]+),\s*"
    r"icon: ([^,]+),\s*"
    r"slug: '([^']+)',\s*"
    r"subtitle: ([^,]+),")

STUDY_RE = re.compile(r"'(\w+)': GalleryDemo\(\s*title: ([^,]+),\s*subtitle: ([^,]+),")


def localized():
  """name -> English text. The first class in the file is `en`; later ones are
  en_GB and friends, so first definition wins."""
  text = open(ROOT + '/gallery_localizations_en.dart', encoding='utf-8').read()
  out = {}
  for match in STRING_RE.finditer(text):
    name = match.group(1)
    value = match.group(2) if match.group(2) is not None else match.group(3)
    if name not in out:
      out[name] = (value.replace("\\'", "'")
                   .replace('\\"', '"')
                   .replace('\\n', ' '))
  return out


def key(expr):
  match = re.match(r'localizations\.(\w+)', expr.strip())
  return match.group(1) if match else None


def main():
  strings = localized()
  source = open(ROOT + '/data/demos.dart', encoding='utf-8').read()

  demos = []
  for match in DEMO_RE.finditer(source):
    tail = source[match.end():match.end() + 3000]
    description = re.search(r'description: localizations\.(\w+)', tail)
    category = re.search(r'category: GalleryDemoCategory\.(\w+)', tail)
    demos.append({
        'slug': match.group(3),
        'icon': match.group(2).strip().replace('GalleryIcons.', ''),
        'title': strings.get(key(match.group(1)), ''),
        'subtitle': strings.get(key(match.group(4)), ''),
        'description': strings.get(description.group(1), '') if description else '',
        'category': category.group(1) if category else '?',
    })

  start = source.index('static Map<String, GalleryDemo> studies')
  end = source.index('static List<GalleryDemo> materialDemos')
  studies = [{
      'slug': match.group(1),
      'title': strings.get(key(match.group(2)), ''),
      'subtitle': strings.get(key(match.group(3)), ''),
  } for match in STUDY_RE.finditer(source[start:end])]

  headers = {name: strings[name] for name in strings
             if name.startswith('homeCategory') or name.startswith('homeHeader')}

  # The icon font's codepoint table, which gen_catalog.py needs alongside this.
  icons_source = open(ROOT + '/data/icons.dart', encoding='utf-8').read()
  icons = dict(re.findall(
      r'static const IconData (\w+) = IconData\((0x[0-9a-fA-F]+)', icons_source))

  json.dump({'headers': headers, 'studies': studies, 'demos': demos},
            open('K:/rustflutter/upstream_catalog.json', 'w', encoding='utf-8'),
            indent=1, ensure_ascii=False)
  json.dump(icons, open('K:/rustflutter/upstream_icons.json', 'w', encoding='utf-8'),
            indent=1)
  print('icons:', len(icons))

  print('localized strings:', len(strings))
  print('studies:', len(studies), [s['slug'] for s in studies])
  print(Counter(d['category'] for d in demos))
  print(json.dumps(headers, ensure_ascii=False, indent=1))
  for demo in demos[:2]:
    print(json.dumps(demo, ensure_ascii=False))


if __name__ == '__main__':
  main()
