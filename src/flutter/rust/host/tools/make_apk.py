#!/usr/bin/env python3
# Copyright 2013 The Flutter Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Packages one rustflutter application as an Android APK.

There is no Gradle here, and that is deliberate. Gradle exists to build Java
projects with transitive dependencies; this has one .java file, one .so and one
asset, and every step below is a single command. What Gradle would add is a
second build system, a second dependency graph and a second toolchain download,
none of which would be describing anything the engine's own build does not
already know.

The steps, in order:

    aapt2 link   the manifest and nothing else, producing an APK that holds a
                 compiled AndroidManifest.xml and an empty resource table
    javac        RustflutterActivity.java against android.jar
    d8           those classes into classes.dex
    zip          the dex, the native library and the assets into the APK
    zipalign     so the loader can mmap what it needs to
    apksigner    with a debug key, because a device will not install an
                 unsigned package

Run with --help for the arguments.
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile

MANIFEST = '''<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{package}"
    android:versionCode="1"
    android:versionName="1.0">

  <uses-sdk android:minSdkVersion="{min_sdk}" android:targetSdkVersion="{target_sdk}" />

  <application
      android:label="{label}"
      android:icon="@android:drawable/sym_def_app_icon"
      android:extractNativeLibs="true"
      android:hardwareAccelerated="true">
    <activity
        android:name="io.flutter.rustflutter.RustflutterActivity"
        android:exported="true"
        android:launchMode="singleTop"
        android:hardwareAccelerated="true"
        android:windowSoftInputMode="adjustResize|stateHidden"
        android:theme="@android:style/Theme.Black.NoTitleBar"
        android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|navigation|uiMode|density|locale|layoutDirection|fontScale">
      <meta-data android:name="rustflutter.library" android:value="{library}" />
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
  </application>
</manifest>
'''


def run(command, cwd=None):
  """Runs a command, showing what failed rather than a traceback."""
  result = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
  if result.returncode != 0:
    sys.stderr.write('FAILED: %s\n' % ' '.join(str(part) for part in command))
    sys.stderr.write(result.stdout)
    sys.stderr.write(result.stderr)
    raise SystemExit(result.returncode)
  return result.stdout


def batch(sdk_tool):
  """Windows ships these as .bat wrappers; everywhere else they are scripts."""
  if sys.platform == 'win32' and os.path.exists(sdk_tool + '.bat'):
    return sdk_tool + '.bat'
  return sdk_tool


def executable(path):
  return path + '.exe' if sys.platform == 'win32' else path


def debug_keystore(java_home, work):
  """The debug key, made once and kept.

  Android refuses to install an unsigned package, so every APK needs a
  signature; a debug key is the same thing `flutter run` uses and means nothing
  beyond "this was built locally".
  """
  keystore = os.path.join(
      os.path.expanduser('~'), '.rustflutter', 'debug.keystore')
  if os.path.exists(keystore):
    return keystore
  os.makedirs(os.path.dirname(keystore), exist_ok=True)
  run([
      executable(os.path.join(java_home, 'bin', 'keytool')),
      '-genkeypair',
      '-keystore', keystore,
      '-storepass', 'android',
      '-keypass', 'android',
      '-alias', 'rustflutter',
      '-keyalg', 'RSA',
      '-keysize', '2048',
      '-validity', '10000',
      '-dname', 'CN=rustflutter, OU=rustflutter, O=rustflutter, C=US',
  ])
  return keystore


def main(argv):
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument('--name', required=True,
                      help='the application, e.g. "counter"')
  parser.add_argument('--library', required=True,
                      help='path to lib<name>.so')
  parser.add_argument('--java', required=True,
                      help='directory holding the io/flutter/rustflutter tree')
  parser.add_argument('--icu', required=True, help='path to icudtl.dat')
  parser.add_argument('--output', required=True, help='the APK to write')
  parser.add_argument('--sdk', required=True, help='the Android SDK root')
  parser.add_argument('--java-home', required=True, help='a JDK 17 or newer')
  parser.add_argument('--build-tools', default='36.0.0')
  parser.add_argument('--platform', default='android-36.1')
  parser.add_argument('--abi', default='arm64-v8a')
  parser.add_argument('--min-sdk', default='24')
  parser.add_argument('--target-sdk', default='34')
  parser.add_argument('--package', default=None,
                      help='the application id; defaults to '
                           'io.flutter.rustflutter.<name>')
  parser.add_argument('--label', default=None,
                      help='what the launcher shows; defaults to the name')
  args = parser.parse_args(argv)

  # d8 and apksigner are launcher scripts that look for a JDK themselves, and
  # they look in JAVA_HOME. Setting it here means the caller only has to say
  # where the JDK is once.
  os.environ['JAVA_HOME'] = args.java_home

  package = args.package or ('io.flutter.rustflutter.' + args.name)
  label = args.label or args.name
  build_tools = os.path.join(args.sdk, 'build-tools', args.build_tools)
  android_jar = os.path.join(args.sdk, 'platforms', args.platform, 'android.jar')
  if not os.path.exists(android_jar):
    raise SystemExit('No android.jar at %s' % android_jar)

  library_name = os.path.basename(args.library)
  if not library_name.startswith('lib') or not library_name.endswith('.so'):
    raise SystemExit('Expected lib<name>.so, got %s' % library_name)
  # What System.loadLibrary is given, which is the name without lib/.so.
  library = library_name[3:-3]

  work = tempfile.mkdtemp(prefix='rustflutter-apk-')
  try:
    manifest_path = os.path.join(work, 'AndroidManifest.xml')
    with open(manifest_path, 'w', encoding='utf-8') as handle:
      handle.write(MANIFEST.format(package=package, label=label,
                                   library=library, min_sdk=args.min_sdk,
                                   target_sdk=args.target_sdk))

    unsigned = os.path.join(work, 'unsigned.apk')
    run([
        executable(os.path.join(build_tools, 'aapt2')), 'link',
        '-I', android_jar,
        '--manifest', manifest_path,
        '--min-sdk-version', args.min_sdk,
        '--target-sdk-version', args.target_sdk,
        '-o', unsigned,
    ])

    # Java. `--release 11` because d8 reads class files, not source, and its
    # ceiling is lower than the JDK that is doing the compiling.
    classes = os.path.join(work, 'classes')
    os.makedirs(classes)
    sources = []
    for root, _, names in os.walk(args.java):
      sources.extend(os.path.join(root, name) for name in names
                     if name.endswith('.java'))
    if not sources:
      raise SystemExit('No .java under %s' % args.java)
    run([
        executable(os.path.join(args.java_home, 'bin', 'javac')),
        '--release', '11',
        '-Xlint:-options',
        '-classpath', android_jar,
        '-d', classes,
    ] + sources)

    run([
        batch(os.path.join(args.sdk, 'cmdline-tools', 'latest', 'bin', 'd8')),
        '--lib', android_jar,
        '--min-api', args.min_sdk,
        '--output', work,
    ] + [os.path.join(root, name)
         for root, _, names in os.walk(classes)
         for name in names if name.endswith('.class')])

    # Everything aapt2 did not put in, added to the archive it produced.
    with zipfile.ZipFile(unsigned, 'a', zipfile.ZIP_DEFLATED) as apk:
      apk.write(os.path.join(work, 'classes.dex'), 'classes.dex')
      apk.write(args.library, 'lib/%s/%s' % (args.abi, library_name))
      apk.write(args.icu, 'assets/icudtl.dat')

    aligned = os.path.join(work, 'aligned.apk')
    run([
        executable(os.path.join(build_tools, 'zipalign')),
        '-f', '-p', '4', unsigned, aligned,
    ])

    keystore = debug_keystore(args.java_home, work)
    os.makedirs(os.path.dirname(os.path.abspath(args.output)), exist_ok=True)
    run([
        batch(os.path.join(build_tools, 'apksigner')), 'sign',
        '--ks', keystore,
        '--ks-pass', 'pass:android',
        '--key-pass', 'pass:android',
        '--ks-key-alias', 'rustflutter',
        '--min-sdk-version', args.min_sdk,
        '--out', args.output,
        aligned,
    ])
    print(args.output)
  finally:
    shutil.rmtree(work, ignore_errors=True)


if __name__ == '__main__':
  main(sys.argv[1:])
