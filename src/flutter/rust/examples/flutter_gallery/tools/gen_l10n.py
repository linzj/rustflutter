#!/usr/bin/env python3
"""Writes the l10n string table and locale placeholders from upstream.

Parses `lib/l10n/gallery_localizations_en.dart` in the upstream clone and
generates:

- `src/l10n/gallery_localizations_en.rs` -- the full English string table
  (upstream's `GalleryLocalizationsEn`): one method per upstream getter, and
  one per parameterized message, with intl's pluralLogic resolved the way the
  English plural rules resolve it (`one` at exactly 1, `other` otherwise).
- `src/l10n/gallery_localizations_<locale>.rs` for each of the other 76
  locales -- a placeholder module, because the port is English-only per
  PORTING.md.

Generated rather than hand-written because the table is upstream's own
wording, and anything retyped drifts from the thing it is a port of.

The upstream clone is located by --upstream, then $GALLERY_UPSTREAM, then the
default below (the same arrangement as extract_catalog.py). The example's own
location is derived from this script's path, so the repo can live anywhere.
"""

import argparse
import glob
import os
import re

DEFAULT_UPSTREAM = 'D:/linzjUbuntu2204/gallery_upstream'
HERE = os.path.dirname(os.path.abspath(__file__))
EXAMPLE = os.path.dirname(HERE)
OUT_DIR = os.path.join(EXAMPLE, 'src', 'l10n')

GETTER_RE = re.compile(
    r"String get (\w+) =>\s*"
    r"(?:'((?:[^'\\]|\\.)*)'"
    r"|\"((?:[^\"\\]|\\.)*)\")\s*;",
    re.S)

METHOD_RE = re.compile(
    r"String (\w+)\(((?:[^()]|\([^)]*\))*)\) \{(.*?)\n  \}", re.S)

PLURAL_KWARG_RE = re.compile(
    r"^\s*(zero|one|two|few|many|other):\s*'((?:[^'\\]|\\.)*)'\s*,?$",
    re.M)


def snake(name):
  """camelCase -> snake_case, with acronym runs kept together
  (rallyAlertsMessageATMFees -> rally_alerts_message_atm_fees)."""
  return re.sub(r'(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])',
                '_', name).lower()


def unescape_dart(text):
  """The escapes a Dart single-quoted string can carry."""
  out = []
  i = 0
  while i < len(text):
    if text[i] == '\\' and i + 1 < len(text):
      nxt = text[i + 1]
      out.append({'n': '\n', 'r': '\r', 't': '\t'}.get(nxt, nxt))
      i += 2
    else:
      out.append(text[i])
      i += 1
  return ''.join(out)


def rust_literal(text):
  """A Rust string literal, wrapped with line continuations so the source stays
  inside a sensible width without changing the text (as gen_catalog.py does)."""
  text = (text.replace('\\', '\\\\').replace('"', '\\"')
          .replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t'))
  if len(text) <= 62:
    return '"%s"' % text
  words, lines, line = text.split(' '), [], ''
  for word in words:
    if line and len(line) + 1 + len(word) > 62:
      lines.append(line)
      line = word
    else:
      line = (line + ' ' + word) if line else word
  if line:
    lines.append(line)
  body = ('\\\n' + ' ' * 22).join(l + ' ' if i < len(lines) - 1 else l
                                  for i, l in enumerate(lines))
  return '"%s"' % body


def template(text):
  """A Dart interpolated string as a Rust format string: `$name` / `${name}`
  become `{name}`, literal braces are doubled. Returns None when the text has
  no interpolation, meaning a plain literal will do."""
  text = unescape_dart(text)
  has_interp = re.search(r'\$(?:\w|\{)', text) is not None
  text = text.replace('{', '{{').replace('}', '}}')
  text = re.sub(r'\$\{(\w+)\}', lambda m: '{%s}' % snake(m.group(1)), text)
  text = re.sub(r'\$(\w+)', lambda m: '{%s}' % snake(m.group(1)), text)
  text = (text.replace('\\', '\\\\').replace('"', '\\"')
          .replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t'))
  return ('"%s"' % text) if has_interp else None


def parse_params(params):
  """`(Object billName, num count)` -> [(rust_name, 'display' | 'count')]."""
  out = []
  for param in params.split(','):
    param = param.strip()
    if not param:
      continue
    match = re.match(r'(Object|num) (\w+)$', param)
    if not match:
      raise ValueError('unrecognized parameter: %r' % param)
    kind = 'display' if match.group(1) == 'Object' else 'count'
    out.append((snake(match.group(2)), kind))
  return out


def parse_en(text):
  """The members of `GalleryLocalizationsEn`, in declaration order. The file
  continues with the regional subclasses (en_GB and friends), which are not
  part of the English table."""
  end = text.index('\nclass GalleryLocalizationsEnAu')
  region = text[:end]
  members = []
  for match in re.finditer(r'%s|%s' % (GETTER_RE.pattern, METHOD_RE.pattern),
                           region, re.S):
    if match.group(1) is not None:
      members.append(('getter', match.group(1),
                      match.group(2) if match.group(2) is not None
                      else match.group(3)))
    else:
      members.append(('method', match.group(4), match.group(5), match.group(6)))
  return members


def emit_getter(name, value):
  return '    pub fn %s(&self) -> &\'static str { %s }' % (
      snake(name), rust_literal(unescape_dart(value)))


def emit_method(name, params_src, body):
  params = parse_params(params_src)
  signature = '    pub fn %s(&self%s) -> String {' % (
      snake(name),
      ''.join(', %s: %s' % (n, 'impl std::fmt::Display' if k == 'display'
                            else 'i64')
              for n, k in params))
  simple = re.match(r"^\s*return\s*'((?:[^'\\]|\\.)*)'\s*;\s*$", body, re.S)
  if simple:
    fmt = template(simple.group(1))
    if fmt is None:
      raise ValueError('%s: a method with no interpolation' % name)
    return '\n'.join([signature, '        format!(%s)' % fmt, '    }'])

  if 'pluralLogic' not in body:
    raise ValueError('%s: unrecognized body shape:\n%s' % (name, body))
  count = params[0][0]
  forms = dict((m.group(1), m.group(2)) for m in PLURAL_KWARG_RE.finditer(body))
  if 'other' not in forms:
    raise ValueError('%s: a plural without an other form' % name)

  lines = [signature]
  skipped = [k for k in forms if k not in ('one', 'other')]
  if skipped:
    lines.append('        // intl\'s English plural rules select `one` at exactly 1 and')
    lines.append('        // `other` otherwise, so the %s form%s upstream declares' %
                 ('/'.join(skipped), 's' if len(skipped) > 1 else ''))
    lines.append('        // (%s) never render%s in English.' % (
        ', '.join('%r' % unescape_dart(forms[k]) for k in skipped),
        's' if len(skipped) == 1 else ''))
  if 'one' in forms:
    one = template(forms['one'])
    other = template(forms['other'])
    lines.append('        if %s == 1 {' % count)
    lines.append('            format!(%s)' % one if one else
                 '            %s.to_string()' % rust_literal(unescape_dart(forms['one'])))
    lines.append('        } else {')
    lines.append('            format!(%s)' % other if other else
                 '            %s.to_string()' % rust_literal(unescape_dart(forms['other'])))
    lines.append('        }')
  else:
    other = template(forms['other'])
    lines.append('        format!(%s)' % other if other else
                 '        %s.to_string()' % rust_literal(unescape_dart(forms['other'])))
  lines.append('    }')
  return '\n'.join(lines)


HEADER = '''// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
'''


def write_en(members):
  lines = [HEADER.rstrip('\n')]
  add = lines.append
  add('')
  add('//! The translations for English (`en`).')
  add('//!')
  add('//! Upstream `lib/l10n/gallery_localizations_en.dart` (flutter/gallery @')
  add('//! d12640d), class `GalleryLocalizationsEn`. Generated by')
  add('//! `tools/gen_l10n.py`; do not edit by hand. The regional subclasses')
  add('//! upstream keeps in the same file (`GalleryLocalizationsEnAu` and')
  add('//! friends) are not part of the table: their strings are English too,')
  add('//! and the port is English-only per PORTING.md.')
  add('')
  add('/// Upstream\'s `GalleryLocalizationsEn`. Zero-sized: every member is a')
  add('/// method, so the value only ever exists to be reached through')
  add('/// `super::gallery_localizations::GalleryLocalizations`\'s `Deref`.')
  add('pub struct GalleryLocalizationsEn;')
  add('')
  add('impl GalleryLocalizationsEn {')
  for member in members:
    if member[0] == 'getter':
      add(emit_getter(member[1], member[2]))
    else:
      add(emit_method(member[1], member[2], member[3]))
  add('}')
  out = os.path.join(OUT_DIR, 'gallery_localizations_en.rs')
  open(out, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines) + '\n')
  print('wrote', out, '(%d members)' % len(members))


def write_placeholder(locale):
  lines = [HEADER.rstrip('\n')]
  add = lines.append
  add('')
  add('//! Generated by `tools/gen_l10n.py`; do not edit by hand.')
  add('//!')
  add('//! aligned to upstream gallery_localizations_%s.dart; strings not' % locale)
  add('//! ported (English only per PORTING.md).')
  out = os.path.join(OUT_DIR, 'gallery_localizations_%s.rs' % locale)
  open(out, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines) + '\n')


def main():
  parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  parser.add_argument(
      '--upstream',
      default=os.environ.get('GALLERY_UPSTREAM', DEFAULT_UPSTREAM),
      help='path to a flutter/gallery clone (pinned at d12640d); '
           'defaults to $GALLERY_UPSTREAM or ' + DEFAULT_UPSTREAM)
  args = parser.parse_args()
  l10n = os.path.join(args.upstream, 'lib', 'l10n')
  os.makedirs(OUT_DIR, exist_ok=True)

  text = open(os.path.join(l10n, 'gallery_localizations_en.dart'),
              encoding='utf-8').read()
  members = parse_en(text)
  names = [snake(m[1]) for m in members]
  if len(set(names)) != len(names):
    raise ValueError('snake_case collision in %s' %
                     [n for n in names if names.count(n) > 1])
  write_en(members)

  locales = sorted(
      os.path.basename(p)[len('gallery_localizations_'):-len('.dart')]
      for p in glob.glob(os.path.join(l10n, 'gallery_localizations_*.dart')))
  locales.remove('en')
  for locale in locales:
    write_placeholder(locale)
  print('placeholders:', len(locales))


if __name__ == '__main__':
  main()
