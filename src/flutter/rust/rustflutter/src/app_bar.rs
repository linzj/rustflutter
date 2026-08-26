//! Port of `material/app_bar.dart`'s `SliverAppBar`.
//!
//! An app bar that lives in a scroll view, and most of it is the arithmetic of
//! how much of itself is still showing.

/// Upstream `kToolbarHeight`.
pub const TOOLBAR_HEIGHT: f32 = 56.0;

/// Upstream's `_SliverAppVariant`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SliverAppVariant {
    #[default]
    Small,
    Medium,
    Large,
}

/// Why a sliver app bar's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliverAppBarError {
    /// `assert(floating || !snap, 'The "snap" argument only makes sense for floating app bars.')`
    SnapWithoutFloating,
    NonPositiveStretchTrigger,
    CollapsedHeightBelowToolbar,
}

/// Upstream's `_ScrollUnderFlexibleConfig`, which is the per-variant table
/// behind `SliverAppBar.medium` and `SliverAppBar.large`.
///
/// The small variant has none: it does not grow a title into the space below
/// its toolbar, so there is nothing for a config to describe.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollUnderConfig {
    /// How tall the bar is once it has been scrolled under. The same for both
    /// variants -- what makes a large bar large is only how far it *expands*.
    pub collapsed_height: f32,
    pub expanded_height: f32,
    /// The title's style while the bar is collapsed.
    pub collapsed_text_style: Option<crate::engine::TextStyle>,
    /// And while it is open. The one thing besides the height that separates
    /// the two variants.
    pub expanded_text_style: Option<crate::engine::TextStyle>,
    pub expanded_title_padding: crate::render::EdgeInsets,
}

impl ScrollUnderConfig {
    /// Both variants' `collapsedHeight`, and both variants' default
    /// `toolbarHeight`. A medium or large bar is eight pixels taller
    /// collapsed than an ordinary one, which is `kToolbarHeight`'s 56.
    pub const COLLAPSED_HEIGHT: f32 = 64.0;
    pub const MEDIUM_EXPANDED_HEIGHT: f32 = 112.0;
    pub const LARGE_EXPANDED_HEIGHT: f32 = 152.0;
    /// The room around an expanded title. The horizontal sixteen is the same
    /// in both; only the bottom differs, and that is what gives a large bar's
    /// title the extra breathing room its extra forty pixels bought.
    pub const TITLE_INSET: f32 = 16.0;
    pub const MEDIUM_TITLE_BOTTOM: f32 = 20.0;
    pub const LARGE_TITLE_BOTTOM: f32 = 28.0;

    /// The table for one variant, or `None` for the small one.
    ///
    /// `foreground` is the bar's resolved foreground colour. Upstream puts it
    /// over the config's own ink with a `copyWith`, so the `apply(color:
    /// onSurface)` written in each config is a value the next line
    /// overwrites: the two agree under the Material 3 default and stop
    /// agreeing the moment a bar names a foreground.
    pub fn of(
        variant: SliverAppVariant,
        theme: &crate::theme::ThemeData,
        foreground: crate::engine::Color,
    ) -> Option<ScrollUnderConfig> {
        let ink = |style: Option<crate::engine::TextStyle>| {
            style.map(|style| crate::engine::TextStyle {
                color: foreground,
                ..style
            })
        };
        let collapsed = ink(theme.text_theme.title_large.clone());
        match variant {
            SliverAppVariant::Small => None,
            SliverAppVariant::Medium => Some(ScrollUnderConfig {
                collapsed_height: ScrollUnderConfig::COLLAPSED_HEIGHT,
                expanded_height: ScrollUnderConfig::MEDIUM_EXPANDED_HEIGHT,
                collapsed_text_style: collapsed,
                expanded_text_style: ink(theme.text_theme.headline_small.clone()),
                expanded_title_padding: crate::render::EdgeInsets {
                    left: ScrollUnderConfig::TITLE_INSET,
                    top: 0.0,
                    right: ScrollUnderConfig::TITLE_INSET,
                    bottom: ScrollUnderConfig::MEDIUM_TITLE_BOTTOM,
                },
            }),
            SliverAppVariant::Large => Some(ScrollUnderConfig {
                collapsed_height: ScrollUnderConfig::COLLAPSED_HEIGHT,
                expanded_height: ScrollUnderConfig::LARGE_EXPANDED_HEIGHT,
                collapsed_text_style: collapsed,
                expanded_text_style: ink(theme.text_theme.headline_medium.clone()),
                expanded_title_padding: crate::render::EdgeInsets {
                    left: ScrollUnderConfig::TITLE_INSET,
                    top: 0.0,
                    right: ScrollUnderConfig::TITLE_INSET,
                    bottom: ScrollUnderConfig::LARGE_TITLE_BOTTOM,
                },
            }),
        }
    }

    /// The padding an expanded title actually gets, given what is under the
    /// bar.
    ///
    /// Upstream's `bottomHeight > 0 ? resolvedTitlePadding.copyWith(bottom: 0)
    /// : resolvedTitlePadding`. A tab bar or search field beneath the title
    /// brings its own room; keeping the twenty pixels as well would push the
    /// title up off the bar it belongs to.
    pub fn title_padding_over(&self, bottom_height: f32) -> crate::render::EdgeInsets {
        if bottom_height > 0.0 {
            crate::render::EdgeInsets {
                bottom: 0.0,
                ..self.expanded_title_padding
            }
        } else {
            self.expanded_title_padding
        }
    }
}

/// Upstream `SliverAppBar`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverAppBar {
    pub variant: SliverAppVariant,
    pub pinned: bool,
    pub floating: bool,
    pub snap: bool,
    pub stretch: bool,
    pub stretch_trigger_offset: f32,
    pub toolbar_height: f32,
    pub collapsed_height: Option<f32>,
    pub expanded_height: Option<f32>,
    pub top_padding: f32,
    pub bottom_height: f32,
    pub has_bottom: bool,
    pub force_elevated: bool,
}

impl SliverAppBar {
    pub fn new() -> SliverAppBar {
        SliverAppBar {
            variant: SliverAppVariant::Small,
            pinned: false,
            floating: false,
            snap: false,
            stretch: false,
            stretch_trigger_offset: 100.0,
            toolbar_height: TOOLBAR_HEIGHT,
            collapsed_height: None,
            expanded_height: None,
            top_padding: 0.0,
            bottom_height: 0.0,
            has_bottom: false,
            force_elevated: false,
        }
    }

    /// Upstream's three constructor asserts, written out identically in all
    /// three constructors (`SliverAppBar`, `.medium`, `.large`).
    ///
    /// The first is an **implication**: `snap` requires `floating`, and its
    /// message says why in words -- *"The 'snap' argument only makes sense for
    /// floating app bars."* Snapping is what a floating bar does when you stop
    /// mid-gesture; a bar that cannot float has nothing to snap to.
    pub fn validate(&self) -> Result<(), SliverAppBarError> {
        if self.snap && !self.floating {
            return Err(SliverAppBarError::SnapWithoutFloating);
        }
        if self.stretch_trigger_offset <= 0.0 {
            return Err(SliverAppBarError::NonPositiveStretchTrigger);
        }
        if self
            .collapsed_height
            .is_some_and(|height| height < self.toolbar_height)
        {
            return Err(SliverAppBarError::CollapsedHeightBelowToolbar);
        }
        Ok(())
    }

    /// Upstream's `minExtent`: the collapsed height, plain.
    pub fn min_extent(&self) -> f32 {
        self.collapsed_height.unwrap_or(self.toolbar_height) + self.top_padding
    }

    /// The heights the variant supplies when the caller named none.
    ///
    /// Upstream's `_SliverAppBarState.build` switches on the variant for
    /// exactly this, and nothing in this port read the variant at all:
    ///
    /// ```dart
    /// effectiveExpandedHeight = widget.expandedHeight
    ///     ?? _MediumScrollUnderFlexibleConfig.expandedHeight + bottomHeight;
    /// effectiveCollapsedHeight = widget.collapsedHeight
    ///     ?? topPadding + _MediumScrollUnderFlexibleConfig.collapsedHeight + bottomHeight;
    /// ```
    ///
    /// Note where the bottom goes: **into both**. A bar with a tab strip is
    /// that much taller open and that much taller shut, so the strip stays on
    /// screen the whole way down -- which is the point of putting it in the
    /// bar rather than under it.
    ///
    /// And note where the top padding goes: **into the collapsed height
    /// only**. The status bar's room is already inside `minExtent` by way of
    /// `top_padding`, so adding it to the expanded height as well would count
    /// it twice.
    pub fn variant_extents(&self) -> Option<(f32, f32)> {
        let config = match self.variant {
            SliverAppVariant::Small => return None,
            SliverAppVariant::Medium => (
                ScrollUnderConfig::COLLAPSED_HEIGHT,
                ScrollUnderConfig::MEDIUM_EXPANDED_HEIGHT,
            ),
            SliverAppVariant::Large => (
                ScrollUnderConfig::COLLAPSED_HEIGHT,
                ScrollUnderConfig::LARGE_EXPANDED_HEIGHT,
            ),
        };
        Some((
            self.collapsed_height
                .unwrap_or(self.top_padding + config.0 + self.bottom_height),
            self.expanded_height
                .unwrap_or(config.1 + self.bottom_height),
        ))
    }

    /// Upstream's `maxExtent`:
    ///
    /// ```dart
    /// math.max(topPadding + (expandedHeight ?? (toolbarHeight ?? kToolbarHeight) + _bottomHeight), minExtent)
    /// ```
    ///
    /// **The floor at `minExtent` is what stops the header inverting.** An
    /// `expandedHeight` smaller than the collapsed height would otherwise give a
    /// bar whose expanded state is shorter than its collapsed one, and every
    /// shrink calculation below would run backwards.
    pub fn max_extent(&self) -> f32 {
        let natural = self.top_padding
            + self
                .expanded_height
                .unwrap_or(self.toolbar_height + self.bottom_height);
        natural.max(self.min_extent())
    }

    /// How far the bar can shrink before it is fully collapsed.
    pub fn shrink_range(&self) -> f32 {
        self.max_extent() - self.min_extent()
    }

    /// Upstream's `isScrolledUnder`, which is what raises the elevation:
    /// `overlapsContent || forceElevated || (pinned && shrinkOffset > maxExtent - minExtent)`.
    ///
    /// The third term is the pinned bar's own way of noticing: it has run out of
    /// room to shrink, so anything scrolling now is going *behind* it.
    pub fn is_scrolled_under(&self, shrink_offset: f32, overlaps_content: bool) -> bool {
        overlaps_content
            || self.force_elevated
            || (self.pinned && shrink_offset > self.shrink_range())
    }

    /// Upstream's `_isPinnedWithOpacityFade`:
    /// `pinned && floating && bottom != null && extraToolbarHeight == 0.0`.
    ///
    /// Four conditions naming one arrangement -- a bar that stays put *and*
    /// floats *and* has something below the toolbar, where the toolbar slides
    /// away and leaves the bottom behind. It is the single case in which a
    /// pinned bar is allowed to fade.
    pub fn is_pinned_with_opacity_fade(&self) -> bool {
        self.pinned && self.floating && self.has_bottom && self.extra_toolbar_height() == 0.0
    }

    /// Upstream's `extraToolbarHeight`, clamped at zero.
    pub fn extra_toolbar_height(&self) -> f32 {
        (self.min_extent() - self.bottom_height - self.top_padding - self.toolbar_height).max(0.0)
    }

    /// Upstream's `toolbarOpacity`:
    ///
    /// ```dart
    /// final double toolbarOpacity = !accessibleNavigation && (!pinned || isPinnedWithOpacityFade)
    ///     ? clampDouble(visibleToolbarHeight / (toolbarHeight ?? kToolbarHeight), 0.0, 1.0)
    ///     : 1.0;
    /// ```
    ///
    /// **`accessibleNavigation` switches the fade off entirely.** With a screen
    /// reader running the toolbar stays fully opaque no matter how far it has
    /// scrolled -- because a half-faded toolbar is still focusable and still
    /// read aloud, and fading it would leave a control the reader can reach and
    /// the user cannot see.
    ///
    /// The accessibility path is not the ordinary path with a flag turned down;
    /// it is a different answer. Same shape as the expansion tile's whole extra
    /// second of delay for VoiceOver.
    ///
    /// The rest of the condition says a pinned bar does not fade at all, since
    /// it is always there -- except in the one arrangement
    /// [`SliverAppBar::is_pinned_with_opacity_fade`] names.
    pub fn toolbar_opacity(&self, shrink_offset: f32, accessible_navigation: bool) -> f32 {
        if accessible_navigation {
            return 1.0;
        }
        if self.pinned && !self.is_pinned_with_opacity_fade() {
            return 1.0;
        }
        let visible_main = self.max_extent() - shrink_offset - self.top_padding;
        let visible_toolbar = visible_main - self.bottom_height - self.extra_toolbar_height();
        (visible_toolbar / self.toolbar_height).clamp(0.0, 1.0)
    }

    /// Upstream's `bottomOpacity`: `pinned ? 1.0 : clamp(visibleMainHeight / _bottomHeight)`.
    ///
    /// The bottom never fades when pinned, with no exception for the arrangement
    /// above -- in that case the toolbar is what fades and the bottom is exactly
    /// the part that stays.
    pub fn bottom_opacity(&self, shrink_offset: f32) -> f32 {
        if self.pinned {
            return 1.0;
        }
        if self.bottom_height == 0.0 {
            return 1.0;
        }
        let visible_main = self.max_extent() - shrink_offset - self.top_padding;
        (visible_main / self.bottom_height).clamp(0.0, 1.0)
    }

    /// Upstream's `currentExtent`: `math.max(minExtent, maxExtent - shrinkOffset)`.
    pub fn current_extent(&self, shrink_offset: f32) -> f32 {
        (self.max_extent() - shrink_offset).max(self.min_extent())
    }

    /// The medium and large variants fade their title in on scroll-under rather
    /// than showing it always, which is the small variant's behaviour.
    pub fn title_follows_scrolled_under(&self) -> bool {
        !matches!(self.variant, SliverAppVariant::Small)
    }
}

impl Default for SliverAppBar {
    fn default() -> Self {
        SliverAppBar::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The variant, which nothing read, tick 258 ---------------------------
    //
    // `SliverAppVariant` was ported and `SliverAppBar` carried one, and
    // nothing in this port ever asked which it was. The variant is the whole
    // difference between upstream's three constructors.

    fn config(variant: SliverAppVariant) -> ScrollUnderConfig {
        ScrollUnderConfig::of(
            variant,
            &crate::theme::ThemeData::light(),
            crate::theme::ThemeData::light().color_scheme.on_surface,
        )
        .expect("medium and large have one")
    }

    #[test]
    fn a_small_bar_has_no_configuration_to_read() {
        // It does not grow a title into the space below its toolbar, so there
        // is nothing for a config to describe -- and `variant_extents` has
        // nothing to supply either, which is what leaves the caller's own
        // heights alone.
        assert!(
            ScrollUnderConfig::of(
                SliverAppVariant::Small,
                &crate::theme::ThemeData::light(),
                crate::engine::Color::BLACK,
            )
            .is_none()
        );
        let mut bar = SliverAppBar::new();
        bar.variant = SliverAppVariant::Small;
        assert_eq!(bar.variant_extents(), None);
    }

    #[test]
    fn both_variants_collapse_to_the_same_height_and_the_same_style() {
        // What makes a large bar large is only how far it *expands*. Four
        // numbers of which two are equal reads like a copy-paste until it is
        // said, so it is said.
        let medium = config(SliverAppVariant::Medium);
        let large = config(SliverAppVariant::Large);
        assert_eq!(medium.collapsed_height, large.collapsed_height);
        assert_eq!(medium.collapsed_height, 64.0);
        assert_eq!(medium.collapsed_text_style, large.collapsed_text_style);

        // And the expanded halves are where they differ.
        assert!(large.expanded_height > medium.expanded_height);
        assert_ne!(medium.expanded_text_style, large.expanded_text_style);
        assert!(
            large.expanded_title_padding.bottom > medium.expanded_title_padding.bottom,
            "the extra forty pixels buy the title extra room under it"
        );
        assert_eq!(
            medium.expanded_title_padding.left, large.expanded_title_padding.left,
            "and the sides do not move"
        );
    }

    #[test]
    fn a_large_bars_expanded_title_is_headline_medium() {
        // The role had no reader anywhere in this port. A medium bar's is
        // `headlineSmall`, a rung below, and both collapse to `titleLarge` --
        // which is the same role an ordinary app bar's title takes, so a bar
        // that has been scrolled under is indistinguishable from a plain one.
        let theme = crate::theme::ThemeData::light();
        let medium = config(SliverAppVariant::Medium);
        let large = config(SliverAppVariant::Large);
        assert_eq!(
            large.expanded_text_style.as_ref().map(|s| s.font_size),
            theme
                .text_theme
                .headline_medium
                .as_ref()
                .map(|s| s.font_size)
        );
        assert_eq!(
            medium.expanded_text_style.as_ref().map(|s| s.font_size),
            theme
                .text_theme
                .headline_small
                .as_ref()
                .map(|s| s.font_size)
        );
        assert!(
            large.expanded_text_style.as_ref().unwrap().font_size
                > medium.expanded_text_style.as_ref().unwrap().font_size
        );
        assert_eq!(
            medium.collapsed_text_style.as_ref().map(|s| s.font_size),
            theme.text_theme.title_large.as_ref().map(|s| s.font_size)
        );
    }

    #[test]
    fn the_bars_own_foreground_is_what_the_title_is_written_in() {
        // Upstream's chain ends `config.expandedTextStyle?.copyWith(color:
        // foregroundColor ?? appBarTheme.foregroundColor ?? defaults)`, so the
        // `apply(color: onSurface)` inside each config is a value the next
        // line overwrites. They agree under the Material 3 default and stop
        // agreeing the moment a bar names a foreground -- which is when it
        // matters.
        const MINE: crate::engine::Color = crate::engine::Color::argb(0xFF, 0x11, 0x22, 0x33);
        let theme = crate::theme::ThemeData::light();
        let named = ScrollUnderConfig::of(SliverAppVariant::Large, &theme, MINE).unwrap();
        assert_eq!(named.expanded_text_style.as_ref().unwrap().color, MINE);
        assert_eq!(named.collapsed_text_style.as_ref().unwrap().color, MINE);
        assert_ne!(
            theme.text_theme.headline_medium.as_ref().unwrap().color,
            MINE,
            "which the role does not carry, so this says the merge happened"
        );
    }

    #[test]
    fn a_bar_with_something_under_it_loses_the_padding_beneath_its_title() {
        // The tab strip or search field brings its own room; keeping the
        // twenty pixels as well would push the title up off the bar it
        // belongs to. The sides are untouched -- it is only the bottom that
        // the thing below has already paid for.
        let medium = config(SliverAppVariant::Medium);
        assert_eq!(medium.title_padding_over(0.0).bottom, 20.0);
        assert_eq!(medium.title_padding_over(48.0).bottom, 0.0);
        assert_eq!(
            medium.title_padding_over(48.0).left,
            medium.expanded_title_padding.left
        );
    }

    #[test]
    fn a_bars_bottom_makes_it_taller_open_and_shut_alike() {
        // Upstream adds `bottomHeight` to *both* extents. A bar with a tab
        // strip is that much taller expanded and that much taller collapsed,
        // so the strip stays on screen the whole way down -- which is the
        // point of putting it in the bar rather than under it.
        let mut bar = SliverAppBar::new();
        bar.variant = SliverAppVariant::Medium;
        let (shut, open) = bar.variant_extents().expect("a medium bar");
        assert_eq!((shut, open), (64.0, 112.0));

        bar.bottom_height = 48.0;
        bar.has_bottom = true;
        let (shut_with, open_with) = bar.variant_extents().expect("a medium bar");
        assert_eq!(shut_with - shut, 48.0);
        assert_eq!(open_with - open, 48.0);
    }

    #[test]
    fn the_status_bars_room_is_counted_once_and_into_the_collapsed_height() {
        // It is already inside `min_extent` by way of `top_padding`, so
        // adding it to the expanded height as well would count it twice.
        let mut bar = SliverAppBar::new();
        bar.variant = SliverAppVariant::Large;
        let (shut, open) = bar.variant_extents().expect("a large bar");
        bar.top_padding = 24.0;
        let (shut_under, open_under) = bar.variant_extents().expect("a large bar");
        assert_eq!(shut_under - shut, 24.0);
        assert_eq!(open_under, open, "and not twice");
    }

    #[test]
    fn a_named_height_beats_the_variants() {
        let mut bar = SliverAppBar::new();
        bar.variant = SliverAppVariant::Large;
        bar.collapsed_height = Some(70.0);
        bar.expanded_height = Some(200.0);
        assert_eq!(bar.variant_extents(), Some((70.0, 200.0)));
    }

    fn expanded() -> SliverAppBar {
        SliverAppBar {
            expanded_height: Some(200.0),
            ..SliverAppBar::new()
        }
    }

    // -- Accessibility is a different answer, not a dimmer setting ------------------

    #[test]
    fn collapsing_does_not_fade_the_toolbar_scrolling_off_does() {
        // A correction to my first guess. Through the whole shrink the toolbar
        // stays solid, because what the bar is losing is its expanded space and
        // the toolbar is exactly what is left at the bottom of that. The fade
        // belongs to the stretch after full collapse, where an unpinned bar
        // keeps going and leaves the top of the viewport.
        let bar = expanded();
        assert_eq!(bar.min_extent(), bar.toolbar_height);

        assert_eq!(bar.toolbar_opacity(0.0, false), 1.0);
        assert_eq!(
            bar.toolbar_opacity(bar.shrink_range(), false),
            1.0,
            "fully collapsed and still fully opaque"
        );
        assert!(
            bar.toolbar_opacity(bar.max_extent() - 20.0, false) < 1.0,
            "and only then does it start to go"
        );
        assert_eq!(bar.toolbar_opacity(bar.max_extent(), false), 0.0);
    }

    #[test]
    fn a_screen_reader_keeps_the_toolbar_fully_opaque_however_far_it_has_scrolled() {
        let bar = expanded();
        let nearly_gone = bar.max_extent() - 20.0;

        assert!(
            bar.toolbar_opacity(nearly_gone, false) < 1.0,
            "it does fade normally"
        );
        assert_eq!(
            bar.toolbar_opacity(nearly_gone, true),
            1.0,
            "but a control that can be reached must be visible"
        );
    }

    #[test]
    fn the_fade_is_off_at_every_offset_rather_than_scaled() {
        let bar = expanded();
        for step in 0..=10 {
            let offset = bar.max_extent() * (step as f32 / 10.0);
            assert_eq!(bar.toolbar_opacity(offset, true), 1.0, "at {offset}");
        }
    }

    // -- Pinned does not fade, except in one arrangement ----------------------------

    #[test]
    fn a_pinned_bar_stays_opaque_because_it_is_always_there() {
        let mut bar = expanded();
        bar.pinned = true;
        assert!(!bar.is_pinned_with_opacity_fade());
        assert_eq!(bar.toolbar_opacity(bar.shrink_range(), false), 1.0);
    }

    #[test]
    fn the_one_arrangement_where_a_pinned_bar_does_fade_needs_all_four_conditions() {
        let mut bar = expanded();
        bar.pinned = true;
        bar.floating = true;
        bar.has_bottom = true;
        assert!(bar.is_pinned_with_opacity_fade());

        for undo in 0..3 {
            let mut broken = bar;
            match undo {
                0 => broken.pinned = false,
                1 => broken.floating = false,
                _ => broken.has_bottom = false,
            }
            assert!(!broken.is_pinned_with_opacity_fade(), "case {undo}");
        }
    }

    #[test]
    fn the_bottom_is_exactly_the_part_that_stays_when_the_toolbar_goes() {
        let mut bar = expanded();
        bar.pinned = true;
        bar.floating = true;
        bar.has_bottom = true;
        bar.bottom_height = 48.0;

        let far = bar.max_extent() - 10.0;
        assert!(bar.toolbar_opacity(far, false) < 1.0, "the toolbar fades");
        assert_eq!(bar.bottom_opacity(far), 1.0, "and the bottom does not");
    }

    // -- The floor that stops the header inverting ----------------------------------

    #[test]
    fn an_expanded_height_below_the_collapsed_one_does_not_invert_the_bar() {
        let mut silly = SliverAppBar::new();
        silly.collapsed_height = Some(120.0);
        silly.expanded_height = Some(60.0);

        assert_eq!(silly.min_extent(), 120.0);
        assert_eq!(silly.max_extent(), 120.0, "floored, not 60");
        assert_eq!(silly.shrink_range(), 0.0, "so nothing runs backwards");
    }

    #[test]
    fn the_current_extent_never_goes_below_the_collapsed_height() {
        let bar = expanded();
        assert_eq!(bar.current_extent(0.0), 200.0);
        assert_eq!(bar.current_extent(1000.0), bar.min_extent());
    }

    // -- Scrolled under -------------------------------------------------------------

    #[test]
    fn a_pinned_bar_notices_when_it_has_run_out_of_room_to_shrink() {
        let mut bar = expanded();
        bar.pinned = true;
        assert!(!bar.is_scrolled_under(bar.shrink_range() - 1.0, false));
        assert!(bar.is_scrolled_under(bar.shrink_range() + 1.0, false));
    }

    #[test]
    fn an_unpinned_bar_only_notices_by_being_told() {
        let bar = expanded();
        assert!(!bar.is_scrolled_under(bar.shrink_range() + 100.0, false));
        assert!(bar.is_scrolled_under(0.0, true), "overlapsContent");

        let mut forced = bar;
        forced.force_elevated = true;
        assert!(forced.is_scrolled_under(0.0, false));
    }

    // -- What the constructor refuses ------------------------------------------------

    #[test]
    fn snapping_only_makes_sense_for_a_bar_that_floats() {
        let mut bar = SliverAppBar::new();
        bar.snap = true;
        assert_eq!(bar.validate(), Err(SliverAppBarError::SnapWithoutFloating));

        bar.floating = true;
        assert_eq!(bar.validate(), Ok(()));

        // The implication runs one way only.
        let mut floating_only = SliverAppBar::new();
        floating_only.floating = true;
        assert_eq!(floating_only.validate(), Ok(()));
    }

    #[test]
    fn a_stretch_trigger_has_to_be_a_distance() {
        let mut bar = SliverAppBar::new();
        bar.stretch_trigger_offset = 0.0;
        assert_eq!(
            bar.validate(),
            Err(SliverAppBarError::NonPositiveStretchTrigger)
        );
        bar.stretch_trigger_offset = 0.5;
        assert_eq!(bar.validate(), Ok(()));
    }

    #[test]
    fn the_collapsed_height_may_equal_the_toolbar_but_not_undercut_it() {
        let mut bar = SliverAppBar::new();
        bar.collapsed_height = Some(TOOLBAR_HEIGHT);
        assert_eq!(bar.validate(), Ok(()));
        bar.collapsed_height = Some(TOOLBAR_HEIGHT - 1.0);
        assert_eq!(
            bar.validate(),
            Err(SliverAppBarError::CollapsedHeightBelowToolbar)
        );
    }

    #[test]
    fn the_larger_variants_bring_their_title_in_on_scroll_rather_than_showing_it() {
        assert!(!SliverAppBar::new().title_follows_scrolled_under());
        for variant in [SliverAppVariant::Medium, SliverAppVariant::Large] {
            let bar = SliverAppBar {
                variant,
                ..SliverAppBar::new()
            };
            assert!(bar.title_follows_scrolled_under(), "{variant:?}");
        }
    }
}
