#!/usr/bin/env vpython3
# Copyright 2013 The Flutter Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Builds and runs a Rust crate's #[test] functions.

GN has no rust_test tool, and its rust_bin tool cannot pass --test, so the
crate's unit tests are built with a direct rustc invocation instead. The linker
is passed explicitly: rustc would otherwise pick whatever `link.exe` is first on
PATH, which under Git Bash is coreutils' `link`, not MSVC's linker.

A crate that depends on the framework needs it rebuilt rather than reused: the
rlib GN produces declares the engine's C ABI without defining it, and a test
binary is linked without the engine behind it. Passing --dep builds that
dependency here with `--cfg rustflutter_stubs`, which swaps in the inert
stand-ins from `engine_test_stubs.rs` and leaves nothing undefined.
"""

import argparse
import os
import subprocess
import sys


def main():
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument('--crate-root', required=True, help='crate entry point')
  parser.add_argument('--crate-name', required=True)
  parser.add_argument('--output', required=True, help='test binary to produce')
  parser.add_argument('--rustc', default='rustc')
  parser.add_argument('--edition', default='2024')
  parser.add_argument('--linker', default=None, help='explicit linker path')
  parser.add_argument('--stamp', required=True, help='written on success')
  parser.add_argument(
      '--dep',
      action='append',
      default=[],
      metavar='NAME=CRATE_ROOT',
      help='a dependency to build as a stubbed rlib and link against')
  args = parser.parse_args()

  out_dir = os.path.dirname(os.path.abspath(args.output))
  externs = []
  for dep in args.dep:
    name, _, root = dep.partition('=')
    if not root:
      parser.error('--dep wants NAME=CRATE_ROOT, got %r' % dep)
    rlib = os.path.join(out_dir, 'lib%s_stubbed.rlib' % name)
    command = [
        args.rustc,
        '--edition=%s' % args.edition,
        '--crate-type=rlib',
        '--crate-name',
        name,
        '--cfg',
        'rustflutter_stubs',
        root,
        '-o',
        rlib,
    ]
    build = subprocess.run(command, check=False)
    if build.returncode != 0:
      return build.returncode
    externs += ['--extern', '%s=%s' % (name, rlib)]

  command = [
      args.rustc,
      '--test',
      '--edition=%s' % args.edition,
      '--crate-name',
      args.crate_name,
      args.crate_root,
      '-o',
      args.output,
  ] + externs
  if args.linker:
    command += ['-C', 'linker=%s' % args.linker]

  build = subprocess.run(command, check=False)
  if build.returncode != 0:
    return build.returncode

  # The output path is relative to the build directory; CreateProcess will not
  # resolve a bare relative name, so make it absolute before running.
  run = subprocess.run([os.path.abspath(args.output)], check=False)
  if run.returncode != 0:
    return run.returncode

  os.makedirs(os.path.dirname(args.stamp), exist_ok=True)
  with open(args.stamp, 'w') as stamp:
    stamp.write('')
  return 0


if __name__ == '__main__':
  sys.exit(main())
