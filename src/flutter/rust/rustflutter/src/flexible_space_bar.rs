// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/flexible_space_bar.dart`: the part of an app bar that
//! changes as the page scrolls under it.
//!
//! A flexible space bar is what turns a tall header with a photograph into a
//! plain toolbar. It is handed *how much room it currently has* -- by the
//! sliver above it, through [`FlexibleSpaceBarSettings`] -- and everything
//! else follows from one number: how far through that collapse it is.
//!
//! # `t`, and why it is the only state
//!
//! `t` runs 0 when fully expanded to 1 when collapsed to the toolbar, and
//! every other quantity here is a function of it: the background's opacity,
//! the title's scale, where the background sits. The bar itself remembers
//! nothing between frames, which is what lets it be driven by a scroll
//! position that can jump.

use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext};
use crate::render::EdgeInsets;

/// Upstream's `kToolbarHeight`: what the bar collapses *to*.
pub const TOOLBAR_HEIGHT: f32 = 56.0;

/// Upstream `CollapseMode`: what the background does while the bar collapses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CollapseMode {
    /// The background moves at a quarter of the bar's own rate, so it appears
    /// to lie behind the page rather than on it. Upstream's default, and the
    /// effect the whole widget is named for.
    #[default]
    Parallax,
    /// The background stays where it is relative to the toolbar, so the bar
    /// scrolls over a fixed image.
    Pin,
    /// The background scrolls with the bar, at its full rate.
    None,
}

/// Upstream `StretchMode`: what an *over*-scrolled bar does with the extra
/// room, when the list is dragged past its top.
///
/// A list of modes rather than one choice, because they compose: a header can
/// zoom its photograph *and* blur it *and* fade its title at the same time,
/// and each is a separate decision about a different part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StretchMode {
    /// The background grows to fill the extra room. Upstream's default, and
    /// the only one that is on unless asked.
    ZoomBackground,
    BlurBackground,
    FadeTitle,
}

/// Upstream `FlexibleSpaceBarSettings`: what the sliver tells the bar.
///
/// Upstream this is an `InheritedWidget` the persistent-header delegate wraps
/// the bar in, so the bar can read its own geometry without being handed it
/// through every layer in between. Here it is a value put in with
/// [`crate::framework::provide`] and read with `BuildContext::inherited`,
/// which is this crate's equivalent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexibleSpaceBarSettings {
    /// How opaque the toolbar's own contents are -- separate from the
    /// background's, because the toolbar fades in as the background fades
    /// out.
    pub toolbar_opacity: f32,
    pub min_extent: f32,
    pub max_extent: f32,
    pub current_extent: f32,
    /// Upstream's `isScrolledUnder`, which an app bar uses to decide whether
    /// to show its elevation overlay.
    pub is_scrolled_under: Option<bool>,
    /// Upstream's `hasLeading`. It decides the title's leading padding, since
    /// a title has to clear a back button that may not be there.
    pub has_leading: Option<bool>,
}

impl FlexibleSpaceBarSettings {
    /// Upstream's `FlexibleSpaceBar.createSettings`, whose defaults are worth
    /// keeping: an unstated min or max extent is the *current* one, which
    /// means "this bar is not collapsing" rather than zero.
    pub fn new(current_extent: f32) -> FlexibleSpaceBarSettings {
        FlexibleSpaceBarSettings {
            toolbar_opacity: 1.0,
            min_extent: current_extent,
            max_extent: current_extent,
            current_extent,
            is_scrolled_under: None,
            has_leading: None,
        }
    }

    pub fn with_extents(mut self, min_extent: f32, max_extent: f32) -> Self {
        self.min_extent = min_extent;
        self.max_extent = max_extent;
        self
    }

    pub fn with_toolbar_opacity(mut self, opacity: f32) -> Self {
        self.toolbar_opacity = opacity;
        self
    }

    pub fn with_has_leading(mut self, has_leading: bool) -> Self {
        self.has_leading = Some(has_leading);
        self
    }

    pub fn with_scrolled_under(mut self, is_scrolled_under: bool) -> Self {
        self.is_scrolled_under = Some(is_scrolled_under);
        self
    }

    /// How far the bar can collapse. Zero when it cannot.
    pub fn delta_extent(&self) -> f32 {
        self.max_extent - self.min_extent
    }

    /// Upstream's `t`: 0 fully expanded, 1 collapsed to the toolbar.
    ///
    /// A bar that cannot collapse -- `min == max` -- answers 0 rather than
    /// dividing by nothing. Upstream reaches the same answer by a different
    /// road (its division yields infinity, which the clamp turns into 1, and
    /// then every consumer special-cases `maxExtent == minExtent` back to the
    /// expanded look); answering 0 here says the same thing once.
    pub fn t(&self) -> f32 {
        let delta = self.delta_extent();
        if delta == 0.0 {
            return 0.0;
        }
        (1.0 - (self.current_extent - self.min_extent) / delta).clamp(0.0, 1.0)
    }
}

/// Upstream `FlexibleSpaceBar`.
pub struct FlexibleSpaceBar {
    pub title: Option<String>,
    pub background: std::cell::RefCell<Option<AnyWidget>>,
    pub center_title: Option<bool>,
    pub title_padding: Option<EdgeInsets>,
    pub collapse_mode: CollapseMode,
    pub stretch_modes: Vec<StretchMode>,
    /// Upstream's `expandedTitleScale`, at least 1: the title is *larger* when
    /// the bar is open and shrinks to its toolbar size as it closes. Below 1
    /// it would grow while collapsing, which is why upstream asserts.
    pub expanded_title_scale: f32,
}

impl FlexibleSpaceBar {
    /// Upstream's default `expandedTitleScale`.
    pub const DEFAULT_EXPANDED_TITLE_SCALE: f32 = 1.5;
    /// Upstream's leading padding when there is a back button to clear.
    pub const LEADING_PADDING: f32 = 72.0;

    pub fn new() -> FlexibleSpaceBar {
        FlexibleSpaceBar {
            title: None,
            background: std::cell::RefCell::new(None),
            center_title: None,
            title_padding: None,
            collapse_mode: CollapseMode::Parallax,
            stretch_modes: vec![StretchMode::ZoomBackground],
            expanded_title_scale: FlexibleSpaceBar::DEFAULT_EXPANDED_TITLE_SCALE,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_background(self, background: AnyWidget) -> Self {
        *self.background.borrow_mut() = Some(background);
        self
    }

    pub fn with_center_title(mut self, center: bool) -> Self {
        self.center_title = Some(center);
        self
    }

    pub fn with_collapse_mode(mut self, mode: CollapseMode) -> Self {
        self.collapse_mode = mode;
        self
    }

    pub fn with_stretch_modes(mut self, modes: Vec<StretchMode>) -> Self {
        self.stretch_modes = modes;
        self
    }

    pub fn with_expanded_title_scale(mut self, scale: f32) -> Self {
        debug_assert!(
            scale >= 1.0,
            "an expanded title is larger than a collapsed one"
        );
        self.expanded_title_scale = scale;
        self
    }

    pub fn with_title_padding(mut self, padding: EdgeInsets) -> Self {
        self.title_padding = Some(padding);
        self
    }

    /// Upstream's `_getCollapsePadding`: how far the background is offset as
    /// the bar collapses, which is the whole of the parallax.
    ///
    /// `Pin` cancels the collapse exactly, so the background does not move
    /// with the bar at all. `Parallax` moves it a *quarter* of the way -- the
    /// number that makes the background read as further away than the page,
    /// since something behind you appears to move less than something beside
    /// you. `None` leaves it to scroll at the bar's own rate.
    pub fn collapse_padding(&self, settings: &FlexibleSpaceBarSettings) -> f32 {
        match self.collapse_mode {
            CollapseMode::Pin => -(settings.max_extent - settings.current_extent),
            CollapseMode::None => 0.0,
            CollapseMode::Parallax => -(settings.delta_extent() / 4.0) * settings.t(),
        }
    }

    /// Upstream's background opacity.
    ///
    /// The fade happens over the *last* [`TOOLBAR_HEIGHT`] of the collapse, not
    /// over the whole of it: a photograph that started fading the moment the
    /// reader scrolled would look like a bug, where one that holds and then
    /// goes as the toolbar closes in reads as the toolbar arriving.
    ///
    /// A bar that cannot collapse is fully opaque -- upstream says so outright
    /// ("the app bar cannot collapse and the content should be visible"), and
    /// it is also what stops a zero delta from deciding the answer.
    pub fn background_opacity(&self, settings: &FlexibleSpaceBarSettings) -> f32 {
        let delta = settings.delta_extent();
        if delta == 0.0 {
            return 1.0;
        }
        let fade_start = (1.0 - TOOLBAR_HEIGHT / delta).max(0.0);
        let t = settings.t();
        // Upstream's `Interval(fadeStart, 1.0)`: nothing until `fade_start`,
        // then straight to one.
        let through = if t <= fade_start {
            0.0
        } else {
            ((t - fade_start) / (1.0 - fade_start)).clamp(0.0, 1.0)
        };
        1.0 - through
    }

    /// Upstream's title scale: from [`FlexibleSpaceBar::expanded_title_scale`]
    /// down to 1 as the bar closes.
    pub fn title_scale(&self, settings: &FlexibleSpaceBarSettings) -> f32 {
        let t = settings.t();
        self.expanded_title_scale + (1.0 - self.expanded_title_scale) * t
    }

    /// Upstream's title padding. The leading inset clears a back button, and
    /// is dropped for a centred title, which has no button to clear on the
    /// side it sits.
    pub fn effective_title_padding(&self, settings: &FlexibleSpaceBarSettings) -> EdgeInsets {
        if let Some(padding) = self.title_padding {
            return padding;
        }
        let centred = self.center_title.unwrap_or(false);
        let leading = if settings.has_leading.unwrap_or(true) {
            FlexibleSpaceBar::LEADING_PADDING
        } else {
            0.0
        };
        EdgeInsets::only(if centred { 0.0 } else { leading }, 0.0, 0.0, 16.0)
    }

    /// Upstream's `StretchMode.zoomBackground`: how tall the background is
    /// drawn, given the room the bar was actually given.
    ///
    /// Larger than `maxExtent` only when the list has been dragged past its
    /// top, which is the only time there *is* extra room.
    pub fn background_height(
        &self,
        settings: &FlexibleSpaceBarSettings,
        available_height: f32,
    ) -> f32 {
        if self.stretch_modes.contains(&StretchMode::ZoomBackground)
            && available_height > settings.max_extent
        {
            available_height
        } else {
            settings.max_extent
        }
    }

    /// Upstream's `StretchMode.blurBackground`: the blur sigma, which is the
    /// overscroll over ten. Zero when the mode is off or nothing is
    /// overscrolled.
    pub fn blur_sigma(&self, settings: &FlexibleSpaceBarSettings, available_height: f32) -> f32 {
        if self.stretch_modes.contains(&StretchMode::BlurBackground)
            && available_height > settings.max_extent
        {
            (available_height - settings.max_extent) / 10.0
        } else {
            0.0
        }
    }

    /// Upstream's `StretchMode.fadeTitle`: the title fades out over 100
    /// logical pixels of overscroll, so a hard pull leaves the photograph
    /// alone on the screen.
    pub fn stretch_title_opacity(
        &self,
        settings: &FlexibleSpaceBarSettings,
        available_height: f32,
    ) -> f32 {
        if self.stretch_modes.contains(&StretchMode::FadeTitle)
            && available_height > settings.max_extent
        {
            1.0 - ((available_height - settings.max_extent) / 100.0).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// The colour the title is drawn in, faded by the toolbar's own opacity.
    pub fn title_color(&self, base: Color, settings: &FlexibleSpaceBarSettings) -> Color {
        let alpha = (base.alpha() as f32 * settings.toolbar_opacity.clamp(0.0, 1.0)).round();
        base.with_alpha(alpha.clamp(0.0, 255.0) as u8)
    }
}

impl Default for FlexibleSpaceBar {
    fn default() -> FlexibleSpaceBar {
        FlexibleSpaceBar::new()
    }
}

/// The settings the nearest enclosing sliver put in, or a bar that is not
/// collapsing at all.
pub fn settings_of(context: &BuildContext, current_extent: f32) -> FlexibleSpaceBarSettings {
    context
        .inherited::<FlexibleSpaceBarSettings>()
        .map(|settings| *settings)
        .unwrap_or_else(|| FlexibleSpaceBarSettings::new(current_extent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header 200 tall that collapses to a 56 toolbar.
    fn settings(current: f32) -> FlexibleSpaceBarSettings {
        FlexibleSpaceBarSettings::new(current).with_extents(56.0, 200.0)
    }

    #[test]
    fn t_runs_from_expanded_to_collapsed() {
        assert_eq!(settings(200.0).t(), 0.0, "fully open");
        assert_eq!(settings(56.0).t(), 1.0, "closed to the toolbar");
        assert_eq!(settings(128.0).t(), 0.5, "half way");
    }

    #[test]
    fn t_is_clamped_so_an_overscrolled_bar_is_not_more_than_open() {
        // Dragging past the top gives a current extent larger than the max,
        // which would otherwise make `t` negative and every consumer of it
        // read backwards.
        assert_eq!(settings(400.0).t(), 0.0);
        assert_eq!(settings(0.0).t(), 1.0);
    }

    #[test]
    fn a_bar_that_cannot_collapse_reads_as_fully_open_and_fully_opaque() {
        // The zero-delta case, which is a division by nothing if it is not
        // caught. Upstream catches it by special-casing every consumer;
        // catching it once in `t` says the same thing.
        let fixed = FlexibleSpaceBarSettings::new(120.0);
        assert_eq!(fixed.delta_extent(), 0.0);
        assert_eq!(fixed.t(), 0.0);
        assert_eq!(
            FlexibleSpaceBar::new().background_opacity(&fixed),
            1.0,
            "the content should be visible"
        );
    }

    #[test]
    fn an_unstated_extent_means_this_bar_is_not_collapsing() {
        // Upstream's `createSettings` defaults min and max to the *current*
        // extent rather than to zero, which is what makes a bar with no
        // sliver above it behave as a plain one rather than as a fully
        // collapsed one.
        let bare = FlexibleSpaceBarSettings::new(120.0);
        assert_eq!(bare.min_extent, 120.0);
        assert_eq!(bare.max_extent, 120.0);
        assert_eq!(bare.toolbar_opacity, 1.0);
    }

    #[test]
    fn parallax_moves_the_background_a_quarter_of_the_way() {
        // The number that makes the background read as further away than the
        // page: something behind you appears to move less than something
        // beside you.
        let bar = FlexibleSpaceBar::new();
        assert_eq!(
            bar.collapse_mode,
            CollapseMode::Parallax,
            "and it is default"
        );
        // delta is 144, so a full collapse offsets the background by 36.
        assert_eq!(bar.collapse_padding(&settings(56.0)), -36.0);
        assert_eq!(bar.collapse_padding(&settings(128.0)), -18.0, "half of it");
        assert_eq!(bar.collapse_padding(&settings(200.0)), 0.0);
    }

    #[test]
    fn pinning_cancels_the_collapse_exactly() {
        // So the bar scrolls over a background that does not move at all.
        let bar = FlexibleSpaceBar::new().with_collapse_mode(CollapseMode::Pin);
        assert_eq!(bar.collapse_padding(&settings(200.0)), 0.0);
        assert_eq!(bar.collapse_padding(&settings(128.0)), -72.0);
        assert_eq!(bar.collapse_padding(&settings(56.0)), -144.0);
    }

    #[test]
    fn none_lets_the_background_scroll_at_the_bars_own_rate() {
        let bar = FlexibleSpaceBar::new().with_collapse_mode(CollapseMode::None);
        for current in [200.0, 128.0, 56.0] {
            assert_eq!(bar.collapse_padding(&settings(current)), 0.0);
        }
    }

    #[test]
    fn the_background_holds_and_then_fades_over_the_last_toolbar_height() {
        // Not over the whole collapse: a photograph that started fading the
        // moment the reader scrolled would look like a bug, where one that
        // holds and then goes reads as the toolbar arriving.
        let bar = FlexibleSpaceBar::new();
        assert_eq!(bar.background_opacity(&settings(200.0)), 1.0);
        // delta 144, so the fade starts at t = 1 - 56/144 = 0.611.
        assert_eq!(
            bar.background_opacity(&settings(128.0)),
            1.0,
            "half way down and still fully there"
        );
        assert!(bar.background_opacity(&settings(70.0)) < 1.0, "nearly shut");
        assert_eq!(bar.background_opacity(&settings(56.0)), 0.0);
    }

    #[test]
    fn a_shallow_bar_fades_across_its_whole_collapse() {
        // When the collapse is shorter than a toolbar there is no room to
        // hold, so `fadeStart` floors at zero and the fade runs the whole way.
        let shallow = FlexibleSpaceBarSettings::new(60.0).with_extents(56.0, 80.0);
        let bar = FlexibleSpaceBar::new();
        assert!(bar.background_opacity(&shallow) < 1.0, "already fading");
    }

    #[test]
    fn the_title_shrinks_to_its_toolbar_size_as_the_bar_closes() {
        let bar = FlexibleSpaceBar::new();
        assert_eq!(bar.title_scale(&settings(200.0)), 1.5);
        assert_eq!(bar.title_scale(&settings(128.0)), 1.25);
        assert_eq!(bar.title_scale(&settings(56.0)), 1.0);
    }

    #[test]
    fn the_titles_leading_inset_clears_a_back_button_only_when_there_is_one() {
        let bar = FlexibleSpaceBar::new();
        let with_button = settings(200.0).with_has_leading(true);
        let without = settings(200.0).with_has_leading(false);
        assert_eq!(bar.effective_title_padding(&with_button).left, 72.0);
        assert_eq!(bar.effective_title_padding(&without).left, 0.0);
        // Unstated means there is one, which is the safe assumption: a title
        // under a button is worse than one indented past nothing.
        assert_eq!(bar.effective_title_padding(&settings(200.0)).left, 72.0);
        // A centred title has no button to clear on the side it sits.
        let centred = FlexibleSpaceBar::new().with_center_title(true);
        assert_eq!(centred.effective_title_padding(&with_button).left, 0.0);
        // And every one of them keeps the 16 above the bar's bottom edge.
        assert_eq!(bar.effective_title_padding(&with_button).bottom, 16.0);
    }

    #[test]
    fn stretching_only_happens_when_the_list_is_dragged_past_its_top() {
        // Which is the only time there is extra room to use.
        let bar = FlexibleSpaceBar::new().with_stretch_modes(vec![
            StretchMode::ZoomBackground,
            StretchMode::BlurBackground,
            StretchMode::FadeTitle,
        ]);
        let open = settings(200.0);
        assert_eq!(bar.background_height(&open, 200.0), 200.0);
        assert_eq!(bar.blur_sigma(&open, 200.0), 0.0);
        assert_eq!(bar.stretch_title_opacity(&open, 200.0), 1.0);

        // Dragged 50 past: the background grows to fill it, the blur is that
        // over ten, and the title is half gone at 50 of the 100 it fades over.
        assert_eq!(bar.background_height(&open, 250.0), 250.0);
        assert_eq!(bar.blur_sigma(&open, 250.0), 5.0);
        assert_eq!(bar.stretch_title_opacity(&open, 250.0), 0.5);
    }

    #[test]
    fn the_stretch_modes_are_separate_decisions_that_compose() {
        // A list rather than one choice: a header can zoom *and* blur *and*
        // fade at once, and each is about a different part.
        let open = settings(200.0);
        let zoom_only = FlexibleSpaceBar::new();
        assert_eq!(
            zoom_only.background_height(&open, 250.0),
            250.0,
            "on by default"
        );
        assert_eq!(zoom_only.blur_sigma(&open, 250.0), 0.0, "but not blurring");
        assert_eq!(zoom_only.stretch_title_opacity(&open, 250.0), 1.0);

        let blur_only =
            FlexibleSpaceBar::new().with_stretch_modes(vec![StretchMode::BlurBackground]);
        assert_eq!(
            blur_only.background_height(&open, 250.0),
            200.0,
            "no zoom without the mode"
        );
        assert_eq!(blur_only.blur_sigma(&open, 250.0), 5.0);
    }

    #[test]
    fn the_title_fades_with_the_toolbars_own_opacity() {
        // Separate from the background's, because the toolbar fades in as the
        // background fades out.
        let bar = FlexibleSpaceBar::new();
        let solid = Color::argb(0xFF, 0, 0, 0);
        assert_eq!(bar.title_color(solid, &settings(200.0)).alpha(), 0xFF);
        let half = settings(200.0).with_toolbar_opacity(0.5);
        assert_eq!(bar.title_color(solid, &half).alpha(), 128);
        let gone = settings(200.0).with_toolbar_opacity(0.0);
        assert_eq!(bar.title_color(solid, &gone).alpha(), 0);
    }
}
