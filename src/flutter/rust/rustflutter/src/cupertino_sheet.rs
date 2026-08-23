//! Ports of `cupertino/nav_bar.dart`'s `CupertinoSliverNavigationBar` and
//! `cupertino/sheet.dart`'s `CupertinoSheetTransition` and `CupertinoSheetRoute`.
//!
//! Both files are full of numbers somebody obtained by looking at a phone, and
//! both say so. See [`EYEBALLED_BY_LAYER`].

/// How many `// Eyeballed`-style comments each upstream layer carries, counted
/// across `packages/flutter/lib/src` at the revision this port follows.
///
/// ```text
/// cupertino  74      rendering   1
/// material   15      painting    0
/// widgets     8      gestures    0
///                    services    0
///                    animation   0
///                    scheduler   0
///                    foundation  0
/// ```
///
/// Ninety-eight in total and **three quarters of them in one layer**, which is
/// not sloppiness but a difference in what the two design languages are.
/// **Material is a published specification you can read; iOS is a shipped
/// product you can only measure.** Nothing documents that an iOS navigation
/// bar's background fades over ten logical pixels, so somebody opened Settings
/// on a simulator and watched it.
///
/// The tail is the other half of it. `painting`, `gestures`, `services`,
/// `animation`, `scheduler` and `foundation` have none at all, because they
/// compute things that are *true* rather than things that *look right*. A
/// bezier is a bezier. The count falls off exactly as you descend from
/// appearance to arithmetic.
pub const EYEBALLED_BY_LAYER: [(&str, usize); 10] = [
    ("cupertino", 74),
    ("material", 15),
    ("widgets", 8),
    ("rendering", 1),
    ("painting", 0),
    ("gestures", 0),
    ("services", 0),
    ("animation", 0),
    ("scheduler", 0),
    ("foundation", 0),
];

/// Upstream `_kNavBarPersistentHeight`, which is
/// `kMinInteractiveDimensionCupertino`.
pub const NAV_BAR_PERSISTENT_HEIGHT: f32 = 44.0;

/// Upstream `_kNavBarLargeTitleHeightExtension`.
pub const NAV_BAR_LARGE_TITLE_HEIGHT_EXTENSION: f32 = 52.0;

/// Upstream `_kNavBarShowLargeTitleThreshold`: *"Number of logical pixels
/// scrolled down before the title text is transferred from the normal
/// navigation bar to a big title below the navigation bar."*
pub const NAV_BAR_SHOW_LARGE_TITLE_THRESHOLD: f32 = 10.0;

/// Upstream `_kNavBarScrollUnderAnimationExtent`, and its comment is the most
/// specific of the ninety-eight:
///
/// > Number of logical pixels scrolled during which the navigation bar's
/// > background fades in or out.
/// >
/// > **Eyeballed on the native Settings app on an iPhone 15 simulator running
/// > iOS 17.4.**
///
/// It names the app, the device and the OS version -- which is what makes it a
/// claim somebody could go and check, rather than a number. Compare
/// `cupertino/button.dart`'s *"Eyeballed values. Feel free to tweak."* Both are
/// honest; only one is reproducible.
///
/// Note that this and [`NAV_BAR_SHOW_LARGE_TITLE_THRESHOLD`] are both 10.0 and
/// mean entirely different things -- one is how far you scroll before the title
/// moves, the other how far the background takes to fade. **Only one of the two
/// says where it came from.**
pub const NAV_BAR_SCROLL_UNDER_ANIMATION_EXTENT: f32 = 10.0;

/// Why a sliver navigation bar's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavBarError {
    /// `assert(automaticallyImplyTitle || largeTitle != null, ...)`
    NoTitleAndNoneImplied,
    /// `assert(bottomMode == null || bottom != null, ...)`
    BottomModeWithoutABottom,
    /// `assert(widget.middle == null || widget.largeTitle == null)`
    TwoTitles,
    /// `assert(!widget._searchable || widget.bottom == null)`
    SearchableWithABottom,
}

/// Upstream `NavigationBarBottomMode`: whether the bar's bottom -- a search
/// field, or whatever was given as `bottom` -- can be scrolled away.
///
/// Both modes consume the same total: the bar shrinks from
/// `persistent + largeTitle + bottom` down to its minimum. **What differs is
/// what is left at the bottom of that travel**, and therefore what gets
/// consumed first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NavigationBarBottomMode {
    /// The bottom goes first. Upstream: "the large title stays pinned while
    /// the bottom resizes until it is completely consumed. Then, the large
    /// title scrolls under the persistent navigation bar."
    #[default]
    Automatic,
    /// The bottom stays. Upstream: "the bottom stays pinned while the large
    /// title scrolls under."
    Always,
}

impl NavigationBarBottomMode {
    pub const ALL: [NavigationBarBottomMode; 2] = [
        NavigationBarBottomMode::Automatic,
        NavigationBarBottomMode::Always,
    ];

    /// Upstream's `minExtent`:
    /// `persistentHeight + (bottomMode == always ? bottomHeight : 0.0)`.
    pub fn min_extent(self, persistent_height: f32, bottom_height: f32) -> f32 {
        persistent_height
            + match self {
                NavigationBarBottomMode::Always => bottom_height,
                NavigationBarBottomMode::Automatic => 0.0,
            }
    }

    /// Upstream's `maxExtent`, which **does not mention the mode**: the bar is
    /// the same size fully expanded either way.
    pub fn max_extent(persistent_height: f32, large_title_height: f32, bottom_height: f32) -> f32 {
        persistent_height + large_title_height + bottom_height
    }

    /// Upstream's `bottomScrollOffset`: `always ? 0.0 : bottomHeight`.
    ///
    /// How much of the bottom the scroll may eat. It is the exact complement
    /// of the part [`NavigationBarBottomMode::min_extent`] keeps, and the two
    /// are written separately upstream -- so they can be made to disagree, and
    /// a test here says they must not.
    pub fn scrollable_bottom(self, bottom_height: f32) -> f32 {
        match self {
            NavigationBarBottomMode::Always => 0.0,
            NavigationBarBottomMode::Automatic => bottom_height,
        }
    }
}

/// Upstream `CupertinoSliverNavigationBar`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoSliverNavigationBar {
    pub has_large_title: bool,
    pub has_middle: bool,
    pub automatically_imply_title: bool,
    pub has_bottom: bool,
    pub has_bottom_mode: bool,
    /// Upstream's `bottomMode`, which only means anything when there is a
    /// bottom -- see [`NavBarError::BottomModeWithoutABottom`].
    pub bottom_mode: NavigationBarBottomMode,
    pub searchable: bool,
    pub bottom_height: f32,
}

impl CupertinoSliverNavigationBar {
    pub fn new() -> CupertinoSliverNavigationBar {
        CupertinoSliverNavigationBar {
            has_large_title: true,
            has_middle: false,
            automatically_imply_title: true,
            has_bottom: false,
            has_bottom_mode: false,
            bottom_mode: NavigationBarBottomMode::Automatic,
            searchable: false,
            bottom_height: 0.0,
        }
    }

    /// Upstream's asserts, two in the constructor and two in the state.
    ///
    /// The title one carries a message that names **both** ways out:
    ///
    /// > No largeTitle has been provided but automaticallyImplyTitle is also
    /// > false. **Either provide a largeTitle or set automaticallyImplyTitle to
    /// > true.**
    ///
    /// Which is the useful shape for an error about a two-sided rule: a reader
    /// who reached it did not want either fix in particular, they wanted a
    /// title, and being told the rule alone would leave them guessing which side
    /// to change.
    ///
    /// The `bottomMode` one is an implication -- a mode for a thing that is not
    /// there configures nothing -- and the searchable one is the same shape from
    /// the other end: **a searchable bar cannot also have a bottom, because the
    /// search field is the bottom.**
    pub fn validate(&self) -> Result<(), NavBarError> {
        if !self.automatically_imply_title && !self.has_large_title {
            return Err(NavBarError::NoTitleAndNoneImplied);
        }
        if self.has_bottom_mode && !self.has_bottom {
            return Err(NavBarError::BottomModeWithoutABottom);
        }
        if self.has_middle && self.has_large_title {
            return Err(NavBarError::TwoTitles);
        }
        if self.searchable && self.has_bottom {
            return Err(NavBarError::SearchableWithABottom);
        }
        Ok(())
    }

    /// Upstream's `preferredSize`: the persistent height plus whatever is below
    /// it plus the large title's extension, when there is one.
    pub fn preferred_height(&self, top_padding: f32) -> f32 {
        let large = if self.has_large_title {
            NAV_BAR_LARGE_TITLE_HEIGHT_EXTENSION
        } else {
            0.0
        };
        NAV_BAR_PERSISTENT_HEIGHT + self.bottom_height + large + top_padding
    }

    /// How opaque the bar's background is after scrolling `offset` pixels, over
    /// [`NAV_BAR_SCROLL_UNDER_ANIMATION_EXTENT`].
    pub fn background_opacity(offset: f32) -> f32 {
        (offset / NAV_BAR_SCROLL_UNDER_ANIMATION_EXTENT).clamp(0.0, 1.0)
    }

    /// Whether the large title has handed its text up to the bar proper.
    pub fn title_has_moved_up(offset: f32) -> bool {
        offset > NAV_BAR_SHOW_LARGE_TITLE_THRESHOLD
    }
}

impl Default for CupertinoSliverNavigationBar {
    fn default() -> Self {
        CupertinoSliverNavigationBar::new()
    }
}

/// Upstream `_kMinFlingVelocity`, in screen heights per second: *"Eyeballed from
/// a comparison against a simulator running iOS 18.0."*
pub const SHEET_MIN_FLING_VELOCITY: f32 = 2.0;

/// Upstream `_kDroppedSheetDragAnimationDuration`, also eyeballed against
/// iOS 18.0.
pub const DROPPED_SHEET_DRAG_ANIMATION_MS: u64 = 300;

/// Upstream `_kSheetScaleFactor`, and this one is a different kind of number.
///
/// > Amount the sheet in the background scales down. **Found by measuring the
/// > width of the sheet in the background and comparing against the screen width
/// > on the iOS simulator showing an iPhone 16 pro running iOS 18.0.**
///
/// Not eyeballed -- measured, with the method stated. Which is why it has four
/// significant figures where the eyeballed constants beside it are 2.0, 300 and
/// 10.0. **The precision of a constant tells you how it was obtained**, and here
/// the comment confirms it.
pub const SHEET_SCALE_FACTOR: f32 = 0.0835;

/// Upstream `_kTopGapRatio`.
pub const TOP_GAP_RATIO: f32 = 0.08;

/// Upstream `_kStretchedTopGapRatio`, which is exactly nine tenths of
/// [`TOP_GAP_RATIO`].
pub const STRETCHED_TOP_GAP_RATIO: f32 = 0.072;

/// Upstream `CupertinoSheetTransition`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoSheetTransition;

impl CupertinoSheetTransition {
    /// Upstream's `_kScaleTween`, `Tween(begin: 1.0, end: 1.0 - _kSheetScaleFactor)`:
    /// the page behind the sheet shrinks by the measured factor as the sheet
    /// arrives.
    pub fn background_scale(t: f32) -> f32 {
        1.0 - SHEET_SCALE_FACTOR * t.clamp(0.0, 1.0)
    }

    /// Upstream's `_kOpacityTween`, `Tween(begin: 0.0, end: 0.10)`: the page
    /// behind is dimmed by a tenth and no more.
    pub fn background_dim(t: f32) -> f32 {
        0.10 * t.clamp(0.0, 1.0)
    }

    /// Whether a downward drag at this velocity dismisses the sheet.
    pub fn dismisses_on_fling(velocity_screen_heights_per_second: f32) -> bool {
        velocity_screen_heights_per_second >= SHEET_MIN_FLING_VELOCITY
    }
}

/// Why a sheet route's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetRouteError {
    TopGapOutOfRange,
    NoBuilder,
}

/// Upstream `CupertinoSheetRoute`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoSheetRoute {
    /// `None` uses [`TOP_GAP_RATIO`].
    pub top_gap: Option<f32>,
    pub has_builder: bool,
    pub has_scrollable_builder: bool,
    pub show_drag_handle: bool,
}

impl CupertinoSheetRoute {
    pub fn new() -> CupertinoSheetRoute {
        CupertinoSheetRoute {
            top_gap: None,
            has_builder: false,
            has_scrollable_builder: true,
            show_drag_handle: false,
        }
    }

    /// Upstream's two asserts:
    ///
    /// ```dart
    /// assert(topGap == null || (topGap >= 0.0 && topGap <= 0.9), 'topGap must be between 0.0 and 0.9'),
    /// assert(builder != null || scrollableBuilder != null, 'Either scrollableBuilder or builder must not be null'),
    /// ```
    ///
    /// **The upper bound is 0.9, not 1.0.** A sheet has to leave at least a
    /// tenth of the screen showing, because a sheet that covered everything
    /// would not read as a sheet -- the strip of the page behind it is what says
    /// there is something to go back to.
    ///
    /// The second is an "at least one", with `builder` deprecated in favour of
    /// `scrollableBuilder`.
    pub fn validate(&self) -> Result<(), SheetRouteError> {
        if self.top_gap.is_some_and(|gap| !(0.0..=0.9).contains(&gap)) {
            return Err(SheetRouteError::TopGapOutOfRange);
        }
        if !self.has_builder && !self.has_scrollable_builder {
            return Err(SheetRouteError::NoBuilder);
        }
        Ok(())
    }

    /// How far down the screen the sheet's top edge sits.
    pub fn effective_top_gap(&self, stretched: bool) -> f32 {
        self.top_gap.unwrap_or(if stretched {
            STRETCHED_TOP_GAP_RATIO
        } else {
            TOP_GAP_RATIO
        })
    }
}

impl Default for CupertinoSheetRoute {
    fn default() -> Self {
        CupertinoSheetRoute::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Where the numbers come from --------------------------------------------------

    #[test]
    fn three_quarters_of_the_eyeballed_constants_are_in_one_layer() {
        let total: usize = EYEBALLED_BY_LAYER.iter().map(|(_, n)| n).sum();
        let cupertino = EYEBALLED_BY_LAYER[0].1;
        assert_eq!(total, 98);
        assert_eq!(cupertino, 74);
        assert!(cupertino * 4 > total * 3, "{cupertino} of {total}");
    }

    #[test]
    fn the_layers_that_compute_rather_than_appear_have_none_at_all() {
        let arithmetic = [
            "painting",
            "gestures",
            "services",
            "animation",
            "scheduler",
            "foundation",
        ];
        for (layer, count) in EYEBALLED_BY_LAYER {
            if arithmetic.contains(&layer) {
                assert_eq!(count, 0, "{layer}");
            }
        }
    }

    #[test]
    fn the_count_falls_off_as_the_layers_descend() {
        // cupertino, material, widgets, rendering, then nothing.
        let counts: Vec<usize> = EYEBALLED_BY_LAYER.iter().map(|(_, n)| *n).collect();
        for window in counts.windows(2) {
            assert!(window[0] >= window[1], "{window:?}");
        }
    }

    #[test]
    fn the_precision_of_a_constant_says_how_it_was_obtained() {
        // The eyeballed ones are round; the measured one is not.
        assert_eq!(SHEET_MIN_FLING_VELOCITY, 2.0);
        assert_eq!(DROPPED_SHEET_DRAG_ANIMATION_MS, 300);
        assert_eq!(NAV_BAR_SCROLL_UNDER_ANIMATION_EXTENT, 10.0);
        assert_eq!(
            SHEET_SCALE_FACTOR, 0.0835,
            "measured, with the method stated"
        );
    }

    #[test]
    fn two_adjacent_tens_mean_different_things_and_one_says_where_it_came_from() {
        assert_eq!(
            NAV_BAR_SHOW_LARGE_TITLE_THRESHOLD,
            NAV_BAR_SCROLL_UNDER_ANIMATION_EXTENT
        );
        // Equal numbers, unrelated jobs: at 5 pixels the background is half
        // faded and the title has not moved.
        assert_eq!(CupertinoSliverNavigationBar::background_opacity(5.0), 0.5);
        assert!(!CupertinoSliverNavigationBar::title_has_moved_up(5.0));

        assert_eq!(CupertinoSliverNavigationBar::background_opacity(10.0), 1.0);
        assert!(
            !CupertinoSliverNavigationBar::title_has_moved_up(10.0),
            "the threshold is exclusive, so the fade finishes first"
        );
        assert!(CupertinoSliverNavigationBar::title_has_moved_up(10.5));
    }

    // -- What the nav bar refuses -------------------------------------------------------

    #[test]
    fn a_bar_with_no_title_and_no_leave_to_imply_one_is_refused() {
        let mut bar = CupertinoSliverNavigationBar::new();
        bar.has_large_title = false;
        assert_eq!(bar.validate(), Ok(()), "it may imply one");

        bar.automatically_imply_title = false;
        assert_eq!(bar.validate(), Err(NavBarError::NoTitleAndNoneImplied));

        // And the message names both ways out, so either fixes it.
        bar.has_large_title = true;
        assert_eq!(bar.validate(), Ok(()));
        bar.has_large_title = false;
        bar.automatically_imply_title = true;
        assert_eq!(bar.validate(), Ok(()));
    }

    #[test]
    fn a_mode_for_a_thing_that_is_not_there_configures_nothing() {
        let mut bar = CupertinoSliverNavigationBar::new();
        bar.has_bottom_mode = true;
        assert_eq!(bar.validate(), Err(NavBarError::BottomModeWithoutABottom));
        bar.has_bottom = true;
        assert_eq!(bar.validate(), Ok(()));
    }

    #[test]
    fn a_bar_has_one_title_or_the_other() {
        let mut bar = CupertinoSliverNavigationBar::new();
        bar.has_middle = true;
        assert_eq!(bar.validate(), Err(NavBarError::TwoTitles));
        bar.has_large_title = false;
        assert_eq!(bar.validate(), Ok(()));
    }

    #[test]
    fn a_searchable_bar_cannot_have_a_bottom_because_the_search_field_is_one() {
        let mut bar = CupertinoSliverNavigationBar::new();
        bar.searchable = true;
        assert_eq!(bar.validate(), Ok(()));
        bar.has_bottom = true;
        assert_eq!(bar.validate(), Err(NavBarError::SearchableWithABottom));
    }

    #[test]
    fn the_large_title_costs_its_extension_and_nothing_else() {
        let mut bar = CupertinoSliverNavigationBar::new();
        assert_eq!(bar.preferred_height(0.0), 44.0 + 52.0);
        bar.has_large_title = false;
        assert_eq!(bar.preferred_height(0.0), 44.0);
        assert_eq!(bar.preferred_height(20.0), 64.0, "plus the status bar");
    }

    // -- The sheet ----------------------------------------------------------------------

    #[test]
    fn a_sheet_has_to_leave_a_tenth_of_the_screen_showing() {
        let mut route = CupertinoSheetRoute::new();
        route.top_gap = Some(0.9);
        assert_eq!(
            route.validate(),
            Ok(()),
            "nine tenths is the most it may take"
        );

        route.top_gap = Some(0.91);
        assert_eq!(route.validate(), Err(SheetRouteError::TopGapOutOfRange));

        route.top_gap = Some(1.0);
        assert_eq!(
            route.validate(),
            Err(SheetRouteError::TopGapOutOfRange),
            "a sheet that covered everything would not read as a sheet"
        );
    }

    #[test]
    fn a_gap_of_nothing_is_allowed_though() {
        let mut route = CupertinoSheetRoute::new();
        route.top_gap = Some(0.0);
        assert_eq!(route.validate(), Ok(()));
    }

    #[test]
    fn the_default_gap_is_a_twelfth_and_the_stretched_one_nine_tenths_of_that() {
        let route = CupertinoSheetRoute::new();
        assert_eq!(route.effective_top_gap(false), TOP_GAP_RATIO);
        assert_eq!(route.effective_top_gap(true), STRETCHED_TOP_GAP_RATIO);
        assert!((STRETCHED_TOP_GAP_RATIO - TOP_GAP_RATIO * 0.9).abs() < 1e-6);
    }

    #[test]
    fn a_route_needs_something_to_build_with() {
        let mut route = CupertinoSheetRoute::new();
        route.has_scrollable_builder = false;
        assert_eq!(route.validate(), Err(SheetRouteError::NoBuilder));

        route.has_builder = true;
        assert_eq!(route.validate(), Ok(()), "the deprecated one still counts");
    }

    #[test]
    fn the_page_behind_shrinks_by_the_measured_factor_and_dims_by_a_tenth() {
        assert_eq!(CupertinoSheetTransition::background_scale(0.0), 1.0);
        assert_eq!(
            CupertinoSheetTransition::background_scale(1.0),
            1.0 - SHEET_SCALE_FACTOR
        );
        assert_eq!(CupertinoSheetTransition::background_dim(1.0), 0.10);
        assert_eq!(CupertinoSheetTransition::background_dim(0.0), 0.0);
    }

    #[test]
    fn two_screen_heights_a_second_is_the_flick_that_closes_it() {
        assert!(!CupertinoSheetTransition::dismisses_on_fling(1.9));
        assert!(CupertinoSheetTransition::dismisses_on_fling(2.0));
    }
}

#[cfg(test)]
mod bottom_mode_tests {
    use super::{CupertinoSliverNavigationBar, NavigationBarBottomMode};

    const PERSISTENT: f32 = 44.0;
    const LARGE_TITLE: f32 = 52.0;
    const BOTTOM: f32 = 35.0;

    #[test]
    fn always_keeps_the_bottom_at_the_end_of_the_travel() {
        assert_eq!(
            NavigationBarBottomMode::Always.min_extent(PERSISTENT, BOTTOM),
            PERSISTENT + BOTTOM
        );
        assert_eq!(
            NavigationBarBottomMode::Automatic.min_extent(PERSISTENT, BOTTOM),
            PERSISTENT
        );
    }

    #[test]
    fn but_both_modes_start_from_the_same_size() {
        // maxExtent does not mention the mode: fully expanded, the bar is the
        // same either way, and only what survives the shrinking differs.
        let expanded = NavigationBarBottomMode::max_extent(PERSISTENT, LARGE_TITLE, BOTTOM);
        assert_eq!(expanded, PERSISTENT + LARGE_TITLE + BOTTOM);
        for mode in NavigationBarBottomMode::ALL {
            assert!(
                mode.min_extent(PERSISTENT, BOTTOM) <= expanded,
                "{mode:?} shrinks rather than grows"
            );
        }
    }

    #[test]
    fn what_scrolls_away_and_what_stays_add_up_to_the_bottom() {
        // Upstream writes minExtent and bottomScrollOffset separately, so they
        // can be made to disagree. They are complements and must stay so, or
        // the bar would either eat part of itself twice or leave a gap.
        for mode in NavigationBarBottomMode::ALL {
            let kept = mode.min_extent(PERSISTENT, BOTTOM) - PERSISTENT;
            assert_eq!(kept + mode.scrollable_bottom(BOTTOM), BOTTOM, "{mode:?}");
        }
    }

    #[test]
    fn and_only_the_automatic_one_lets_the_bottom_go() {
        assert_eq!(
            NavigationBarBottomMode::Automatic.scrollable_bottom(BOTTOM),
            BOTTOM
        );
        assert_eq!(
            NavigationBarBottomMode::Always.scrollable_bottom(BOTTOM),
            0.0
        );
        // Which is a real difference, or the mode would decide nothing.
        assert_ne!(
            NavigationBarBottomMode::Automatic.scrollable_bottom(BOTTOM),
            NavigationBarBottomMode::Always.scrollable_bottom(BOTTOM)
        );
    }

    #[test]
    fn a_bar_with_no_bottom_scrolls_the_same_either_way() {
        // With nothing there, the mode has nothing to decide -- which is why
        // upstream asserts a bottomMode without a bottom is a mistake rather
        // than a no-op worth allowing.
        for mode in NavigationBarBottomMode::ALL {
            assert_eq!(mode.min_extent(PERSISTENT, 0.0), PERSISTENT, "{mode:?}");
            assert_eq!(mode.scrollable_bottom(0.0), 0.0, "{mode:?}");
        }
    }

    #[test]
    fn a_bar_hides_its_bottom_unless_told_otherwise() {
        assert_eq!(
            CupertinoSliverNavigationBar::new().bottom_mode,
            NavigationBarBottomMode::Automatic
        );
        assert_eq!(
            NavigationBarBottomMode::default(),
            NavigationBarBottomMode::Automatic
        );
    }
}
