#!/usr/bin/env python3
"""Pulls the Gallery's catalogue out of upstream Dart into JSON.

The titles, subtitles and descriptions are read from the English localisation
rather than retyped, so the Rust port shows upstream's own wording. Anything
retyped would drift.

The data source is the flutter/gallery repo itself (pinned at d12640d):
`lib/l10n/gallery_localizations_en.dart`, `lib/data/demos.dart` and
`lib/data/icons.dart`. No Flutter framework checkout is needed.

The upstream clone is located by --upstream, then $GALLERY_UPSTREAM, then the
default below. The JSON lands next to this script, where gen_catalog.py reads
it; both files are regenerable intermediates, not source.
"""

import argparse
import json
import os
import re
from collections import Counter

DEFAULT_UPSTREAM = 'D:/linzjUbuntu2204/gallery_upstream'
HERE = os.path.dirname(os.path.abspath(__file__))

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


def localized(root):
  """name -> English text. The first class in the file is `en`; later ones are
  en_GB and friends, so first definition wins."""
  text = open(root + '/l10n/gallery_localizations_en.dart', encoding='utf-8').read()
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
  parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  parser.add_argument(
      '--upstream',
      default=os.environ.get('GALLERY_UPSTREAM', DEFAULT_UPSTREAM),
      help='path to a flutter/gallery clone (pinned at d12640d); '
           'defaults to $GALLERY_UPSTREAM or ' + DEFAULT_UPSTREAM)
  args = parser.parse_args()
  root = os.path.join(args.upstream, 'lib')

  strings = localized(root)
  source = open(root + '/data/demos.dart', encoding='utf-8').read()

  demos = []
  for match in DEMO_RE.finditer(source):
    tail = source[match.end():match.end() + 3000]
    description = re.search(r'description:\s*localizations\.(\w+)', tail)
    category = re.search(r'category: GalleryDemoCategory\.(\w+)', tail)
    # two-pane is a DeferredWidget over the dual_screen package: deferred
    # loading is in scope but not ported yet, and the catalogue deliberately
    # lists the four screens that are.
    if match.group(3) == 'two-pane':
      continue
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
  icons_source = open(root + '/data/icons.dart', encoding='utf-8').read()
  icons = dict(re.findall(
      r'static const IconData (\w+) = IconData\(\s*(0x[0-9a-fA-F]+)', icons_source))

  json.dump({'headers': headers, 'studies': studies, 'demos': demos},
            open(os.path.join(HERE, 'upstream_catalog.json'), 'w', encoding='utf-8'),
            indent=1, ensure_ascii=False)
  json.dump(icons, open(os.path.join(HERE, 'upstream_icons.json'), 'w', encoding='utf-8'),
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
