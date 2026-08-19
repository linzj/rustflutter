"""Generates the AutofillHints constant table from upstream `autofill.dart`.

Sixty-seven names, each of which has to match the platform's string exactly:
a typo is a field the operating system silently declines to fill. Parsed
rather than transcribed, for that reason.
"""
import io
import re
import sys

src = io.open(sys.argv[1], encoding='utf-8').read()
out_path = sys.argv[2]

body = src[src.index('abstract final class AutofillHints'):]
body = body[:body.index('\n}')]

rows = []
for match in re.finditer(
        r"(?:///.*\n(?:\s*///.*\n)*)?\s*static const String (\w+) = '([^']*)';", body):
    rows.append((match.group(1), match.group(2)))

assert len(rows) == 67, len(rows)


def screaming(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).upper()


w = []
add = w.append
add('''// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Telling the operating system what a field is for (upstream
//! `services/autofill.dart`).
//!
//! An operating system that remembers an address can only offer it to a field
//! that says it wants an address. That is the whole of autofill: a field
//! names what it holds, gives itself an identifier stable across restarts,
//! and the platform does the rest.
//!
//! # Recorded divergences
//!
//! * Upstream's `AutofillScopeMixin.attach` wraps the triggering field's
//!   configuration in a private `_AutofillScopeTextInputConfiguration` that
//!   adds a `fields` list. That private class is not a public class of
//!   upstream's; the wrapping is
//!   [`AutofillScope::configuration_with_fields`] here, which is the same
//!   JSON by another route.
//! * `AutofillClient` and `AutofillScope` are traits rather than abstract
//!   classes, and `AutofillScopeMixin` is a blanket implementation of the
//!   part upstream puts in the mixin -- which is what a Dart mixin over an
//!   interface is.

use crate::services::codec::Value;
use crate::services::text_input::{TextEditingValue, TextInputConfiguration};

/// Upstream `AutofillHints`: the names a field can give for what it holds.
///
/// Every one of these is a string the platform matches on, so a typo is a
/// field the operating system silently declines to fill -- which is why the
/// table is generated from upstream rather than typed out.
pub struct AutofillHints;

impl AutofillHints {''')

for name, value in rows:
    add('    pub const %s: &\'static str = "%s";' % (screaming(name), value))

add('')
add('    /// Every hint upstream defines, for a caller that wants to check one')
add('    /// it was handed.')
add('    pub const ALL: [&\'static str; %d] = [' % len(rows))
for name, _ in rows:
    add('        AutofillHints::%s,' % screaming(name))
add('    ];')
add('}')

io.open(out_path, 'w', encoding='utf-8', newline='').write('\n'.join(w) + '\n')
print('wrote', out_path, len(rows), 'hints')
