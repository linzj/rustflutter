"""Generates the SystemChannels name table from upstream `system_channels.dart`.

Twenty-four channel names, each of which has to match the engine's exactly:
a typo is a channel nobody is listening on, which fails by silence.
"""
import io
import re
import sys

src = io.open(sys.argv[1], encoding='utf-8').read()
out_path = sys.argv[2]

rows = []
for match in re.finditer(
        r"static const (\w+)(?:<[^>]*>)? (\w+) =\s*(\w+)(?:<[^>]*>)?\(\s*'([^']+)'",
        src):
    kind, name, _ctor, channel = match.groups()
    rows.append((name, channel, kind))

assert len(rows) == 24, len(rows)


def screaming(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).upper()


w = []
add = w.append
add('''// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The channels the engine listens on (upstream
//! `services/system_channels.dart`), and the three small services that are
//! nothing but a call on one of them (`services/scribe.dart`,
//! `services/sensitive_content.dart`, `services/deferred_component.dart`).
//!
//! # Recorded divergences
//!
//! * Upstream's `SystemChannels` holds built channel objects, each with its
//!   codec. Here it holds the names: a channel in this crate is cheap to
//!   build and the codec is chosen where the call is made, so a table of
//!   pre-built channels would be a table of things nobody could use without
//!   knowing which codec each carried anyway. The names are what has to be
//!   exact, and they are what is generated.
//! * Every call upstream is a `Future`; they are callbacks here, for the
//!   reason recorded on [`spell_check`](crate::services::spell_check).

use crate::services::channel::MethodChannel;
use crate::services::codec::{JsonMethodCodec, StandardMethodCodec, Value};

/// Upstream `SystemChannels`: the names the engine answers on.
///
/// Generated from upstream, because a typo in one of these is a channel
/// nobody is listening on -- nothing errors, the call simply never arrives.
pub struct SystemChannels;

impl SystemChannels {''')

for name, channel, kind in rows:
    add('    /// Upstream `SystemChannels.%s`, a `%s`.' % (name, kind))
    add('    pub const %s: &\'static str = "%s";' % (screaming(name), channel))

add('')
add('    /// Every channel upstream names.')
add('    pub const ALL: [&\'static str; %d] = [' % len(rows))
for name, _, _ in rows:
    add('        SystemChannels::%s,' % screaming(name))
add('    ];')
add('}')

io.open(out_path, 'w', encoding='utf-8', newline='').write('\n'.join(w) + '\n')
print('wrote', out_path, len(rows), 'channels')
