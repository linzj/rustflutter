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
