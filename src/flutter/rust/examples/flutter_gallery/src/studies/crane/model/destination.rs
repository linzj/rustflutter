// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/model/destination.dart` (flutter/gallery @
//! d12640d): the abstract `Destination` and its three implementations.
//!
//! The Dart abstract class is a trait here; each subclass is a struct whose
//! fields are upstream's constructor arguments, plus the photograph itself
//! (`photo`) baked in with `include_bytes!` -- there is no asset bundle to
//! resolve `assetName` against, so the name is kept for provenance and the
//! bytes are what is read.
//!
//! Divergence: upstream's `FlyDestination.subtitle` orders the stops and
//! duration by the resolved text direction. The catalogue is English-only
//! (PORTING.md), so the LTR branch is taken unconditionally.

use std::time::Duration;

use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::formatters::formatted_duration;

/// Upstream's abstract `Destination`.
pub trait Destination {
    /// Upstream's `id`.
    fn id(&self) -> u32;
    /// Upstream's `destination`.
    fn name(&self) -> &'static str;
    /// Upstream's `assetSemanticLabel`.
    fn asset_semantic_label(&self) -> &'static str;
    /// Upstream's `imageAspectRatio`: width over height.
    fn image_aspect_ratio(&self) -> f32;
    /// Upstream's `assetName`, the path inside `flutter_gallery_assets`.
    /// Provenance only -- the bytes are what [`Destination::photo`] returns.
    fn asset_name(&self) -> String;
    /// The photograph, decoded once and cached by the card that draws it.
    fn photo(&self) -> &'static [u8];
    /// Upstream's `subtitle(BuildContext)`.
    fn subtitle(&self) -> String;
    /// Upstream's `subtitleSemantics(BuildContext)`.
    fn subtitle_semantics(&self) -> String {
        self.subtitle()
    }
}

/// Upstream's `FlyDestination`.
pub struct FlyDestination {
    pub id: u32,
    pub destination: &'static str,
    pub stops: u32,
    pub duration: Option<Duration>,
    pub asset_semantic_label: &'static str,
    pub image_aspect_ratio: f32,
    pub photo: &'static [u8],
}

impl Destination for FlyDestination {
    fn id(&self) -> u32 {
        self.id
    }
    fn name(&self) -> &'static str {
        self.destination
    }
    fn asset_semantic_label(&self) -> &'static str {
        self.asset_semantic_label
    }
    fn image_aspect_ratio(&self) -> f32 {
        self.image_aspect_ratio
    }
    fn asset_name(&self) -> String {
        format!("crane/destinations/fly_{}.jpg", self.id)
    }
    fn photo(&self) -> &'static [u8] {
        self.photo
    }

    fn subtitle(&self) -> String {
        let localizations = GalleryLocalizations::en();
        let stops_text = localizations.crane_fly_stops(self.stops as i64);
        match self.duration {
            None => stops_text,
            // LTR order, unconditionally -- see the module header.
            Some(duration) => {
                format!("{stops_text} · {}", formatted_duration(duration, true))
            }
        }
    }

    fn subtitle_semantics(&self) -> String {
        let localizations = GalleryLocalizations::en();
        let stops_text = localizations.crane_fly_stops(self.stops as i64);
        match self.duration {
            None => stops_text,
            Some(duration) => {
                format!("{stops_text}, {}", formatted_duration(duration, false))
            }
        }
    }
}

/// Upstream's `SleepDestination`.
pub struct SleepDestination {
    pub id: u32,
    pub destination: &'static str,
    pub total: u32,
    pub asset_semantic_label: &'static str,
    pub image_aspect_ratio: f32,
    pub photo: &'static [u8],
}

impl Destination for SleepDestination {
    fn id(&self) -> u32 {
        self.id
    }
    fn name(&self) -> &'static str {
        self.destination
    }
    fn asset_semantic_label(&self) -> &'static str {
        self.asset_semantic_label
    }
    fn image_aspect_ratio(&self) -> f32 {
        self.image_aspect_ratio
    }
    fn asset_name(&self) -> String {
        format!("crane/destinations/sleep_{}.jpg", self.id)
    }
    fn photo(&self) -> &'static [u8] {
        self.photo
    }

    fn subtitle(&self) -> String {
        GalleryLocalizations::en().crane_sleep_properties(self.total as i64)
    }
}

/// Upstream's `EatDestination`.
pub struct EatDestination {
    pub id: u32,
    pub destination: &'static str,
    pub total: u32,
    pub asset_semantic_label: &'static str,
    pub image_aspect_ratio: f32,
    pub photo: &'static [u8],
}

impl Destination for EatDestination {
    fn id(&self) -> u32 {
        self.id
    }
    fn name(&self) -> &'static str {
        self.destination
    }
    fn asset_semantic_label(&self) -> &'static str {
        self.asset_semantic_label
    }
    fn image_aspect_ratio(&self) -> f32 {
        self.image_aspect_ratio
    }
    fn asset_name(&self) -> String {
        format!("crane/destinations/eat_{}.jpg", self.id)
    }
    fn photo(&self) -> &'static [u8] {
        self.photo
    }

    fn subtitle(&self) -> String {
        GalleryLocalizations::en().crane_eat_restaurants(self.total as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHOTO: &[u8] = b"not a real photograph";

    #[test]
    fn a_fly_destination_joins_stops_and_duration() {
        let destination = FlyDestination {
            id: 0,
            destination: "Aspen, United States",
            stops: 1,
            duration: Some(Duration::from_secs(6 * 3600 + 15 * 60)),
            asset_semantic_label: "Chalet",
            image_aspect_ratio: 1.0,
            photo: PHOTO,
        };
        assert_eq!(destination.subtitle(), "1 stop · 6h 15m");
        assert_eq!(destination.subtitle_semantics(), "1 stop, 6h 15m");
        assert_eq!(destination.asset_name(), "crane/destinations/fly_0.jpg");
    }

    #[test]
    fn a_fly_destination_without_a_duration_shows_stops_only() {
        let destination = FlyDestination {
            id: 2,
            destination: "Khumbu Valley, Nepal",
            stops: 0,
            duration: None,
            asset_semantic_label: "Prayer flags",
            image_aspect_ratio: 1.0,
            photo: PHOTO,
        };
        assert_eq!(destination.subtitle(), "0 stops");
    }

    #[test]
    fn sleep_and_eat_subtitles_count_properties_and_restaurants() {
        let sleep = SleepDestination {
            id: 0,
            destination: "Malé, Maldives",
            total: 2241,
            asset_semantic_label: "Bungalows",
            image_aspect_ratio: 1.0,
            photo: PHOTO,
        };
        assert_eq!(sleep.subtitle(), "2241 Available Properties");
        assert_eq!(sleep.asset_name(), "crane/destinations/sleep_0.jpg");
        let eat = EatDestination {
            id: 0,
            destination: "Naples, Italy",
            total: 354,
            asset_semantic_label: "Pizzeria",
            image_aspect_ratio: 1.0,
            photo: PHOTO,
        };
        assert_eq!(eat.subtitle(), "354 Restaurants");
        assert_eq!(eat.asset_name(), "crane/destinations/eat_0.jpg");
    }
}
