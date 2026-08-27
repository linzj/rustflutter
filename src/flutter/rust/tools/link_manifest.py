#!/usr/bin/env python3
# Copyright 2013 The Flutter Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Writes what an application has to link, read off the engine's own link line.

An application built by Cargo cannot ask GN anything: it has a build.rs, a path
to an output directory, and no way to walk a dependency graph it is not part of.
So the list of system libraries it needs used to be copied into the generated
build.rs by hand -- and rotted, exactly as a copied list does. `uiautomationcore`
was added to the host when the accessibility bridge landed and never reached the
copy, so every generated project failed to link until someone noticed.

The list is not a copy any more. GN already computes it, in full and
transitively, for the one target that actually links the engine: the shared
library. That expansion is written into the target's own .ninja file as `libs =`
and `frameworks =`, together with the names both artifacts end up with, and this
reads them from there and writes them where a build.rs can find them.

Which means the manifest cannot drift: it *is* the link line, and a library
added anywhere under the engine reaches an application the moment gn regenerates
-- there is nothing to remember.
"""

import argparse
import os
import re
import sys

# `key = value` at the top level of a .ninja file, or indented inside a build
# statement's variable block. Both forms appear in a target's file: the name at
# the top, the link line inside the rule that produces it.
ASSIGNMENT = re.compile(r'^\s*([a-z_]+) = ?(.*)$')


def variables(path):
  """Every `key = value` in a .ninja file, last one winning."""
  found = {}
  with open(path, encoding='utf-8') as handle:
    for line in handle:
      match = ASSIGNMENT.match(line.rstrip('\n'))
      if match:
        found[match.group(1)] = match.group(2).strip()
  return found


def library_names(value):
  """The libraries in a `libs =` line, however the platform spells them.

  GN expands `{{libs}}` with whatever the toolchain's `lib_switch` and
  `lib_dir_switch` are: `user32.lib` on Windows, `-luser32` everywhere else.
  What Cargo wants is neither -- it wants the bare name -- so both spellings
  are reduced to it.
  """
  names = []
  for token in value.split():
    if token.startswith('-l'):
      token = token[2:]
    if token.lower().endswith('.lib'):
      token = token[:-4]
    if token and token not in names:
      names.append(token)
  return names


def framework_names(value):
  """The frameworks in a `frameworks =` line, which is macOS only.

  Spelled `-framework Cocoa`, as two tokens, or occasionally as one.
  """
  names = []
  tokens = value.split()
  index = 0
  while index < len(tokens):
    token = tokens[index]
    if token == '-framework':
      index += 1
      if index < len(tokens):
        names.append(tokens[index])
    elif token.startswith('-framework'):
      names.append(token[len('-framework'):])
    elif token:
      names.append(token.removesuffix('.framework'))
    index += 1
  return [name for name in names if name]


def artifact(ninja):
  """What a target's .ninja file says its output is called.

  `target_output_name` already carries whatever prefix the linker tool gives
  its outputs -- `librustflutter_engine` where a platform wants one, plain
  `rustflutter_engine` on Windows -- so the whole name is here and none of it
  has to be guessed from the platform.
  """
  found = variables(ninja)
  name = found.get('target_output_name')
  extension = found.get('output_extension', '')
  if not name:
    raise SystemExit('No target_output_name in %s' % ninja)
  return name + extension


def link_name(artifact_name):
  """What `-l` wants: the name with its prefix and extension taken back off."""
  name = artifact_name
  if name.startswith('lib'):
    name = name[3:]
  return name.split('.', 1)[0]


def main(argv):
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument('--archive-ninja', required=True,
                      help="the static_library target's generated .ninja")
  parser.add_argument('--library-ninja', required=True,
                      help="the shared_library target's generated .ninja")
  parser.add_argument('--archive-dir', required=True,
                      help='where the archive lands, relative to the output '
                           'directory')
  parser.add_argument('--os', required=True, help='GN\'s current_os')
  parser.add_argument('--output', required=True)
  parser.add_argument('--depfile',
                      help='where to record the two .ninja files this read, so '
                           'that ninja reruns it when either changes. They '
                           'cannot be declared as inputs: gn writes them, and '
                           'nothing in the build graph does.')
  args = parser.parse_args(argv)

  windows = args.os == 'win'
  apple = args.os in ('mac', 'ios')

  archive = artifact(args.archive_ninja)
  library = artifact(args.library_ninja)

  # What to hand `-l dylib=`. On Windows the file to name is the import library
  # beside the DLL -- `rustflutter_engine.dll.lib` -- and rustc gets there by
  # appending `.lib`, so what it is given keeps the `.dll`. Everywhere else it
  # is the bare name.
  library_link_name = library if windows else link_name(library)
  archive_link_name = link_name(archive)

  # How an executable is told to look beside itself for the library. Windows
  # needs nothing: its loader searches there first.
  if windows:
    rpath = ''
  elif apple:
    rpath = '@executable_path'
  else:
    rpath = '$ORIGIN'

  link = variables(args.library_ninja)
  libraries = library_names(link.get('libs', ''))
  frameworks = framework_names(link.get('frameworks', ''))

  lines = [
      '# Generated by //flutter/rust:rustflutter_link_manifest. Do not edit.',
      '#',
      '# What an application links, both ways. Read off the engine\'s own link',
      '# line by tools/link_manifest.py rather than copied, so that a library',
      '# added anywhere under the engine reaches an application by itself.',
      '#',
      '# `lib` and `framework` are needed only by the archive: a shared library',
      '# resolved all of them at its own link time.',
      '',
      'version = 1',
      'os = %s' % args.os,
      '',
      'archive = %s' % (args.archive_dir + '/' + archive),
      'archive_link_name = %s' % archive_link_name,
      '',
      'library = %s' % library,
      'library_link_name = %s' % library_link_name,
      'library_rpath = %s' % rpath,
      '',
  ]
  lines += ['lib = %s' % name for name in libraries]
  lines += ['framework = %s' % name for name in frameworks]

  text = '\n'.join(lines) + '\n'
  directory = os.path.dirname(os.path.abspath(args.output))
  if directory:
    os.makedirs(directory, exist_ok=True)

  if args.depfile:
    os.makedirs(os.path.dirname(os.path.abspath(args.depfile)), exist_ok=True)
    with open(args.depfile, 'w', encoding='utf-8', newline='\n') as handle:
      handle.write('%s: %s %s\n' % (args.output.replace('\\', '/'),
                                    args.archive_ninja.replace('\\', '/'),
                                    args.library_ninja.replace('\\', '/')))

  # Written only when it differs, so that an application's `rerun-if-changed`
  # on it does not fire on every regeneration.
  if os.path.exists(args.output):
    with open(args.output, encoding='utf-8') as handle:
      if handle.read() == text:
        return 0
  with open(args.output, 'w', encoding='utf-8', newline='\n') as handle:
    handle.write(text)
  return 0


if __name__ == '__main__':
  sys.exit(main(sys.argv[1:]))
