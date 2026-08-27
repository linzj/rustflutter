#!/usr/bin/env python3
# Copyright 2013 The Flutter Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Builds an APK for every rustflutter example in an Android output directory.

    python flutter/rust/host/tools/build_apks.py --out out/android_release_arm64

Everything it needs beyond that it works out: the abi from the directory name,
the applications from the .so files ninja produced, and the SDK and JDK from the
usual environment variables. See make_apk.py for what one APK is made of.
"""

import argparse
import os
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MAKE_APK = os.path.join(HERE, 'make_apk.py')
JAVA = os.path.join(HERE, os.pardir, 'android')

# The data upstream ships, trimmed to what Flutter needs (see the README
# beside it) -- not the full Chromium bundle the build copies to the out dir.
ICU_DATA = os.path.join(HERE, os.pardir, os.pardir, os.pardir,
                        'third_party', 'icu', 'flutter', 'icudtl.dat')

# The engine, when it was built as a library of its own. It is in lib.stripped
# beside the applications and is not one, so it is skipped when they are
# enumerated and packaged with whichever of them asked for it.
ENGINE = 'rustflutter_engine'

# Which ABI an output directory holds, by the suffix the gn wrapper gives it.
ABIS = {
    'arm64': 'arm64-v8a',
    'x64': 'x86_64',
    'x86': 'x86',
    'arm': 'armeabi-v7a',
}


def abi_for(out_directory):
  name = os.path.basename(os.path.normpath(out_directory))
  for suffix, abi in ABIS.items():
    if name.endswith('_' + suffix):
      return abi
  # `android_release` with no suffix is the gn wrapper's name for arm.
  return 'armeabi-v7a'


def needed_libraries(path):
  """The DT_NEEDED entries of an ELF shared library.

  Which is how an application linked against the shared engine is told from one
  that folded the archive in: the first names librustflutter_engine.so and the
  second names nothing of the sort. There is no flag to consult because there is
  no flag -- the link mode is a property of the .so ninja produced, and this
  reads it off rather than being told.

  Returns an empty list for anything it cannot parse, which packages the
  application on its own: the same thing that happened before there was a shared
  engine to find.
  """
  try:
    with open(path, 'rb') as handle:
      data = handle.read()
  except OSError:
    return []
  if data[:4] != b'\x7fELF':
    return []

  wide = data[4] == 2
  order = '<' if data[5] == 1 else '>'
  size = 8 if wide else 4

  def word(offset, width):
    fmt = {2: 'H', 4: 'I', 8: 'Q'}[width]
    try:
      return struct.unpack_from(order + fmt, data, offset)[0]
    except struct.error:
      return None

  # The program header table, from the ELF header. Its field offsets are the
  # only thing that moves between the two classes, and everything below is read
  # through these three.
  ph_offset = word(32 if wide else 28, size)
  ph_size = word(54 if wide else 42, 2)
  ph_count = word(56 if wide else 44, 2)
  if ph_offset is None or ph_size is None or ph_count is None:
    return []

  # Program header fields, by class: p_offset, p_vaddr, p_filesz.
  p_offset_at, p_vaddr_at, p_filesz_at = (8, 16, 32) if wide else (4, 8, 16)

  def segments(kind):
    for index in range(ph_count):
      header = ph_offset + index * ph_size
      if word(header, 4) == kind:
        yield header

  # PT_DYNAMIC holds the dynamic array: DT_NEEDED and the string table its
  # entries index into.
  dynamic = next((word(header + p_offset_at, size) for header in segments(2)),
                 None)
  if dynamic is None:
    return []

  entries = []
  cursor = dynamic
  while cursor + 2 * size <= len(data):
    tag, value = word(cursor, size), word(cursor + size, size)
    if tag is None or tag == 0:  # DT_NULL ends it.
      break
    entries.append((tag, value))
    cursor += 2 * size

  # DT_STRTAB is a virtual address, so it has to be mapped back to a file
  # offset through whichever PT_LOAD segment covers it.
  strtab = next((value for tag, value in entries if tag == 5), None)
  if strtab is None:
    return []
  offset = None
  for header in segments(1):  # PT_LOAD
    vaddr = word(header + p_vaddr_at, size)
    length = word(header + p_filesz_at, size)
    if vaddr is not None and length is not None and vaddr <= strtab < vaddr + length:
      offset = word(header + p_offset_at, size) + (strtab - vaddr)
      break
  if offset is None:
    return []

  names = []
  for tag, value in entries:
    if tag != 1:  # DT_NEEDED
      continue
    end = data.find(b'\x00', offset + value)
    if end >= 0:
      names.append(data[offset + value:end].decode('utf-8', 'replace'))
  return names


def main(argv):
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument('--out', required=True,
                      help='an Android output directory ninja has built')
  parser.add_argument('--sdk',
                      default=os.environ.get('ANDROID_SDK_ROOT')
                      or os.environ.get('ANDROID_HOME'))
  parser.add_argument('--java-home', default=os.environ.get('JAVA_HOME'))
  parser.add_argument('--only', nargs='*',
                      help='build only these applications')
  args = parser.parse_args(argv)

  if not args.sdk:
    raise SystemExit('No Android SDK: pass --sdk or set ANDROID_SDK_ROOT.')
  if not args.java_home:
    raise SystemExit('No JDK: pass --java-home or set JAVA_HOME.')

  # The stripped libraries, which are what goes in a package: the unstripped
  # ones are a hundred megabytes of debug information the device has no use for.
  stripped = os.path.join(args.out, 'lib.stripped')
  if not os.path.isdir(stripped):
    raise SystemExit('No lib.stripped in %s -- has ninja run?' % args.out)

  icu = ICU_DATA
  if not os.path.exists(icu):
    raise SystemExit('No icudtl.dat at %s.' % icu)

  names = sorted(name[3:-3] for name in os.listdir(stripped)
                 if name.startswith('lib') and name.endswith('.so')
                 and name[3:-3] != ENGINE)
  if args.only:
    names = [name for name in names if name in args.only]
  if not names:
    raise SystemExit('Nothing to package.')

  engine = os.path.join(stripped, 'lib%s.so' % ENGINE)

  failures = []
  for name in names:
    output = os.path.join(args.out, 'apk', name + '.apk')
    library = os.path.join(stripped, 'lib%s.so' % name)
    command = [
        sys.executable, MAKE_APK,
        '--name', name,
        '--library', library,
        '--java', JAVA,
        '--icu', icu,
        '--output', output,
        '--sdk', args.sdk,
        '--java-home', args.java_home,
        '--abi', abi_for(args.out),
    ]
    # Only for an application that asked for it. One that linked the archive
    # carries the engine already, and a second copy would be 15 MB of APK
    # nothing opens.
    if os.path.basename(engine) in needed_libraries(library):
      if not os.path.exists(engine):
        raise SystemExit(
            '%s needs %s, which is not in %s. Build '
            'flutter/rust:rustflutter_engine_shared.'
            % (name, os.path.basename(engine), stripped))
      command += ['--extra-library', engine]
    print('packaging %s' % name, flush=True)
    # Captured rather than inherited, so that a failure's reason appears next
    # to the name of what failed instead of interleaved with the next one.
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
      failures.append(name)
      sys.stderr.write(result.stdout)
      sys.stderr.write(result.stderr)

  if failures:
    print('failed: %s' % ', '.join(failures))
    return 1
  print('%d APKs in %s' % (len(names), os.path.join(args.out, 'apk')))
  return 0


if __name__ == '__main__':
  sys.exit(main(sys.argv[1:]))
