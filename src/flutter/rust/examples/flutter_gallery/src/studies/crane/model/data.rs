// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/model/data.dart` (flutter/gallery @
//! d12640d): the three destination tables.
//!
//! Upstream builds a fresh `List` per call, resolving names and semantic
//! labels through the localizations. The catalogue is English-only
//! (PORTING.md), so the resolved English strings are the tables here, in
//! upstream's order, with upstream's ids, stops, durations, totals and
//! aspect ratios. The photographs are upstream's too, copied from
//! `flutter_gallery_assets`' `crane/destinations/` (the 1x files) into
//! `assets/crane/destinations/` and baked in with `include_bytes!`; see
//! `assets/README.md`.

use std::time::Duration;

use super::destination::{Destination, EatDestination, FlyDestination, SleepDestination};

const fn hours_and_minutes(hours: u64, minutes: u64) -> Duration {
    Duration::from_secs(hours * 3600 + minutes * 60)
}

/// Upstream's `getFlyDestinations`.
pub fn fly_destinations() -> &'static [FlyDestination] {
    static DESTINATIONS: &[FlyDestination] = &[
        FlyDestination {
            id: 0,
            destination: "Aspen, United States",
            stops: 1,
            duration: Some(hours_and_minutes(6, 15)),
            asset_semantic_label: "Chalet in a snowy landscape with evergreen trees",
            image_aspect_ratio: 400.0 / 400.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_0.jpg"),
        },
        FlyDestination {
            id: 1,
            destination: "Big Sur, United States",
            stops: 0,
            duration: Some(hours_and_minutes(13, 30)),
            asset_semantic_label: "Tent in a field",
            image_aspect_ratio: 400.0 / 410.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_1.jpg"),
        },
        FlyDestination {
            id: 2,
            destination: "Khumbu Valley, Nepal",
            stops: 0,
            duration: Some(hours_and_minutes(5, 16)),
            asset_semantic_label: "Prayer flags in front of snowy mountain",
            image_aspect_ratio: 400.0 / 394.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_2.jpg"),
        },
        FlyDestination {
            id: 3,
            destination: "Machu Picchu, Peru",
            stops: 2,
            duration: Some(hours_and_minutes(19, 40)),
            asset_semantic_label: "Machu Picchu citadel",
            image_aspect_ratio: 400.0 / 377.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_3.jpg"),
        },
        FlyDestination {
            id: 4,
            destination: "Malé, Maldives",
            stops: 0,
            duration: Some(hours_and_minutes(8, 24)),
            asset_semantic_label: "Overwater bungalows",
            image_aspect_ratio: 400.0 / 308.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_4.jpg"),
        },
        FlyDestination {
            id: 5,
            destination: "Vitznau, Switzerland",
            stops: 1,
            duration: Some(hours_and_minutes(14, 12)),
            asset_semantic_label: "Lake-side hotel in front of mountains",
            image_aspect_ratio: 400.0 / 418.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_5.jpg"),
        },
        FlyDestination {
            id: 6,
            destination: "Mexico City, Mexico",
            stops: 0,
            duration: Some(hours_and_minutes(5, 24)),
            asset_semantic_label: "Aerial view of Palacio de Bellas Artes",
            image_aspect_ratio: 400.0 / 345.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_6.jpg"),
        },
        FlyDestination {
            id: 7,
            destination: "Mount Rushmore, United States",
            stops: 1,
            duration: Some(hours_and_minutes(5, 43)),
            asset_semantic_label: "Mount Rushmore",
            image_aspect_ratio: 400.0 / 408.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_7.jpg"),
        },
        FlyDestination {
            id: 8,
            destination: "Singapore",
            stops: 0,
            duration: Some(hours_and_minutes(8, 25)),
            asset_semantic_label: "Supertree Grove",
            image_aspect_ratio: 400.0 / 399.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_8.jpg"),
        },
        FlyDestination {
            id: 9,
            destination: "Havana, Cuba",
            stops: 1,
            duration: Some(hours_and_minutes(15, 52)),
            asset_semantic_label: "Man leaning on an antique blue car",
            image_aspect_ratio: 400.0 / 379.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_9.jpg"),
        },
        FlyDestination {
            id: 10,
            destination: "Cairo, Egypt",
            stops: 0,
            duration: Some(hours_and_minutes(5, 57)),
            asset_semantic_label: "Al-Azhar Mosque towers during sunset",
            image_aspect_ratio: 400.0 / 307.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_10.jpg"),
        },
        FlyDestination {
            id: 11,
            destination: "Lisbon, Portugal",
            stops: 1,
            duration: Some(hours_and_minutes(13, 24)),
            asset_semantic_label: "Brick lighthouse at sea",
            image_aspect_ratio: 400.0 / 369.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_11.jpg"),
        },
        FlyDestination {
            id: 12,
            destination: "Napa, United States",
            stops: 2,
            duration: Some(hours_and_minutes(10, 20)),
            asset_semantic_label: "Pool with palm trees",
            image_aspect_ratio: 400.0 / 394.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_12.jpg"),
        },
        FlyDestination {
            id: 13,
            destination: "Bali, Indonesia",
            stops: 0,
            duration: Some(hours_and_minutes(7, 15)),
            asset_semantic_label: "Sea-side pool with palm trees",
            image_aspect_ratio: 400.0 / 433.0,
            photo: include_bytes!("../../../../assets/crane/destinations/fly_13.jpg"),
        },
    ];
    DESTINATIONS
}

/// Upstream's `getSleepDestinations`.
pub fn sleep_destinations() -> &'static [SleepDestination] {
    static DESTINATIONS: &[SleepDestination] = &[
        SleepDestination {
            id: 0,
            destination: "Malé, Maldives",
            total: 2241,
            asset_semantic_label: "Overwater bungalows",
            image_aspect_ratio: 400.0 / 308.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_0.jpg"),
        },
        SleepDestination {
            id: 1,
            destination: "Aspen, United States",
            total: 876,
            asset_semantic_label: "Chalet in a snowy landscape with evergreen trees",
            // Upstream leaves this one at the `imageAspectRatio` default of 1.
            image_aspect_ratio: 1.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_1.jpg"),
        },
        SleepDestination {
            id: 2,
            destination: "Machu Picchu, Peru",
            total: 1286,
            asset_semantic_label: "Machu Picchu citadel",
            image_aspect_ratio: 400.0 / 377.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_2.jpg"),
        },
        SleepDestination {
            id: 3,
            destination: "Havana, Cuba",
            total: 496,
            asset_semantic_label: "Man leaning on an antique blue car",
            image_aspect_ratio: 400.0 / 379.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_3.jpg"),
        },
        SleepDestination {
            id: 4,
            destination: "Vitznau, Switzerland",
            total: 390,
            asset_semantic_label: "Lake-side hotel in front of mountains",
            image_aspect_ratio: 400.0 / 418.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_4.jpg"),
        },
        SleepDestination {
            id: 5,
            destination: "Big Sur, United States",
            total: 876,
            asset_semantic_label: "Tent in a field",
            image_aspect_ratio: 400.0 / 410.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_5.jpg"),
        },
        SleepDestination {
            id: 6,
            destination: "Napa, United States",
            total: 989,
            asset_semantic_label: "Pool with palm trees",
            image_aspect_ratio: 400.0 / 394.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_6.jpg"),
        },
        SleepDestination {
            id: 7,
            destination: "Porto, Portugal",
            total: 306,
            asset_semantic_label: "Colorful apartments at Riberia Square",
            image_aspect_ratio: 400.0 / 266.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_7.jpg"),
        },
        SleepDestination {
            id: 8,
            destination: "Tulum, Mexico",
            total: 385,
            asset_semantic_label: "Mayan ruins on a cliff above a beach",
            image_aspect_ratio: 400.0 / 376.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_8.jpg"),
        },
        SleepDestination {
            id: 9,
            destination: "Lisbon, Portugal",
            total: 989,
            asset_semantic_label: "Brick lighthouse at sea",
            image_aspect_ratio: 400.0 / 369.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_9.jpg"),
        },
        SleepDestination {
            id: 10,
            destination: "Cairo, Egypt",
            total: 1380,
            asset_semantic_label: "Al-Azhar Mosque towers during sunset",
            image_aspect_ratio: 400.0 / 307.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_10.jpg"),
        },
        SleepDestination {
            id: 11,
            destination: "Taipei, Taiwan",
            total: 1109,
            asset_semantic_label: "Taipei 101 skyscraper",
            image_aspect_ratio: 400.0 / 456.0,
            photo: include_bytes!("../../../../assets/crane/destinations/sleep_11.jpg"),
        },
    ];
    DESTINATIONS
}

/// Upstream's `getEatDestinations`.
pub fn eat_destinations() -> &'static [EatDestination] {
    static DESTINATIONS: &[EatDestination] = &[
        EatDestination {
            id: 0,
            destination: "Naples, Italy",
            total: 354,
            asset_semantic_label: "Pizza in a wood-fired oven",
            image_aspect_ratio: 400.0 / 444.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_0.jpg"),
        },
        EatDestination {
            id: 1,
            destination: "Dallas, United States",
            total: 623,
            asset_semantic_label: "Empty bar with diner-style stools",
            image_aspect_ratio: 400.0 / 340.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_1.jpg"),
        },
        EatDestination {
            id: 2,
            destination: "Córdoba, Argentina",
            total: 124,
            asset_semantic_label: "Burger",
            image_aspect_ratio: 400.0 / 406.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_2.jpg"),
        },
        EatDestination {
            id: 3,
            destination: "Portland, United States",
            total: 495,
            asset_semantic_label: "Korean taco",
            image_aspect_ratio: 400.0 / 323.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_3.jpg"),
        },
        EatDestination {
            id: 4,
            destination: "Paris, France",
            total: 683,
            asset_semantic_label: "Chocolate dessert",
            image_aspect_ratio: 400.0 / 404.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_4.jpg"),
        },
        EatDestination {
            id: 5,
            destination: "Seoul, South Korea",
            total: 786,
            asset_semantic_label: "Artsy restaurant seating area",
            image_aspect_ratio: 400.0 / 407.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_5.jpg"),
        },
        EatDestination {
            id: 6,
            destination: "Seattle, United States",
            total: 323,
            asset_semantic_label: "Shrimp dish",
            image_aspect_ratio: 400.0 / 431.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_6.jpg"),
        },
        EatDestination {
            id: 7,
            destination: "Nashville, United States",
            total: 285,
            asset_semantic_label: "Bakery entrance",
            image_aspect_ratio: 400.0 / 422.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_7.jpg"),
        },
        EatDestination {
            id: 8,
            destination: "Atlanta, United States",
            total: 323,
            asset_semantic_label: "Plate of crawfish",
            image_aspect_ratio: 400.0 / 300.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_8.jpg"),
        },
        EatDestination {
            id: 9,
            destination: "Madrid, Spain",
            total: 1406,
            asset_semantic_label: "Cafe counter with pastries",
            image_aspect_ratio: 400.0 / 451.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_9.jpg"),
        },
        EatDestination {
            id: 10,
            destination: "Lisbon, Portugal",
            total: 849,
            asset_semantic_label: "Woman holding huge pastrami sandwich",
            image_aspect_ratio: 400.0 / 266.0,
            photo: include_bytes!("../../../../assets/crane/destinations/eat_10.jpg"),
        },
    ];
    DESTINATIONS
}

/// The table for a tab index: 0 fly, 1 sleep, 2 eat -- upstream's
/// `_FrontLayer.didChangeDependencies` chain. A fresh `Vec` per call rather
/// than a shared static: `dyn Destination` is not `Sync`, so a static of
/// trait objects cannot compile, and the three tables behind the functions
/// already are.
pub fn destinations_for_tab(tab: usize) -> Vec<&'static dyn Destination> {
    match tab {
        1 => sleep_destinations()
            .iter()
            .map(|d| d as &'static dyn Destination)
            .collect(),
        2 => eat_destinations()
            .iter()
            .map(|d| d as &'static dyn Destination)
            .collect(),
        _ => fly_destinations()
            .iter()
            .map(|d| d as &'static dyn Destination)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_match_upstream_in_size_order_and_ids() {
        assert_eq!(fly_destinations().len(), 14);
        assert_eq!(sleep_destinations().len(), 12);
        assert_eq!(eat_destinations().len(), 11);
        for (id, destination) in fly_destinations().iter().enumerate() {
            assert_eq!(destination.id, id as u32);
        }
        for (id, destination) in sleep_destinations().iter().enumerate() {
            assert_eq!(destination.id, id as u32);
        }
        for (id, destination) in eat_destinations().iter().enumerate() {
            assert_eq!(destination.id, id as u32);
        }
        // Spot-check the first and last rows against upstream's table.
        assert_eq!(fly_destinations()[0].destination, "Aspen, United States");
        assert_eq!(fly_destinations()[13].destination, "Bali, Indonesia");
        assert_eq!(sleep_destinations()[11].total, 1109);
        assert_eq!(eat_destinations()[9].destination, "Madrid, Spain");
    }

    #[test]
    fn every_photograph_is_a_jpeg() {
        for tab in 0..3 {
            for destination in destinations_for_tab(tab) {
                let photo = destination.photo();
                assert!(
                    photo.len() > 4 && photo[0] == 0xFF && photo[1] == 0xD8,
                    "{} is not a JPEG",
                    destination.asset_name(),
                );
            }
        }
    }

    #[test]
    fn the_tab_dispatch_matches_upstream() {
        assert_eq!(destinations_for_tab(0)[0].name(), "Aspen, United States");
        assert_eq!(destinations_for_tab(1)[0].name(), "Malé, Maldives");
        assert_eq!(destinations_for_tab(2)[0].name(), "Naples, Italy");
        // Anything past the last tab is the fly tab, as upstream's chain of
        // `if`s leaves index 0's list standing.
        assert_eq!(destinations_for_tab(3)[0].name(), "Aspen, United States");
    }

    #[test]
    fn the_asset_names_point_at_the_copied_files() {
        // The bytes are compiled in, but the names are the provenance trail:
        // each should name a file that was actually copied.
        for tab in 0..3 {
            for destination in destinations_for_tab(tab) {
                let name = destination.asset_name();
                assert!(name.starts_with("crane/destinations/"));
                assert!(name.ends_with(".jpg"));
            }
        }
    }
}
