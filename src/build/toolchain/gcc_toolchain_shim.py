#!/usr/bin/env python3
# Copyright 2013 The Flutter Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""The gcc_toolchain steps that a Windows host has no shell to run.

`gcc_toolchain.gni` assembles some of its tool commands out of shell: `rm -f x
&& ar ...`, `{ readelf ... | grep ...; nm ... | cut ...; } > toc`, `if ! cmp -s
a b; then mv a b; fi`, `touch`, `ln -f ... || cp`. Ninja on Windows starts each
command with CreateProcess and no shell at all -- not `cmd`, so not even `&&`
works, let alone `rm` or `cmp`. A Windows host therefore cannot drive a
GCC-style toolchain, which is what cross-compiling to Android needs.

Each subcommand here is one tool's whole command as a single process: `ar` does
its own removal first, `solink` links and then writes the TOC and then strips.
That is why some of these run other programs rather than only touching files.

Nothing here is Windows-specific; it is only *called* on Windows, because a
POSIX host already has the shell the original commands assume.
"""

import argparse
import os
import shutil
import subprocess
import sys


def _run(command):
  """Runs a command with no shell involved, returning its exit code."""
  return subprocess.call(command)


def _unlink(path):
  """Removes a file if it is there. `rm -f`."""
  try:
    os.remove(path)
  except FileNotFoundError:
    pass


def _replace_if_different(temporary, final):
  """`if ! cmp -s tmp final; then mv tmp final; fi`.

  The comparison is the point: the toolchain declares these outputs with
  `restat = true`, so leaving an unchanged file's timestamp alone is what stops
  a rebuild from cascading into everything downstream.
  """
  same = False
  if os.path.exists(final):
    with open(temporary, 'rb') as fresh, open(final, 'rb') as existing:
      same = fresh.read() == existing.read()
  if same:
    os.remove(temporary)
  else:
    os.replace(temporary, final)


def do_ar(args):
  """`rm -f out && ar rcs out @rsp`.

  The removal is not tidiness: `ar` updates an existing archive rather than
  replacing it, so an object that stops being part of a target would otherwise
  stay in the archive forever.
  """
  _unlink(args.output)
  directory = os.path.dirname(args.output)
  if directory:
    os.makedirs(directory, exist_ok=True)
  return _run([args.ar, 'rcs', args.output, '@' + args.rsp])


def _tail(rest):
  """Everything after `--`. argparse leaves the separator in; the linker
  would take it as an argument of its own."""
  return rest[1:] if rest and rest[0] == '--' else rest


def do_solink(args):
  """Link a shared library, write its TOC, and strip a copy of it.

  Three steps as one process. On a POSIX host these are three commands joined
  with `&&`; the ordering and the conditions are the same.
  """
  code = _run([args.ld] + _tail(args.rest))
  if code != 0:
    return code
  code = do_toc(args)
  if code != 0:
    return code
  if args.strip:
    return do_strip_from(args.strip, args.sofile, args.stripped)
  return 0


def do_link(args):
  """Link an executable, and strip a copy of it."""
  code = _run([args.ld] + _tail(args.rest))
  if code != 0:
    return code
  if args.strip:
    return do_strip_from(args.strip, args.source, args.stripped)
  return 0


def do_toc(args):
  """The shared library's table of contents, as solink builds it.

  A .so's dependents only need relinking when its *interface* changes, so the
  toolchain summarises the interface -- soname plus exported symbols -- and
  makes that summary, not the library, the dependency. Writing it only when it
  differs is what keeps a private change from relinking the world.
  """
  soname = subprocess.run([args.readelf, '-d', args.sofile],
                          capture_output=True,
                          text=True,
                          check=True).stdout
  lines = [line for line in soname.splitlines() if 'SONAME' in line]

  symbols = subprocess.run([args.nm, '-gD', '-f', 'posix', args.sofile],
                           capture_output=True,
                           text=True,
                           check=True).stdout
  for line in symbols.splitlines():
    # `cut -f1-2 -d' '`: the symbol name and its type, without the address,
    # which changes on every link and would defeat the whole point.
    lines.append(' '.join(line.split(' ')[:2]))

  temporary = args.toc + '.tmp'
  with open(temporary, 'w', encoding='utf-8', newline='\n') as handle:
    handle.write('\n'.join(lines) + '\n')
  _replace_if_different(temporary, args.toc)
  return 0


def do_strip_from(strip, source, output):
  """`strip --strip-unneeded -o output source`, into a directory that exists.

  Also the `cmp`/`mv` dance for the same reason as the TOC: a stripped library
  that has not changed should not look new to whatever copies it into a package.
  """
  directory = os.path.dirname(output)
  if directory:
    os.makedirs(directory, exist_ok=True)
  temporary = output + '.tmp'
  code = _run([strip, '--strip-unneeded', '-o', temporary, source])
  if code != 0:
    _unlink(temporary)
    return code
  _replace_if_different(temporary, output)
  return 0


def do_strip(args):
  return do_strip_from(args.strip, args.source, args.output)


def do_touch(args):
  """`touch`, for the stamp tool."""
  directory = os.path.dirname(args.file)
  if directory:
    os.makedirs(directory, exist_ok=True)
  with open(args.file, 'a', encoding='utf-8'):
    pass
  os.utime(args.file, None)
  return 0


def do_copy(args):
  """`ln -f src dst || (rm -rf dst && cp -af src dst)`.

  Windows has hard links, but only on the same volume and only for files, so
  this goes straight to copying: correctness first, and the build's copies are
  few.
  """
  _unlink(args.destination)
  directory = os.path.dirname(args.destination)
  if directory:
    os.makedirs(directory, exist_ok=True)
  if os.path.isdir(args.source):
    shutil.rmtree(args.destination, ignore_errors=True)
    shutil.copytree(args.source, args.destination)
  else:
    shutil.copyfile(args.source, args.destination)
    shutil.copymode(args.source, args.destination)
  return 0


def main(argv):
  parser = argparse.ArgumentParser(description=__doc__)
  commands = parser.add_subparsers(dest='command', required=True)

  archive = commands.add_parser('ar')
  archive.add_argument('--ar', required=True)
  archive.add_argument('--output', required=True)
  archive.add_argument('--rsp', required=True)
  archive.set_defaults(handler=do_ar)

  # `rest` is everything after `--`: the linker's own command line, which is
  # passed through untouched rather than reassembled here.
  solink = commands.add_parser('solink')
  solink.add_argument('--ld', required=True)
  solink.add_argument('--sofile', required=True)
  solink.add_argument('--toc', required=True)
  solink.add_argument('--readelf', required=True)
  solink.add_argument('--nm', required=True)
  solink.add_argument('--strip')
  solink.add_argument('--stripped')
  solink.add_argument('rest', nargs=argparse.REMAINDER)
  solink.set_defaults(handler=do_solink)

  link = commands.add_parser('link')
  link.add_argument('--ld', required=True)
  link.add_argument('--source')
  link.add_argument('--strip')
  link.add_argument('--stripped')
  link.add_argument('rest', nargs=argparse.REMAINDER)
  link.set_defaults(handler=do_link)

  toc = commands.add_parser('toc')
  toc.add_argument('--readelf', required=True)
  toc.add_argument('--nm', required=True)
  toc.add_argument('--sofile', required=True)
  toc.add_argument('--toc', required=True)
  toc.set_defaults(handler=do_toc)

  strip = commands.add_parser('strip')
  strip.add_argument('--strip', required=True)
  strip.add_argument('--source', required=True)
  strip.add_argument('--output', required=True)
  strip.set_defaults(handler=do_strip)

  touch = commands.add_parser('touch')
  touch.add_argument('file')
  touch.set_defaults(handler=do_touch)

  copy = commands.add_parser('copy')
  copy.add_argument('source')
  copy.add_argument('destination')
  copy.set_defaults(handler=do_copy)

  args = parser.parse_args(argv)
  return args.handler(args)


if __name__ == '__main__':
  sys.exit(main(sys.argv[1:]))
