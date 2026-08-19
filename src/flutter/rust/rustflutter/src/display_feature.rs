// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Laying out around a fold or a hinge (upstream
//! `widgets/display_feature_sub_screen.dart`).
//!
//! A folding phone has a screen with something across it -- a crease, a
//! hinge, a camera cutout -- and a dialog centred on the whole screen would
//! sit astride it. `DisplayFeatureSubScreen` splits the screen into the
//! rectangles the feature leaves whole, picks the one nearest an anchor
//! point, and lays its child out in that.
//!
//! # Recorded divergences
//!
//! * `DisplayFeature` and its two enums are `dart:ui` classes -- outside the
//!   ruler's `packages/flutter/lib/src` -- and are declared here because
//!   nothing else in this crate had needed them. The engine binding does not
//!   report any yet, so the list is empty in practice and
//!   [`sub_screens_in_bounds`] then returns the whole screen, which is what
//!   [`popup_menu_offset`](crate::menu::popup_menu_offset) already relies on.
//! * Upstream's widget reads the anchor's fallback from the text direction
//!   and asserts one is available. Here the direction is a parameter, because
//!   this crate passes direction rather than asserting it out of the context.

use crate::direction::TextDirection;
use crate::engine::Rect;
use crate::render::{Offset, Size};

/// Upstream `DisplayFeatureType`: what the thing across the screen is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayFeatureType {
    /// A fold with no separation -- one continuous screen that bends.
    Fold,
    /// A physical gap between two panels.
    Hinge,
    /// A cutout for a camera. Not a split: it is a hole in one screen.
    Cutout,
}

/// Upstream `DisplayFeatureState`: how far the device is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayFeatureState {
    /// Not applicable, which is what a cutout always is.
    Unknown,
    PostureFlat,
    /// Held part-way, like a book. The one state that matters to layout even
    /// when the feature itself has no width.
    PostureHalfOpened,
}

/// Upstream `DisplayFeature`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayFeature {
    pub bounds: Rect,
    pub feature_type: DisplayFeatureType,
    pub state: DisplayFeatureState,
}

impl DisplayFeature {
    pub fn new(
        bounds: Rect,
        feature_type: DisplayFeatureType,
        state: DisplayFeatureState,
    ) -> DisplayFeature {
        DisplayFeature {
            bounds,
            feature_type,
            state,
        }
    }
}

/// Upstream `DisplayFeatureSubScreen`.
pub struct DisplayFeatureSubScreen;

impl DisplayFeatureSubScreen {
    /// Upstream `avoidBounds`: which features are worth laying out around.
    ///
    /// A feature with no width *and* not half-opened is skipped: a fold in a
    /// flat device is a line on a continuous screen, and nothing is hidden by
    /// it. Half-opened is the exception, because then the two halves face
    /// different ways and content across the crease is unreadable even though
    /// the crease itself takes no room.
    pub fn avoid_bounds(features: &[DisplayFeature]) -> Vec<Rect> {
        features
            .iter()
            .filter(|feature| {
                feature.bounds.shortest_side() > 0.0
                    || feature.state == DisplayFeatureState::PostureHalfOpened
            })
            .map(|feature| feature.bounds)
            .collect()
    }

    /// Upstream `subScreensInBounds`: the rectangles left whole once every
    /// feature has been cut out.
    ///
    /// A feature splits a screen only when it crosses it completely -- a
    /// hinge that runs the full height splits it into left and right, one
    /// that runs the full width into top and bottom. A feature that stops
    /// part way through, like a cutout, splits nothing: there is no whole
    /// rectangle either side of it, so the screen is left as it is and the
    /// caller lays out over the hole.
    pub fn sub_screens_in_bounds(wanted_bounds: Rect, avoid_bounds: &[Rect]) -> Vec<Rect> {
        let mut sub_screens = vec![wanted_bounds];
        for bounds in avoid_bounds {
            let mut next = Vec::new();
            for screen in &sub_screens {
                if screen.top >= bounds.top && screen.bottom <= bounds.bottom {
                    // Splits this screen vertically: it spans the screen's
                    // whole height.
                    if screen.left < bounds.left {
                        next.push(Rect::xywh(
                            screen.left,
                            screen.top,
                            bounds.left - screen.left,
                            screen.height(),
                        ));
                    }
                    if screen.right > bounds.right {
                        next.push(Rect::xywh(
                            bounds.right,
                            screen.top,
                            screen.right - bounds.right,
                            screen.height(),
                        ));
                    }
                } else if screen.left >= bounds.left && screen.right <= bounds.right {
                    // Splits it horizontally.
                    if screen.top < bounds.top {
                        next.push(Rect::xywh(
                            screen.left,
                            screen.top,
                            screen.width(),
                            bounds.top - screen.top,
                        ));
                    }
                    if screen.bottom > bounds.bottom {
                        next.push(Rect::xywh(
                            screen.left,
                            bounds.bottom,
                            screen.width(),
                            screen.bottom - bounds.bottom,
                        ));
                    }
                } else {
                    // Crosses neither way: not a split.
                    next.push(*screen);
                }
            }
            sub_screens = next;
        }
        sub_screens
    }

    /// Upstream's `_distanceFromPointToRect`.
    ///
    /// Upstream's own diagram is the nine regions a point can be in relative
    /// to a rectangle: the four corners measure to the corner, the four sides
    /// measure straight across, and inside is zero.
    pub fn distance_from_point_to_rect(point: Offset, rect: Rect) -> f32 {
        let corner = |x: f32, y: f32| ((point.dx - x).powi(2) + (point.dy - y).powi(2)).sqrt();
        if point.dx < rect.left {
            if point.dy < rect.top {
                corner(rect.left, rect.top)
            } else if point.dy > rect.bottom {
                corner(rect.left, rect.bottom)
            } else {
                rect.left - point.dx
            }
        } else if point.dx > rect.right {
            if point.dy < rect.top {
                corner(rect.right, rect.top)
            } else if point.dy > rect.bottom {
                corner(rect.right, rect.bottom)
            } else {
                point.dx - rect.right
            }
        } else if point.dy < rect.top {
            rect.top - point.dy
        } else if point.dy > rect.bottom {
            point.dy - rect.bottom
        } else {
            0.0
        }
    }

    /// Upstream's `_closestToAnchorPoint`.
    ///
    /// The first is kept on a tie -- upstream's `<` and not `<=` -- so the
    /// answer does not depend on the order the features happened to arrive
    /// in when two sub-screens are equally near.
    pub fn closest_to_anchor_point(sub_screens: &[Rect], anchor_point: Offset) -> Option<Rect> {
        let mut closest = *sub_screens.first()?;
        let mut closest_distance =
            DisplayFeatureSubScreen::distance_from_point_to_rect(anchor_point, closest);
        for screen in sub_screens {
            let distance =
                DisplayFeatureSubScreen::distance_from_point_to_rect(anchor_point, *screen);
            if distance < closest_distance {
                closest = *screen;
                closest_distance = distance;
            }
        }
        Some(closest)
    }

    /// Upstream's `_capOffset`: an anchor outside the screen is pulled to its
    /// edge.
    ///
    /// Which is what makes the right-to-left fallback work: upstream's is
    /// `Offset(double.maxFinite, 0)`, meaning "the far right", and this is
    /// what turns that into a real point on the screen.
    pub fn cap_offset(offset: Offset, maximum: Size) -> Offset {
        Offset::new(
            offset.dx.max(0.0).min(maximum.width),
            offset.dy.max(0.0).min(maximum.height),
        )
    }

    /// Upstream's `_fallbackAnchorPoint`: which corner a reader's language
    /// starts from.
    pub fn fallback_anchor_point(direction: TextDirection) -> Offset {
        match direction {
            TextDirection::Ltr => Offset::ZERO,
            TextDirection::Rtl => Offset::new(f32::MAX, 0.0),
        }
    }

    /// The sub-screen a child should be laid out in: upstream's `build`, as
    /// the rectangle it works out rather than the padding it wraps the child
    /// in.
    ///
    /// A caller pads by `left`, `top`, `parent.width - right`,
    /// `parent.height - bottom`, which is upstream's `Padding`.
    pub fn sub_screen_for(
        parent_size: Size,
        features: &[DisplayFeature],
        anchor_point: Option<Offset>,
        direction: TextDirection,
    ) -> Rect {
        let wanted = Rect::xywh(0.0, 0.0, parent_size.width, parent_size.height);
        let anchor = DisplayFeatureSubScreen::cap_offset(
            anchor_point
                .unwrap_or_else(|| DisplayFeatureSubScreen::fallback_anchor_point(direction)),
            parent_size,
        );
        let avoid = DisplayFeatureSubScreen::avoid_bounds(features);
        let sub_screens = DisplayFeatureSubScreen::sub_screens_in_bounds(wanted, &avoid);
        DisplayFeatureSubScreen::closest_to_anchor_point(&sub_screens, anchor).unwrap_or(wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 400x800 screen with a hinge 20 wide down the middle, as a folding
    /// phone held open reports one.
    fn vertical_hinge() -> DisplayFeature {
        DisplayFeature::new(
            Rect::ltrb(190.0, 0.0, 210.0, 800.0),
            DisplayFeatureType::Hinge,
            DisplayFeatureState::PostureFlat,
        )
    }

    const SCREEN: Rect = Rect::ltrb(0.0, 0.0, 400.0, 800.0);

    #[test]
    fn a_hinge_across_the_screen_splits_it_in_two() {
        let sub_screens =
            DisplayFeatureSubScreen::sub_screens_in_bounds(SCREEN, &[vertical_hinge().bounds]);
        assert_eq!(
            sub_screens,
            vec![
                Rect::ltrb(0.0, 0.0, 190.0, 800.0),
                Rect::ltrb(210.0, 0.0, 400.0, 800.0)
            ]
        );
    }

    #[test]
    fn a_feature_that_stops_part_way_across_splits_nothing() {
        // A camera cutout is a hole in one screen, not a divider. There is no
        // whole rectangle either side of it, so the screen is left as it is
        // and the caller lays out over the hole -- which is right: a dialog
        // that dodged a cutout would jump for no visible reason.
        let cutout = Rect::ltrb(180.0, 0.0, 220.0, 40.0);
        assert_eq!(
            DisplayFeatureSubScreen::sub_screens_in_bounds(SCREEN, &[cutout]),
            vec![SCREEN]
        );
    }

    #[test]
    fn a_flat_fold_with_no_width_is_not_avoided_and_a_half_open_one_is() {
        // A fold on a flat device is a line on a continuous screen and hides
        // nothing. Half-opened is the exception: the two halves then face
        // different ways, and content across the crease is unreadable even
        // though the crease itself takes no room.
        let flat = DisplayFeature::new(
            Rect::ltrb(200.0, 0.0, 200.0, 800.0),
            DisplayFeatureType::Fold,
            DisplayFeatureState::PostureFlat,
        );
        assert!(DisplayFeatureSubScreen::avoid_bounds(&[flat]).is_empty());

        let half_open = DisplayFeature::new(
            Rect::ltrb(200.0, 0.0, 200.0, 800.0),
            DisplayFeatureType::Fold,
            DisplayFeatureState::PostureHalfOpened,
        );
        assert_eq!(DisplayFeatureSubScreen::avoid_bounds(&[half_open]).len(), 1);
        // And it does split, into two halves that meet at the crease.
        assert_eq!(
            DisplayFeatureSubScreen::sub_screens_in_bounds(SCREEN, &[half_open.bounds]),
            vec![
                Rect::ltrb(0.0, 0.0, 200.0, 800.0),
                Rect::ltrb(200.0, 0.0, 400.0, 800.0)
            ]
        );
    }

    #[test]
    fn two_features_split_the_screen_twice() {
        // The splitting is a fold over the features, so a device with a hinge
        // and a half-open crease gets three sub-screens rather than the
        // second feature being ignored.
        let horizontal = Rect::ltrb(0.0, 390.0, 400.0, 410.0);
        let sub_screens = DisplayFeatureSubScreen::sub_screens_in_bounds(
            SCREEN,
            &[vertical_hinge().bounds, horizontal],
        );
        assert_eq!(sub_screens.len(), 4, "two cuts each way give four quarters");
        assert!(sub_screens.contains(&Rect::ltrb(0.0, 0.0, 190.0, 390.0)));
        assert!(sub_screens.contains(&Rect::ltrb(210.0, 410.0, 400.0, 800.0)));
    }

    #[test]
    fn the_distance_to_a_rectangle_is_zero_from_inside_it() {
        let rect = Rect::ltrb(100.0, 100.0, 200.0, 200.0);
        assert_eq!(
            DisplayFeatureSubScreen::distance_from_point_to_rect(Offset::new(150.0, 150.0), rect),
            0.0
        );
        // Beside it: straight across, not to a corner.
        assert_eq!(
            DisplayFeatureSubScreen::distance_from_point_to_rect(Offset::new(80.0, 150.0), rect),
            20.0
        );
        assert_eq!(
            DisplayFeatureSubScreen::distance_from_point_to_rect(Offset::new(150.0, 230.0), rect),
            30.0
        );
        // Diagonally off a corner: the real distance, which is why the eight
        // regions are eight and not four.
        assert_eq!(
            DisplayFeatureSubScreen::distance_from_point_to_rect(Offset::new(97.0, 96.0), rect),
            5.0,
            "a 3-4-5 triangle off the top-left corner"
        );
    }

    #[test]
    fn a_left_to_right_reader_gets_the_left_half_and_a_right_to_left_one_the_right() {
        // The fallback anchor is the corner a reader's language starts from,
        // which is what decides which half a dialog lands in when nobody said.
        let features = [vertical_hinge()];
        assert_eq!(
            DisplayFeatureSubScreen::sub_screen_for(
                Size::new(400.0, 800.0),
                &features,
                None,
                TextDirection::Ltr
            ),
            Rect::ltrb(0.0, 0.0, 190.0, 800.0)
        );
        assert_eq!(
            DisplayFeatureSubScreen::sub_screen_for(
                Size::new(400.0, 800.0),
                &features,
                None,
                TextDirection::Rtl
            ),
            Rect::ltrb(210.0, 0.0, 400.0, 800.0)
        );
    }

    #[test]
    fn an_anchor_off_the_screen_is_pulled_to_its_edge() {
        // Which is what makes the right-to-left fallback work at all:
        // upstream's is "the far right", and capping is what turns that into
        // a point on the screen.
        assert_eq!(
            DisplayFeatureSubScreen::cap_offset(
                Offset::new(f32::MAX, 0.0),
                Size::new(400.0, 800.0)
            ),
            Offset::new(400.0, 0.0)
        );
        assert_eq!(
            DisplayFeatureSubScreen::cap_offset(Offset::new(-50.0, 900.0), Size::new(400.0, 800.0)),
            Offset::new(0.0, 800.0)
        );
        // A point already inside is untouched.
        assert_eq!(
            DisplayFeatureSubScreen::cap_offset(Offset::new(10.0, 20.0), Size::new(400.0, 800.0)),
            Offset::new(10.0, 20.0)
        );
    }

    #[test]
    fn an_explicit_anchor_beats_the_reading_direction() {
        let features = [vertical_hinge()];
        assert_eq!(
            DisplayFeatureSubScreen::sub_screen_for(
                Size::new(400.0, 800.0),
                &features,
                Some(Offset::new(399.0, 400.0)),
                TextDirection::Ltr
            ),
            Rect::ltrb(210.0, 0.0, 400.0, 800.0),
            "the anchor is on the right, so the right half wins"
        );
    }

    #[test]
    fn a_tie_keeps_the_first_sub_screen() {
        // Upstream compares with `<` and not `<=`, so an anchor equidistant
        // from two halves does not depend on the order the features happened
        // to arrive in.
        let halves = [
            Rect::ltrb(0.0, 0.0, 190.0, 800.0),
            Rect::ltrb(210.0, 0.0, 400.0, 800.0),
        ];
        assert_eq!(
            DisplayFeatureSubScreen::closest_to_anchor_point(&halves, Offset::new(200.0, 400.0)),
            Some(halves[0])
        );
    }

    #[test]
    fn a_screen_with_no_features_is_one_sub_screen() {
        // What the engine binding reports today, and what
        // `popup_menu_offset` already relies on.
        assert_eq!(
            DisplayFeatureSubScreen::sub_screens_in_bounds(SCREEN, &[]),
            vec![SCREEN]
        );
        assert_eq!(
            DisplayFeatureSubScreen::sub_screen_for(
                Size::new(400.0, 800.0),
                &[],
                None,
                TextDirection::Ltr
            ),
            SCREEN
        );
    }
}
