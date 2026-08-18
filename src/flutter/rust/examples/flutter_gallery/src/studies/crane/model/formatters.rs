// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/model/formatters.dart` (flutter/gallery @
//! d12640d): `formattedDuration`.
//!
//! Divergence: the generated English table's `crane_hours`/`crane_minutes`
//! (`src/l10n/gallery_localizations_en.rs`) carry a generator bug -- the
//! `${hours}` placeholders were escaped into literals, so they return
//! "${hours}h" rather than "6h". That file is generated and not this
//! batch's to edit, so the short forms are computed here with the same
//! English rule upstream declares (`one: '1h', other: '{hours}h'`, which
//! render identically) and only the join goes through the catalogue. The
//! `abbreviated` flag is accepted and ignored exactly as upstream ignores
//! it -- both call sites of the flag resolve to the same short form at
//! d12640d.

use std::time::Duration;

use crate::l10n::gallery_localizations::GalleryLocalizations;

/// Upstream's `craneHours`: `{hours}h`.
fn hours_short_form(hours: i64) -> String {
    format!("{hours}h")
}

/// Upstream's `craneMinutes`: `{minutes}m`.
fn minutes_short_form(minutes: i64) -> String {
    format!("{minutes}m")
}

/// Duration of time (e.g. `6h 15m`).
pub fn formatted_duration(duration: Duration, abbreviated: bool) -> String {
    // Upstream takes `{bool? abbreviated}` and never reads it.
    let _ = abbreviated;
    let localizations = GalleryLocalizations::en();
    let hours = hours_short_form(duration.as_secs() as i64 / 3600);
    let minutes = minutes_short_form((duration.as_secs() as i64 / 60) % 60);
    localizations.crane_flight_duration(hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_hours_and_minutes() {
        assert_eq!(
            formatted_duration(Duration::from_secs(6 * 3600 + 15 * 60), true),
            "6h 15m"
        );
        assert_eq!(
            formatted_duration(Duration::from_secs(19 * 3600 + 40 * 60), false),
            "19h 40m"
        );
    }

    #[test]
    fn drops_the_hours_carry_into_minutes() {
        // `duration.inMinutes % 60`: the minutes form is the remainder.
        assert_eq!(
            formatted_duration(Duration::from_secs(61 * 60), true),
            "1h 1m"
        );
    }
}
