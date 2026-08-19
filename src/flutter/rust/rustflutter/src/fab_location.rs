// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/floating_action_button_location.dart`: where the
//! floating action button goes, and how it moves when that changes.
//!
//! The whole file is arithmetic on one input -- the
//! [`ScaffoldPrelayoutGeometry`] the scaffold hands out after it has laid
//! everything else out and before it places the button. That ordering is the
//! reason the class exists at all: the button is placed *last*, so it can be
//! told where the content ended, how tall the snack bar is, and how far the
//! keyboard has pushed in.
//!
//! # The cross product
//!
//! Upstream ships nineteen named locations, and they are not nineteen
//! classes' worth of behaviour: they are three horizontal rules
//! ([`FabStartOffsetX`], [`FabCenterOffsetX`], [`FabEndOffsetX`]) crossed with
//! four vertical ones ([`FabTopOffsetY`], [`FabFloatOffsetY`],
//! [`FabDockedOffsetY`], [`FabContainedOffsetY`]), optionally with the mini
//! adjustment ([`FabMiniOffsetAdjustment`]). Upstream expresses that as
//! mixins, one per rule, combined by each of the nineteen classes.
//!
//! Here each rule is a unit struct holding its formula -- a mixin with no
//! state is a namespace for one function -- and
//! [`FloatingActionButtonLocation`] is the pair of choices plus the flag.
//! The nineteen are its constants. A twentieth combination costs a constant
//! rather than a class, which is what the mixins were for.

use crate::direction::TextDirection;
use crate::render::{EdgeInsets, Offset, Size};

/// Upstream's `kFloatingActionButtonMargin`: how far the button sits from the
/// edges it is measured against.
pub const FLOATING_ACTION_BUTTON_MARGIN: f32 = 16.0;

/// Upstream's `kFloatingActionButtonSegue`, in microseconds: how long the
/// button takes to move when its location changes.
pub const FLOATING_ACTION_BUTTON_SEGUE_MICROS: i64 = 200_000;

/// Upstream's `kFloatingActionButtonTurnInterval`: the fraction of a full
/// turn the button rotates through while it moves.
pub const FLOATING_ACTION_BUTTON_TURN_INTERVAL: f32 = 0.125;

/// Upstream's `kMiniButtonOffsetAdjustment`: a mini button is smaller, so it
/// is nudged this far back towards the edge to keep its *visual* margin the
/// same as a full-sized one's.
pub const MINI_BUTTON_OFFSET_ADJUSTMENT: f32 = 4.0;

/// Upstream `ScaffoldPrelayoutGeometry` (`material/scaffold.dart`): what the
/// scaffold knows when it comes to place the button.
///
/// Everything here is measured *before* the button is placed and after
/// everything else has been, which is what lets a location react to a snack
/// bar or a keyboard it does not otherwise know about.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScaffoldPrelayoutGeometry {
    pub floating_action_button_size: Size,
    pub bottom_sheet_size: Size,
    /// The y of the bottom of the scaffold's body, in scaffold coordinates.
    pub content_bottom: f32,
    /// The y of the top of the body -- under the app bar, if there is one.
    pub content_top: f32,
    /// Upstream's `minInsets`, which is the media query's *viewInsets* floored
    /// at the scaffold's padding: what is covering the scaffold, chiefly the
    /// keyboard.
    pub min_insets: EdgeInsets,
    /// Upstream's `minViewPadding`: the parts of the screen the system owns --
    /// the notch, the home indicator -- with the keyboard's own inset taken
    /// out, so a button avoids the indicator without also avoiding the
    /// keyboard twice.
    pub min_view_padding: EdgeInsets,
    pub scaffold_size: Size,
    pub snack_bar_size: Size,
    pub material_banner_size: Size,
    pub text_direction: TextDirection,
}

/// Upstream's `StandardFabLocation._leftOffsetX`.
fn left_offset_x(geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
    FLOATING_ACTION_BUTTON_MARGIN + geometry.min_insets.left - adjustment
}

/// Upstream's `StandardFabLocation._rightOffsetX`.
///
/// Note the button's own width comes off: the offset is a *left* edge, so the
/// right-hand rule has to subtract the thing it is placing.
fn right_offset_x(geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
    geometry.scaffold_size.width
        - FLOATING_ACTION_BUTTON_MARGIN
        - geometry.min_insets.right
        - geometry.floating_action_button_size.width
        + adjustment
}

/// Upstream's `FabStartOffsetX` mixin: against the edge the reader starts
/// from, which swaps with the reading direction.
pub struct FabStartOffsetX;

impl FabStartOffsetX {
    pub fn offset_x(geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
        match geometry.text_direction {
            TextDirection::Rtl => right_offset_x(geometry, adjustment),
            TextDirection::Ltr => left_offset_x(geometry, adjustment),
        }
    }
}

/// Upstream's `FabCenterOffsetX` mixin.
pub struct FabCenterOffsetX;

impl FabCenterOffsetX {
    /// The `adjustment` is ignored, and upstream ignores it too: the mini
    /// nudge exists to keep a *margin* looking right, and a centred button has
    /// no margin to keep.
    pub fn offset_x(geometry: &ScaffoldPrelayoutGeometry, _adjustment: f32) -> f32 {
        (geometry.scaffold_size.width - geometry.floating_action_button_size.width) / 2.0
    }
}

/// Upstream's `FabEndOffsetX` mixin.
pub struct FabEndOffsetX;

impl FabEndOffsetX {
    pub fn offset_x(geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
        match geometry.text_direction {
            TextDirection::Rtl => left_offset_x(geometry, adjustment),
            TextDirection::Ltr => right_offset_x(geometry, adjustment),
        }
    }
}

/// Upstream's `FabTopOffsetY` mixin: straddling the bottom edge of the app
/// bar, half above it and half below.
pub struct FabTopOffsetY;

impl FabTopOffsetY {
    /// Upstream's fallback is the interesting half: with no app bar to
    /// straddle -- `contentTop` no higher than the system's own top padding --
    /// the button sits *at* the safe area's top edge rather than half over it,
    /// because half over it would be half under the notch.
    pub fn offset_y(geometry: &ScaffoldPrelayoutGeometry, _adjustment: f32) -> f32 {
        if geometry.content_top > geometry.min_view_padding.top {
            let half = geometry.floating_action_button_size.height / 2.0;
            return geometry.content_top - half;
        }
        geometry.min_view_padding.top
    }
}

/// Upstream's `FabFloatOffsetY` mixin: floating clear of the bottom edge.
pub struct FabFloatOffsetY;

impl FabFloatOffsetY {
    pub fn offset_y(geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
        let content_bottom = geometry.content_bottom;
        let bottom_content_height = geometry.scaffold_size.height - content_bottom;
        let bottom_sheet_height = geometry.bottom_sheet_size.height;
        let fab_height = geometry.floating_action_button_size.height;
        let snack_bar_height = geometry.snack_bar_size.height;
        // The margin grows if the system's bottom padding reaches past
        // whatever is already below the content: the button clears the home
        // indicator, but only by however much a bottom bar has not already
        // cleared it.
        let safe_margin = FLOATING_ACTION_BUTTON_MARGIN.max(
            geometry.min_view_padding.bottom - bottom_content_height
                + FLOATING_ACTION_BUTTON_MARGIN,
        );
        let mut fab_y = content_bottom - fab_height - safe_margin;
        // A snack bar pushes it up, and a bottom sheet pushes it up further --
        // but each with `min`, so whichever is worse wins rather than both
        // applying. Two things covering the same space are one obstruction.
        if snack_bar_height > 0.0 {
            fab_y = fab_y.min(
                content_bottom - snack_bar_height - fab_height - FLOATING_ACTION_BUTTON_MARGIN,
            );
        }
        if bottom_sheet_height > 0.0 {
            fab_y = fab_y.min(content_bottom - bottom_sheet_height - fab_height / 2.0);
        }
        fab_y + adjustment
    }
}

/// Upstream's `FabDockedOffsetY` mixin: sitting *on* the bottom bar, centre
/// on its top edge.
pub struct FabDockedOffsetY;

impl FabDockedOffsetY {
    /// The three-way margin is upstream's, and each branch has a reason:
    ///
    /// * Enough room below the content to show the button unclipped -- no
    ///   margin needed, so none is added.
    /// * No keyboard: the system's bottom padding, so the button clears the
    ///   home indicator.
    /// * A keyboard: half the button plus the standard margin, which is what
    ///   shifts it far enough to stay off the keyboard.
    pub fn offset_y(geometry: &ScaffoldPrelayoutGeometry, _adjustment: f32) -> f32 {
        let content_bottom = geometry.content_bottom;
        let content_margin = geometry.scaffold_size.height - content_bottom;
        let bottom_view_padding = geometry.min_view_padding.bottom;
        let bottom_sheet_height = geometry.bottom_sheet_size.height;
        let fab_height = geometry.floating_action_button_size.height;
        let snack_bar_height = geometry.snack_bar_size.height;
        let bottom_min_inset = geometry.min_insets.bottom;

        let safe_margin = if content_margin > bottom_min_inset + fab_height / 2.0 {
            0.0
        } else if bottom_min_inset == 0.0 {
            bottom_view_padding
        } else {
            fab_height / 2.0 + FLOATING_ACTION_BUTTON_MARGIN
        };

        let mut fab_y = content_bottom - fab_height / 2.0 - safe_margin;
        if snack_bar_height > 0.0 {
            fab_y = fab_y.min(
                content_bottom - snack_bar_height - fab_height - FLOATING_ACTION_BUTTON_MARGIN,
            );
        }
        // Its *centre* in front of the sheet's top edge, which is what
        // "docked" means: half on, half off.
        if bottom_sheet_height > 0.0 {
            fab_y = fab_y.min(content_bottom - bottom_sheet_height - fab_height / 2.0);
        }
        let max_fab_y = geometry.scaffold_size.height - fab_height - safe_margin;
        max_fab_y.min(fab_y)
    }
}

/// Upstream's `FabContainedOffsetY` mixin: centred *inside* the bottom bar
/// rather than straddling it.
pub struct FabContainedOffsetY;

impl FabContainedOffsetY {
    pub fn offset_y(geometry: &ScaffoldPrelayoutGeometry, _adjustment: f32) -> f32 {
        let content_bottom = geometry.content_bottom;
        let content_margin = geometry.scaffold_size.height - content_bottom;
        let bottom_view_padding = geometry.min_view_padding.bottom;
        let fab_height = geometry.floating_action_button_size.height;

        let safe_margin = if content_margin > bottom_view_padding + fab_height {
            0.0
        } else {
            bottom_view_padding
        };
        // The gap above the button inside the bar. Upstream's comment says it
        // can go negative when the bar is too short for the button, and it is
        // left negative on purpose: clamping it would centre a button that
        // does not fit, where letting it ride keeps the top of the button
        // visible.
        let content_bottom_to_fab_top = (content_margin - bottom_view_padding - fab_height) / 2.0;
        let fab_y = content_bottom + content_bottom_to_fab_top;
        let max_fab_y = geometry.scaffold_size.height - fab_height - safe_margin;
        max_fab_y.min(fab_y)
    }
}

/// Upstream's `FabMiniOffsetAdjustment` mixin, whose whole body is
/// `isMini() => true`.
pub struct FabMiniOffsetAdjustment;

impl FabMiniOffsetAdjustment {
    pub const ADJUSTMENT: f32 = MINI_BUTTON_OFFSET_ADJUSTMENT;

    pub fn is_mini() -> bool {
        true
    }
}

/// Which horizontal rule a location uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FabHorizontal {
    Start,
    Center,
    End,
}

/// Which vertical rule a location uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FabVertical {
    Top,
    Float,
    Docked,
    Contained,
}

/// Upstream `StandardFabLocation`: the contract a location satisfies once it
/// has split its answer into an x and a y.
///
/// Upstream this is an abstract subclass of `FloatingActionButtonLocation`
/// that turns one method into three -- `getOffsetX`, `getOffsetY`, `isMini` --
/// and supplies `getOffset` from them. A Rust trait with a provided method is
/// the same arrangement, so that is what it is.
pub trait StandardFabLocation {
    fn get_offset_x(&self, geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32;
    fn get_offset_y(&self, geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32;

    fn is_mini(&self) -> bool {
        false
    }

    /// Upstream's `getOffset`, which is the whole reason the split exists:
    /// the mini adjustment is worked out once here rather than in each of the
    /// nineteen.
    fn get_offset(&self, geometry: &ScaffoldPrelayoutGeometry) -> Offset {
        let adjustment = if self.is_mini() {
            MINI_BUTTON_OFFSET_ADJUSTMENT
        } else {
            0.0
        };
        Offset::new(
            self.get_offset_x(geometry, adjustment),
            self.get_offset_y(geometry, adjustment),
        )
    }
}

/// Upstream `FloatingActionButtonLocation`: where the button goes.
///
/// Upstream is an abstract class with nineteen named subclasses; here it is
/// the two choices and the flag those subclasses encode, with the nineteen as
/// constants. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatingActionButtonLocation {
    pub horizontal: FabHorizontal,
    pub vertical: FabVertical,
    pub mini: bool,
}

impl FloatingActionButtonLocation {
    const fn at(
        horizontal: FabHorizontal,
        vertical: FabVertical,
        mini: bool,
    ) -> FloatingActionButtonLocation {
        FloatingActionButtonLocation {
            horizontal,
            vertical,
            mini,
        }
    }

    pub const START_TOP: Self = Self::at(FabHorizontal::Start, FabVertical::Top, false);
    pub const MINI_START_TOP: Self = Self::at(FabHorizontal::Start, FabVertical::Top, true);
    pub const CENTER_TOP: Self = Self::at(FabHorizontal::Center, FabVertical::Top, false);
    pub const MINI_CENTER_TOP: Self = Self::at(FabHorizontal::Center, FabVertical::Top, true);
    pub const END_TOP: Self = Self::at(FabHorizontal::End, FabVertical::Top, false);
    pub const MINI_END_TOP: Self = Self::at(FabHorizontal::End, FabVertical::Top, true);

    pub const START_FLOAT: Self = Self::at(FabHorizontal::Start, FabVertical::Float, false);
    pub const MINI_START_FLOAT: Self = Self::at(FabHorizontal::Start, FabVertical::Float, true);
    pub const CENTER_FLOAT: Self = Self::at(FabHorizontal::Center, FabVertical::Float, false);
    pub const MINI_CENTER_FLOAT: Self = Self::at(FabHorizontal::Center, FabVertical::Float, true);
    pub const END_FLOAT: Self = Self::at(FabHorizontal::End, FabVertical::Float, false);
    pub const MINI_END_FLOAT: Self = Self::at(FabHorizontal::End, FabVertical::Float, true);

    pub const START_DOCKED: Self = Self::at(FabHorizontal::Start, FabVertical::Docked, false);
    pub const MINI_START_DOCKED: Self = Self::at(FabHorizontal::Start, FabVertical::Docked, true);
    pub const CENTER_DOCKED: Self = Self::at(FabHorizontal::Center, FabVertical::Docked, false);
    pub const MINI_CENTER_DOCKED: Self = Self::at(FabHorizontal::Center, FabVertical::Docked, true);
    pub const END_DOCKED: Self = Self::at(FabHorizontal::End, FabVertical::Docked, false);
    pub const MINI_END_DOCKED: Self = Self::at(FabHorizontal::End, FabVertical::Docked, true);

    /// The only contained location upstream ships, and the only one with no
    /// mini counterpart -- a contained button is sized by the bar it sits in.
    pub const END_CONTAINED: Self = Self::at(FabHorizontal::End, FabVertical::Contained, false);
}

impl StandardFabLocation for FloatingActionButtonLocation {
    fn get_offset_x(&self, geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
        match self.horizontal {
            FabHorizontal::Start => FabStartOffsetX::offset_x(geometry, adjustment),
            FabHorizontal::Center => FabCenterOffsetX::offset_x(geometry, adjustment),
            FabHorizontal::End => FabEndOffsetX::offset_x(geometry, adjustment),
        }
    }

    fn get_offset_y(&self, geometry: &ScaffoldPrelayoutGeometry, adjustment: f32) -> f32 {
        match self.vertical {
            FabVertical::Top => FabTopOffsetY::offset_y(geometry, adjustment),
            FabVertical::Float => FabFloatOffsetY::offset_y(geometry, adjustment),
            FabVertical::Docked => FabDockedOffsetY::offset_y(geometry, adjustment),
            FabVertical::Contained => FabContainedOffsetY::offset_y(geometry, adjustment),
        }
    }

    fn is_mini(&self) -> bool {
        self.mini
    }
}

/// Upstream `FloatingActionButtonAnimator`: how the button gets from one
/// location to the next.
///
/// The two upstream ships are here. `Scaling` is the interesting one and its
/// shape is worth stating: the button does not slide. It **shrinks to nothing
/// at the old place and grows back at the new one**, jumping between them at
/// the half-way mark -- which is why `get_offset` is a step function rather
/// than an interpolation. A button that slid across the screen would draw the
/// reader's eye along a path that means nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FloatingActionButtonAnimator {
    #[default]
    Scaling,
    /// Upstream's `noAnimation`: straight to the end, at full size.
    NoAnimation,
}

/// Upstream's `Interval(0.5, 1.0, curve: Curves.ease)`, which both halves of
/// the scale animation read: nothing until half way, then eased to one.
fn scale_interval(t: f32) -> f32 {
    if t <= 0.5 {
        0.0
    } else {
        crate::animation::Curve::EASE.transform((t - 0.5) * 2.0)
    }
}

impl FloatingActionButtonAnimator {
    /// Upstream's `getOffset`. A step at the half-way point for the scaling
    /// animator -- the button is invisible there, so the jump is not seen --
    /// and the end outright for the other.
    pub fn get_offset(&self, begin: Offset, end: Offset, progress: f32) -> Offset {
        match self {
            FloatingActionButtonAnimator::Scaling => {
                if progress < 0.5 {
                    begin
                } else {
                    end
                }
            }
            FloatingActionButtonAnimator::NoAnimation => end,
        }
    }

    /// Upstream's `getScaleAnimation`, as the value rather than the
    /// `Animation`: down from 1 to 0 over the first half, back to 1 over the
    /// second.
    ///
    /// Both halves read the *same* `Interval(0.5, 1.0, ease)` -- the second at
    /// `progress` and the first at `1 - progress`, which is what upstream's
    /// `ReverseAnimation` over the flipped curve comes to. So the shrink is
    /// the grow played backwards, exactly, and the two halves cannot drift.
    pub fn scale(&self, progress: f32) -> f32 {
        match self {
            FloatingActionButtonAnimator::Scaling => {
                if progress < 0.5 {
                    scale_interval(1.0 - progress)
                } else {
                    scale_interval(progress)
                }
            }
            FloatingActionButtonAnimator::NoAnimation => 1.0,
        }
    }

    /// Upstream's `getRotationAnimation`, in turns.
    ///
    /// The numbers, which are not what the comment beside them says. Upstream
    /// writes "this rotation will turn on the way in, but not on the way out",
    /// and then swaps a `Tween(0.75 -> 1.0)` for the *first* half against a
    /// reversed `Threshold(0.5)` -- constant zero -- for the second. So the
    /// half that turns is the one where the button is shrinking away, and the
    /// half where it grows back holds still.
    ///
    /// Ported as written rather than as described. A port that followed the
    /// comment would animate differently from the framework it is a port of,
    /// and the discrepancy is pinned by a regression line so that nobody
    /// "fixes" it from the prose.
    pub fn rotation(&self, progress: f32) -> f32 {
        match self {
            FloatingActionButtonAnimator::Scaling => {
                if progress < 0.5 {
                    let from = 1.0 - FLOATING_ACTION_BUTTON_TURN_INTERVAL * 2.0;
                    from + (1.0 - from) * progress
                } else {
                    // `ReverseAnimation(Threshold(0.5))`: one minus one.
                    0.0
                }
            }
            FloatingActionButtonAnimator::NoAnimation => 1.0,
        }
    }

    /// Upstream's `getAnimationRestart`: where to resume from when the
    /// location changes again mid-move.
    ///
    /// `min(1 - previous, previous)` is upstream's, and its point is in the
    /// comment beside it: a move interrupted while *starting* carries on from
    /// where it was, and one interrupted while *finishing* is treated as
    /// starting again from the same point in reverse. Either way the button
    /// is the same size before and after the restart, so there is no jump.
    pub fn animation_restart(&self, previous: f32) -> f32 {
        match self {
            FloatingActionButtonAnimator::Scaling => (1.0 - previous).min(previous),
            FloatingActionButtonAnimator::NoAnimation => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain phone-shaped scaffold: no keyboard, no snack bar, no sheet, a
    /// 56-tall app bar and a 56-square button.
    fn geometry() -> ScaffoldPrelayoutGeometry {
        ScaffoldPrelayoutGeometry {
            floating_action_button_size: Size::new(56.0, 56.0),
            bottom_sheet_size: Size::ZERO,
            content_bottom: 800.0,
            content_top: 56.0,
            min_insets: EdgeInsets::ZERO,
            min_view_padding: EdgeInsets::ZERO,
            scaffold_size: Size::new(400.0, 800.0),
            snack_bar_size: Size::ZERO,
            material_banner_size: Size::ZERO,
            text_direction: TextDirection::Ltr,
        }
    }

    #[test]
    fn start_and_end_swap_with_the_reading_direction_and_centre_does_not() {
        let ltr = geometry();
        let rtl = ScaffoldPrelayoutGeometry {
            text_direction: TextDirection::Rtl,
            ..geometry()
        };
        // 400 - 16 - 56 = 328 on the right, 16 on the left.
        assert_eq!(FabStartOffsetX::offset_x(&ltr, 0.0), 16.0);
        assert_eq!(FabEndOffsetX::offset_x(&ltr, 0.0), 328.0);
        assert_eq!(FabStartOffsetX::offset_x(&rtl, 0.0), 328.0);
        assert_eq!(FabEndOffsetX::offset_x(&rtl, 0.0), 16.0);
        // (400 - 56) / 2, either way.
        assert_eq!(FabCenterOffsetX::offset_x(&ltr, 0.0), 172.0);
        assert_eq!(FabCenterOffsetX::offset_x(&rtl, 0.0), 172.0);
    }

    #[test]
    fn the_mini_adjustment_moves_the_button_towards_the_edge_at_both_ends() {
        // A mini button is smaller, so it is nudged back towards the edge to
        // keep the *visual* margin the same. Which way that is depends on
        // which edge, so the two signs differ.
        let g = geometry();
        assert_eq!(FabStartOffsetX::offset_x(&g, 4.0), 12.0, "4 further left");
        assert_eq!(FabEndOffsetX::offset_x(&g, 4.0), 332.0, "4 further right");
    }

    #[test]
    fn a_centred_button_ignores_the_mini_adjustment() {
        // Upstream ignores it too: the nudge exists to keep a margin looking
        // right, and a centred button has no margin to keep.
        let g = geometry();
        assert_eq!(
            FabCenterOffsetX::offset_x(&g, 4.0),
            FabCenterOffsetX::offset_x(&g, 0.0)
        );
    }

    #[test]
    fn only_a_mini_location_applies_the_adjustment() {
        let g = geometry();
        assert_eq!(
            FloatingActionButtonLocation::START_FLOAT.get_offset(&g).dx,
            16.0
        );
        assert_eq!(
            FloatingActionButtonLocation::MINI_START_FLOAT
                .get_offset(&g)
                .dx,
            12.0
        );
        assert!(!FloatingActionButtonLocation::START_FLOAT.is_mini());
        assert!(FloatingActionButtonLocation::MINI_START_FLOAT.is_mini());
    }

    #[test]
    fn a_top_button_straddles_the_app_bars_bottom_edge() {
        // Half above, half below: the shape that makes it read as belonging
        // to the bar and to the content at once.
        let g = geometry();
        assert_eq!(FabTopOffsetY::offset_y(&g, 0.0), 56.0 - 28.0);
    }

    #[test]
    fn a_top_button_with_no_bar_to_straddle_sits_inside_the_safe_area() {
        // Upstream's fallback, and the reason for it: half over an edge that
        // is the notch would be half under the notch.
        let g = ScaffoldPrelayoutGeometry {
            content_top: 24.0,
            min_view_padding: EdgeInsets::only(0.0, 44.0, 0.0, 0.0),
            ..geometry()
        };
        assert_eq!(
            FabTopOffsetY::offset_y(&g, 0.0),
            44.0,
            "at the safe area's edge, not half over it"
        );
    }

    #[test]
    fn a_floating_button_clears_the_bottom_edge_by_the_standard_margin() {
        // 800 - 56 - 16.
        assert_eq!(FabFloatOffsetY::offset_y(&geometry(), 0.0), 728.0);
    }

    #[test]
    fn a_snack_bar_pushes_a_floating_button_up_and_a_sheet_pushes_it_further() {
        let with_snack_bar = ScaffoldPrelayoutGeometry {
            snack_bar_size: Size::new(400.0, 48.0),
            ..geometry()
        };
        // 800 - 48 - 56 - 16.
        assert_eq!(FabFloatOffsetY::offset_y(&with_snack_bar, 0.0), 680.0);

        // A sheet puts the button's *centre* on its top edge: 800 - 200 - 28.
        let with_sheet = ScaffoldPrelayoutGeometry {
            bottom_sheet_size: Size::new(400.0, 200.0),
            ..geometry()
        };
        assert_eq!(FabFloatOffsetY::offset_y(&with_sheet, 0.0), 572.0);
    }

    #[test]
    fn two_things_covering_the_same_space_are_one_obstruction() {
        // Each clause is a `min`, not a subtraction, so a snack bar over a
        // sheet does not push the button up twice.
        let both = ScaffoldPrelayoutGeometry {
            snack_bar_size: Size::new(400.0, 48.0),
            bottom_sheet_size: Size::new(400.0, 200.0),
            ..geometry()
        };
        let sheet_only = ScaffoldPrelayoutGeometry {
            snack_bar_size: Size::ZERO,
            ..both
        };
        assert_eq!(
            FabFloatOffsetY::offset_y(&both, 0.0),
            FabFloatOffsetY::offset_y(&sheet_only, 0.0),
            "the sheet is the worse of the two, so it alone decides"
        );
    }

    #[test]
    fn a_docked_button_never_hangs_off_the_bottom_of_the_scaffold() {
        // "Docked" means centre on the bar's top edge -- 800 - 28 = 772 here.
        // But upstream ends with `min(maxFabY, fabY)`, and with the content
        // running to the very bottom there is no bar to dock onto, so the
        // clamp wins and the button sits fully on screen at 800 - 56.
        // Worth pinning: without the clamp a scaffold with no bottom bar
        // would draw half a button off the edge.
        assert_eq!(FabDockedOffsetY::offset_y(&geometry(), 0.0), 744.0);
    }

    #[test]
    fn a_keyboard_shifts_a_docked_button_clear_of_it() {
        // The third branch of upstream's margin: with a keyboard up, half the
        // button plus the standard margin is what keeps it off the keys.
        let keyboard = ScaffoldPrelayoutGeometry {
            min_insets: EdgeInsets::only(0.0, 0.0, 0.0, 300.0),
            content_bottom: 500.0,
            ..geometry()
        };
        // content_margin 300 is not greater than 300 + 28, so the margin is
        // 28 + 16 = 44: 500 - 28 - 44.
        assert_eq!(FabDockedOffsetY::offset_y(&keyboard, 0.0), 428.0);
    }

    #[test]
    fn a_docked_button_with_room_below_the_content_needs_no_margin_at_all() {
        // The first branch: enough room to show the button unclipped, so none
        // is added and the button sits exactly on the edge.
        let with_bar = ScaffoldPrelayoutGeometry {
            content_bottom: 700.0,
            min_view_padding: EdgeInsets::only(0.0, 0.0, 0.0, 34.0),
            ..geometry()
        };
        // content_margin 100 > 0 + 28, so safe_margin is zero.
        assert_eq!(FabDockedOffsetY::offset_y(&with_bar, 0.0), 700.0 - 28.0);
    }

    #[test]
    fn a_contained_button_is_centred_inside_the_bar_rather_than_on_its_edge() {
        // The difference from docked, and the whole of it.
        let bar = ScaffoldPrelayoutGeometry {
            content_bottom: 700.0,
            ..geometry()
        };
        // (100 - 0 - 56) / 2 = 22 above the button: 700 + 22.
        assert_eq!(FabContainedOffsetY::offset_y(&bar, 0.0), 722.0);
        // Where docked would put it half over the edge, 44 higher.
        assert_eq!(FabDockedOffsetY::offset_y(&bar, 0.0), 672.0);
    }

    #[test]
    fn a_contained_button_taller_than_its_bar_stays_on_screen() {
        // Two upstream rules meet here. The gap is allowed to go negative --
        // `(contentMargin - bottomViewPadding - fabHeight) / 2` -- so a button
        // that does not fit starts *above* the bar rather than being centred
        // and clipped at both ends: that alone would give 780 - 18 = 762.
        // Then `min(maxFabY, fabY)` clamps it to 800 - 56, which is what
        // actually keeps the whole button on screen.
        let short_bar = ScaffoldPrelayoutGeometry {
            content_bottom: 780.0,
            ..geometry()
        };
        assert_eq!(FabContainedOffsetY::offset_y(&short_bar, 0.0), 744.0);
    }

    #[test]
    fn the_scaling_animator_steps_between_the_two_places_rather_than_sliding() {
        // The button shrinks to nothing at the old place and grows back at
        // the new one; the jump happens where it is invisible.
        let animator = FloatingActionButtonAnimator::Scaling;
        let begin = Offset::new(0.0, 0.0);
        let end = Offset::new(300.0, 0.0);
        assert_eq!(animator.get_offset(begin, end, 0.0), begin);
        assert_eq!(animator.get_offset(begin, end, 0.49), begin);
        assert_eq!(animator.get_offset(begin, end, 0.5), end);
        assert_eq!(animator.get_offset(begin, end, 1.0), end);
    }

    #[test]
    fn the_scale_reaches_nothing_exactly_where_the_offset_jumps() {
        // Which is what makes the jump invisible, and is the reason the two
        // thresholds are the same number.
        let animator = FloatingActionButtonAnimator::Scaling;
        assert_eq!(animator.scale(0.0), 1.0);
        assert!(animator.scale(0.5).abs() < 0.001, "nothing at the swap");
        assert!((animator.scale(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn the_rotation_does_the_opposite_of_what_its_comment_says() {
        // Upstream writes "this rotation will turn on the way in, but not on
        // the way out", and then puts the `Tween(0.75 -> 1.0)` on the *first*
        // half -- the half where the button is shrinking away -- against a
        // constant zero on the second. Ported as written, and pinned here so
        // that nobody corrects the numbers from the prose.
        let animator = FloatingActionButtonAnimator::Scaling;
        assert_eq!(animator.rotation(0.0), 0.75, "three quarters of a turn");
        assert!(
            animator.rotation(0.25) > animator.rotation(0.0),
            "turning while it leaves"
        );
        assert_eq!(animator.rotation(0.5), 0.0, "and holding still after");
        assert_eq!(animator.rotation(1.0), 0.0);
    }

    #[test]
    fn a_move_interrupted_restarts_from_the_same_size_it_was() {
        // `min(1 - previous, previous)`: interrupted while starting, carry on;
        // interrupted while finishing, treat it as starting from the same
        // point in reverse. Either way the button does not jump in size.
        let animator = FloatingActionButtonAnimator::Scaling;
        assert_eq!(animator.animation_restart(0.2), 0.2, "still starting");
        assert_eq!(
            animator.animation_restart(0.8),
            0.19999999,
            "finishing, mirrored"
        );
        assert_eq!(animator.animation_restart(0.5), 0.5);
    }

    #[test]
    fn the_no_animation_animator_arrives_at_once_and_at_full_size() {
        let animator = FloatingActionButtonAnimator::NoAnimation;
        let begin = Offset::new(0.0, 0.0);
        let end = Offset::new(300.0, 0.0);
        assert_eq!(animator.get_offset(begin, end, 0.0), end, "no half-way");
        assert_eq!(animator.scale(0.5), 1.0);
        assert_eq!(animator.rotation(0.5), 1.0);
        // And nothing to resume, because nothing was running.
        assert_eq!(animator.animation_restart(0.5), 0.0);
    }

    #[test]
    fn every_named_location_is_a_distinct_pair_of_rules() {
        // Nineteen constants over three horizontals, four verticals and a
        // flag: the cross product upstream spells out as nineteen classes.
        let all = [
            FloatingActionButtonLocation::START_TOP,
            FloatingActionButtonLocation::MINI_START_TOP,
            FloatingActionButtonLocation::CENTER_TOP,
            FloatingActionButtonLocation::MINI_CENTER_TOP,
            FloatingActionButtonLocation::END_TOP,
            FloatingActionButtonLocation::MINI_END_TOP,
            FloatingActionButtonLocation::START_FLOAT,
            FloatingActionButtonLocation::MINI_START_FLOAT,
            FloatingActionButtonLocation::CENTER_FLOAT,
            FloatingActionButtonLocation::MINI_CENTER_FLOAT,
            FloatingActionButtonLocation::END_FLOAT,
            FloatingActionButtonLocation::MINI_END_FLOAT,
            FloatingActionButtonLocation::START_DOCKED,
            FloatingActionButtonLocation::MINI_START_DOCKED,
            FloatingActionButtonLocation::CENTER_DOCKED,
            FloatingActionButtonLocation::MINI_CENTER_DOCKED,
            FloatingActionButtonLocation::END_DOCKED,
            FloatingActionButtonLocation::MINI_END_DOCKED,
            FloatingActionButtonLocation::END_CONTAINED,
        ];
        assert_eq!(all.len(), 19);
        for (index, one) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(one, other, "two constants describe the same location");
            }
        }
        // And the one contained location has no mini counterpart: a contained
        // button is sized by the bar it sits in.
        assert!(!FloatingActionButtonLocation::END_CONTAINED.mini);
        assert_eq!(
            all.iter()
                .filter(|one| one.vertical == FabVertical::Contained)
                .count(),
            1
        );
    }
}
