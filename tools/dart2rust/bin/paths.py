# -*- coding: utf-8 -*-
"""Where the SDK, the framework and the app being translated live.

Four scripts each had these written into them as Windows paths, which is
fine until the build moves -- and it is moving to WSL2. One place, three
environment variables, and the current values as defaults so nothing has to
be set to keep working as it did:

    RUSTFLUTTER_FLUTTER   a Flutter checkout (its bundled dart-sdk, and its
                          package_config.json, are what the front ends run on)
    RUSTFLUTTER_APP       the app being translated -- its package_config.json
                          is what the fixture comparison resolves against
    RUSTFLUTTER_ENGINE    an engine checkout with a built dart-sdk, read by
                          `dill.py` for `pkg/kernel` and the frontend server

`exe()` is the other half of the same move: the SDK's tools are `dart.exe` on
Windows and `dart` everywhere else, and four call sites said `.exe` outright.
"""
import os

FLUTTER = os.environ.get('RUSTFLUTTER_FLUTTER', 'E:/source/flutter')
APP = os.environ.get('RUSTFLUTTER_APP', 'D:/linzjUbuntu2204/gallery_upstream')
ENGINE = os.environ.get(
    'RUSTFLUTTER_ENGINE', os.path.join(FLUTTER, 'engine', 'src'))


def exe(name):
    """An SDK tool's file name on this platform."""
    return name + '.exe' if os.name == 'nt' else name


#: The `dart` the analyzer front end runs under.
FLUTTER_DART = os.path.join(
    FLUTTER, 'bin', 'cache', 'dart-sdk', 'bin', exe('dart'))

#: What resolves `package:flutter/...` for the analyzer front end.
FLUTTER_PKGS = '--packages=' + os.path.join(
    FLUTTER, '.dart_tool', 'package_config.json')

#: What resolves the app's own imports when a fixture is built into a dill.
APP_PACKAGES = os.path.join(APP, '.dart_tool', 'package_config.json')
