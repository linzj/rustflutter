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
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MAKE_APK = os.path.join(HERE, 'make_apk.py')
JAVA = os.path.join(HERE, os.pardir, 'android')

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

  icu = os.path.join(args.out, 'icudtl.dat')
  if not os.path.exists(icu):
    raise SystemExit('No icudtl.dat in %s.' % args.out)

  names = sorted(name[3:-3] for name in os.listdir(stripped)
                 if name.startswith('lib') and name.endswith('.so'))
  if args.only:
    names = [name for name in names if name in args.only]
  if not names:
    raise SystemExit('Nothing to package.')

  failures = []
  for name in names:
    output = os.path.join(args.out, 'apk', name + '.apk')
    command = [
        sys.executable, MAKE_APK,
        '--name', name,
        '--library', os.path.join(stripped, 'lib%s.so' % name),
        '--java', JAVA,
        '--icu', icu,
        '--output', output,
        '--sdk', args.sdk,
        '--java-home', args.java_home,
        '--abi', abi_for(args.out),
    ]
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
