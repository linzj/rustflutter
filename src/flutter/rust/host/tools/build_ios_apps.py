#!/usr/bin/env python3
# Copyright 2013 The Flutter Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Builds a .app bundle for every rustflutter example in an iOS output
directory, and optionally installs and launches one on a booted simulator.

    python flutter/rust/host/tools/build_ios_apps.py --out out/ios_debug_sim_unopt_arm64
    python flutter/rust/host/tools/build_ios_apps.py --out out/ios_debug_sim_unopt_arm64 \
        --run flutter_gallery [-- --route demo --slug text-field]

An iOS application is a directory: the executable, an Info.plist naming it,
and whatever it reads beside itself -- for these applications that is
icudtl.dat and nothing else, the assets being compiled in. There is no Xcode
project because there is nothing for one to do: the executables come out of
ninja, and assembling three files into a directory is this script.

The bundles are ad-hoc signed, which is all a simulator checks. A device
build needs a real identity and a provisioning profile, and is not this
script's business yet.
"""

import argparse
import os
import plistlib
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

# The data upstream ships, trimmed to what Flutter needs -- the same file the
# APK packer bundles.
ICU_DATA = os.path.join(HERE, os.pardir, os.pardir, os.pardir,
                        'third_party', 'icu', 'flutter', 'icudtl.dat')

# Matches darwin_sdk.gni's ios_deployment_target.
MINIMUM_OS = '15.0'

BUNDLE_ID_PREFIX = 'dev.rustflutter'


def find_executables(out_directory):
  """Mach-O executables at the top of the out directory.

  Ninja writes every example's binary there; everything else at that level is
  build machinery (ninja files, gn artifacts, subdirectories).
  """
  found = []
  for entry in sorted(os.listdir(out_directory)):
    path = os.path.join(out_directory, entry)
    if not os.path.isfile(path) or not os.access(path, os.X_OK):
      continue
    if '.' in entry:  # args.gn, build.ninja, *.json, *.xcodeproj...
      continue
    with open(path, 'rb') as f:
      magic = f.read(4)
    # MH_MAGIC_64, either endianness spelling.
    if magic in (b'\xcf\xfa\xed\xfe', b'\xfe\xed\xfa\xcf'):
      found.append(entry)
  return found


def info_plist(name):
  return {
      'CFBundleDevelopmentRegion': 'en',
      'CFBundleExecutable': name,
      'CFBundleIdentifier': f'{BUNDLE_ID_PREFIX}.{name}'.replace('_', '-'),
      'CFBundleInfoDictionaryVersion': '6.0',
      'CFBundleName': name,
      'CFBundlePackageType': 'APPL',
      'CFBundleShortVersionString': '1.0',
      'CFBundleVersion': '1',
      'MinimumOSVersion': MINIMUM_OS,
      'UIDeviceFamily': [1, 2],
      # An empty launch-screen dictionary is what says "no storyboard, and no
      # letterboxing either": without the key UIKit sizes the window for a
      # decade-old phone.
      'UILaunchScreen': {},
      'UIRequiredDeviceCapabilities': ['arm64'],
      'UISupportedInterfaceOrientations': [
          'UIInterfaceOrientationPortrait',
          'UIInterfaceOrientationLandscapeLeft',
          'UIInterfaceOrientationLandscapeRight',
      ],
  }


def make_app(out_directory, name):
  """Assembles Payload-less Foo.app beside the executable and signs it."""
  app_dir = os.path.join(out_directory, 'app', f'{name}.app')
  if os.path.isdir(app_dir):
    shutil.rmtree(app_dir)
  os.makedirs(app_dir)

  shutil.copy2(os.path.join(out_directory, name), os.path.join(app_dir, name))
  with open(os.path.join(app_dir, 'Info.plist'), 'wb') as f:
    plistlib.dump(info_plist(name), f)
  shutil.copy2(ICU_DATA, os.path.join(app_dir, 'icudtl.dat'))

  # Ad-hoc. The linker already signed the binary; this re-signs the whole
  # bundle so the seal covers the plist and the ICU data too.
  subprocess.run(['codesign', '--force', '--sign', '-', app_dir], check=True)
  return app_dir


def booted_device():
  """The udid of a booted simulator, booting the first available iPhone if
  none is."""
  out = subprocess.run(['xcrun', 'simctl', 'list', 'devices', 'booted', '-j'],
                       check=True, capture_output=True, text=True).stdout
  import json
  devices = json.loads(out)['devices']
  for runtime in devices.values():
    for device in runtime:
      if device.get('state') == 'Booted':
        return device['udid']
  # Nothing booted: pick the first available iPhone.
  out = subprocess.run(['xcrun', 'simctl', 'list', 'devices', 'available', '-j'],
                       check=True, capture_output=True, text=True).stdout
  devices = json.loads(out)['devices']
  for runtime in sorted(devices.keys(), reverse=True):
    for device in devices[runtime]:
      if 'iPhone' in device.get('name', ''):
        subprocess.run(['xcrun', 'simctl', 'boot', device['udid']], check=True)
        return device['udid']
  raise RuntimeError('no available iPhone simulator')


def run_app(app_dir, name, extra_args):
  udid = booted_device()
  subprocess.run(['xcrun', 'simctl', 'install', udid, app_dir], check=True)
  bundle_id = info_plist(name)['CFBundleIdentifier']
  command = ['xcrun', 'simctl', 'launch', '--terminate-running-process',
             udid, bundle_id]
  command += extra_args
  subprocess.run(command, check=True)
  print(f'launched {bundle_id} on {udid}')


def main():
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument('--out', required=True, help='the iOS output directory')
  parser.add_argument('--only', help='package just this example')
  parser.add_argument('--run', help='install and launch this example on a '
                      'booted simulator after packaging')
  parser.add_argument('launch_args', nargs='*', default=[],
                      help='arguments after -- go to the launched app')
  args = parser.parse_args()

  out_directory = os.path.abspath(args.out)
  if not os.path.isdir(out_directory):
    parser.error(f'{out_directory} is not a directory')

  names = find_executables(out_directory)
  if args.only:
    names = [name for name in names if name == args.only]
  if args.run and args.run not in names:
    parser.error(f'{args.run} is not among the executables: {names}')
  if not names:
    parser.error('no executables found; build first')

  apps = {}
  for name in names:
    apps[name] = make_app(out_directory, name)
    print(f'made {os.path.relpath(apps[name], out_directory)}')

  if args.run:
    run_app(apps[args.run], args.run, args.launch_args)

  return 0


if __name__ == '__main__':
  sys.exit(main())
