#!/usr/bin/env python3
"""Packages the release Gallery into a folder anyone can run.

The application is a single native binary. It needs exactly one file beside
it -- `icudtl.dat`, which the engine's text stack reads for Unicode data --
because everything else it draws is compiled in: the fonts, the icons, the
study artwork, the product photographs. There is no asset bundle, no
`flutter_assets` directory, and no Dart snapshot.
"""

import argparse
import os
import shutil
import zipfile

DEFAULT_OUT = 'K:/rustflutter/src/out/host_release'
DEFAULT_DEST = 'K:/rustflutter/dist/rustflutter-gallery'

README = """rustflutter Gallery
===================

Flutter Gallery, with the Dart framework replaced by one written in Rust.
The engine underneath is Flutter's own -- the same layout, rasteriser and
compositor -- driven from Rust instead of from a Dart isolate. Rendering is
Impeller, through ANGLE.

To run it
---------

    flutter_gallery.exe

Windows may warn that the publisher is unknown; the binary is unsigned.

What you can do
---------------

    scroll          drag anywhere, or use the wheel
    the carousel    drag it sideways
    a category      tap its header to open or close it
    a demo          tap a row
    the theme       the gear, top right

Command line
------------

    --light                     start in the light theme
    --png <path>                render one screen to a PNG and exit
    --route <home|demo|study|settings>
    --slug <name>               which demo or study, with --route demo|study

Files
-----

    flutter_gallery.exe   the whole application
    icudtl.dat            Unicode data for the text stack

Everything else it draws is inside the executable.
"""


def main():
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument('--out', default=DEFAULT_OUT, help='build output directory')
  parser.add_argument('--dest', default=DEFAULT_DEST, help='folder to produce')
  parser.add_argument('--zip', action='store_true', help='also write a zip beside it')
  args = parser.parse_args()

  required = ['flutter_gallery.exe', 'icudtl.dat']
  # ANGLE compiles its shaders through this. Windows ships a copy, so it is
  # taken along only if the build produced one.
  optional = ['d3dcompiler_47.dll']

  missing = [n for n in required if not os.path.isfile(os.path.join(args.out, n))]
  if missing:
    raise SystemExit('not in %s: %s' % (args.out, ', '.join(missing)))

  if os.path.isdir(args.dest):
    shutil.rmtree(args.dest)
  os.makedirs(args.dest)

  copied = []
  for name in required + optional:
    source = os.path.join(args.out, name)
    if not os.path.isfile(source):
      continue
    shutil.copy2(source, os.path.join(args.dest, name))
    copied.append((name, os.path.getsize(source)))

  with open(os.path.join(args.dest, 'README.txt'), 'w', newline='\r\n') as f:
    f.write(README)

  total = 0
  for name, size in copied:
    print('  %-24s %8.1f MB' % (name, size / 1e6))
    total += size
  print('  %-24s %8.1f MB' % ('total', total / 1e6))

  if args.zip:
    archive = args.dest + '.zip'
    if os.path.isfile(archive):
      os.remove(archive)
    root = os.path.basename(args.dest)
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as z:
      for name in sorted(os.listdir(args.dest)):
        z.write(os.path.join(args.dest, name), os.path.join(root, name))
    print('\nzip: %s (%.1f MB)' % (archive, os.path.getsize(archive) / 1e6))

  print('\nfolder: %s' % args.dest)


if __name__ == '__main__':
  main()
