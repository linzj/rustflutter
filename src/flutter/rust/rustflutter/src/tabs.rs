//! Ports of `material/tab_controller.dart`, `material/tabs.dart` and
//! `material/tab_indicator.dart`.
//!
//! A tab selection can move two ways -- somebody taps a tab, or somebody drags
//! the pages -- and almost everything here is about keeping those two apart.

/// Upstream `kTabScrollDuration`, in milliseconds.
pub const TAB_SCROLL_DURATION_MS: u64 = 300;

/// Upstream `_kTabHeight`: a tab with only a label.
pub const TAB_HEIGHT: f32 = 46.0;

/// Upstream `_kTextAndIconTabHeight`: a tab with both.
pub const TEXT_AND_ICON_TAB_HEIGHT: f32 = 72.0;

/// Upstream `TabController`.
#[derive(Clone, Debug, PartialEq)]
pub struct TabController {
    length: usize,
    index: usize,
    previous_index: usize,
    /// Upstream's `_indexIsChangingCount`. **A counter, not a flag** -- see
    /// [`TabController::index_is_changing`].
    index_is_changing_count: i32,
    /// Upstream's `_animationController.value`. The index and this are two views
    /// of the same number; [`TabController::offset`] is the difference.
    animation_value: f32,
    disposed: bool,
}

/// Why a call to change the index did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOutcome {
    Changed,
    /// Already there.
    AlreadySelected,
    /// Upstream's `length < 2`. See [`TabController::change_index`].
    NothingToSwitchBetween,
}

impl TabController {
    /// Upstream asserts `length >= 0` -- **zero tabs is legal** -- and then:
    ///
    /// ```dart
    /// assert(initialIndex >= 0 && (length == 0 || initialIndex < length)),
    /// ```
    ///
    /// The doc comment three lines above it says *"If `length` is zero, then
    /// `initialIndex` must be 0 (the default)."* **The assert does not say
    /// that.** Its `length == 0` term switches the range check off entirely, so
    /// a zero-tab controller reporting index 47 passes, and nothing downstream
    /// ever repairs the value -- [`TabController::change_index`] only stops it
    /// from moving.
    ///
    /// Ported as the assert behaves, since the assert is what runs.
    pub fn new(length: usize, initial_index: usize) -> Result<TabController, &'static str> {
        if length != 0 && initial_index >= length {
            return Err("initialIndex must be valid given length");
        }
        Ok(TabController {
            length,
            index: initial_index,
            previous_index: initial_index,
            index_is_changing_count: 0,
            animation_value: initial_index as f32,
            disposed: false,
        })
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Upstream: *"Initially the same as `index`."*
    pub fn previous_index(&self) -> usize {
        self.previous_index
    }

    /// Upstream: *"True while we're animating from `previousIndex` to `index` as
    /// a consequence of calling `animateTo`. [...] It is false when `offset` is
    /// changing as a consequence of the user dragging."*
    ///
    /// **The flag is named for the cause, not for the fact.** During a drag the
    /// index really does change, and this stays false the whole way -- because
    /// what callers need to know is not *whether* the selection is moving but
    /// *which of the two ways* it is moving, and the arithmetic differs. See
    /// [`TabPageSelector::indicator_brightness`].
    ///
    /// It is backed by a count rather than a bool because the animations
    /// overlap: tap tab 3 while the move to tab 2 is still running and there are
    /// two changes in flight. A bool would be cleared by the first completion
    /// while the second was still going.
    pub fn index_is_changing(&self) -> bool {
        self.index_is_changing_count != 0
    }

    /// Upstream `_changeIndex`.
    ///
    /// The early return is `value == _index || length < 2`, and the second half
    /// of it is doing more than it looks like.
    ///
    /// Upstream's own assert here is
    /// `assert(value >= 0 && (value < length || length == 0))` -- and when the
    /// length is zero that `|| length == 0` **switches the range check off
    /// altogether**, so any non-negative index passes it. What actually stops a
    /// zero-tab controller from accepting index 47 is `length < 2`, several
    /// lines further down. The guard that reads like an optimisation is the one
    /// holding the invariant.
    pub fn change_index(&mut self, value: usize, animated: bool) -> ChangeOutcome {
        if value == self.index {
            return ChangeOutcome::AlreadySelected;
        }
        if self.length < 2 {
            return ChangeOutcome::NothingToSwitchBetween;
        }
        self.previous_index = self.index;
        self.index = value;
        if animated {
            // Upstream notifies here, before the animation starts, with the
            // comment "Because the value of indexIsChanging may have changed":
            // the state change is announced separately from the value change.
            self.index_is_changing_count += 1;
        } else {
            // Upstream increments and decrements around a single synchronous
            // assignment. The count is never observably non-zero afterwards --
            // but the listeners woken by that assignment do see it set, which is
            // the whole reason for the pair.
            self.index_is_changing_count += 1;
            self.animation_value = self.index as f32;
            self.index_is_changing_count -= 1;
        }
        ChangeOutcome::Changed
    }

    /// Upstream `animateTo`, which is `_changeIndex` with a duration.
    pub fn animate_to(&mut self, value: usize) -> ChangeOutcome {
        self.change_index(value, true)
    }

    /// Upstream's `whenCompleteOrCancel` callback, which decrements the count
    /// **only if the controller was not disposed in the meantime** -- otherwise
    /// a finished animation would notify listeners of a dead controller.
    pub fn complete_animation(&mut self) {
        if self.disposed {
            return;
        }
        self.index_is_changing_count -= 1;
        self.animation_value = self.index as f32;
    }

    /// Upstream `offset`: `_animationController.value - _index`.
    ///
    /// Negative means the view has been dragged to the left, positive to the
    /// right, and it is never outside `[-1, 1]` because a drag can only ever
    /// reach an adjacent page.
    pub fn offset(&self) -> f32 {
        self.animation_value - self.index as f32
    }

    /// Upstream's setter asserts the range **and** `!indexIsChanging`.
    ///
    /// So the two ways of moving are not merely distinguished, they are mutually
    /// exclusive: **you cannot drag the pages while a tap's animation is still
    /// running.**
    pub fn set_offset(&mut self, value: f32) -> Result<(), &'static str> {
        if !(-1.0..=1.0).contains(&value) {
            return Err("offset must be between -1.0 and 1.0");
        }
        if self.index_is_changing() {
            return Err("cannot drag while a tap animation is running");
        }
        self.animation_value = value + self.index as f32;
        Ok(())
    }

    /// Upstream's `animation` getter returns null once disposed.
    pub fn animation(&self) -> Option<f32> {
        if self.disposed {
            None
        } else {
            Some(self.animation_value)
        }
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
    }

    /// Upstream `_copyWith`, used by `DefaultTabController` when its length
    /// changes: a new controller that **inherits the running animation
    /// controller** rather than building one, so the tabs do not jump.
    pub fn copy_with(&self, length: usize, index: Option<usize>) -> TabController {
        TabController {
            length,
            index: index.unwrap_or(self.index),
            previous_index: self.previous_index,
            index_is_changing_count: self.index_is_changing_count,
            animation_value: self.animation_value,
            disposed: false,
        }
    }
}

/// Upstream `DefaultTabController`: an inherited widget so descendants can find
/// a controller without being handed one.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultTabController {
    pub controller: TabController,
    pub child: u64,
}

impl DefaultTabController {
    pub fn new(length: usize, initial_index: usize, child: u64) -> Option<DefaultTabController> {
        TabController::new(length, initial_index)
            .ok()
            .map(|controller| DefaultTabController { controller, child })
    }

    /// Upstream's `didUpdateWidget` path: a changed length reuses the animation
    /// controller through `_copyWith`.
    pub fn update_length(&mut self, length: usize) {
        let index = if self.controller.index() >= length && length > 0 {
            Some(length - 1)
        } else {
            None
        };
        self.controller = self.controller.copy_with(length, index);
    }
}

/// Upstream `Tab`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tab {
    pub has_text: bool,
    pub has_child: bool,
    pub has_icon: bool,
    /// `None` computes the height from the content.
    pub height: Option<f32>,
}

impl Tab {
    /// Upstream's two constructor asserts, with their own messages:
    /// *"Tab requires at least one of text, child, or icon to be non-null"* and
    /// *"Provide either text or child, not both, when creating a Tab."*
    ///
    /// The second is the more interesting one: `text` is not a convenience that
    /// loses to `child`, it is an **alternative** to it, and offering both is an
    /// error rather than a precedence question.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.has_text && !self.has_child && !self.has_icon {
            return Err("Tab requires at least one of text, child, or icon to be non-null.");
        }
        if self.has_text && self.has_child {
            return Err("Provide either text or child, not both, when creating a Tab.");
        }
        Ok(())
    }

    /// Upstream's `preferredSize`: 72 when an icon shares the tab with a label,
    /// 46 otherwise. **An icon on its own is still 46** -- the taller row is for
    /// stacking two things, not for having an icon.
    pub fn preferred_height(&self) -> f32 {
        if let Some(height) = self.height {
            return height;
        }
        if self.has_icon && (self.has_text || self.has_child) {
            TEXT_AND_ICON_TAB_HEIGHT
        } else {
            TAB_HEIGHT
        }
    }
}

/// Upstream `TabBarScrollController`.
///
/// Its own comment says what it is for, and the whole class rests on one
/// ordering problem:
///
/// > This class, and `TabBarScrollController`, only exist to handle the case
/// > where a scrollable `TabBar` has a non-zero `initialIndex`. In that case we
/// > can only compute the scroll position's initial scroll offset (the
/// > "correct" pixels value) after the `TabBar` viewport width and scroll limits
/// > are known.
///
/// **You cannot know where to scroll to show tab five until you know how wide
/// the bar is, and you do not know that until layout has run.** So the position
/// is created with no pixels at all and corrects itself once the dimensions
/// arrive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabBarScrollController {
    pub attached: bool,
    viewport_dimension_was_non_zero: bool,
    needs_pixels_correction: bool,
}

impl TabBarScrollController {
    pub fn new() -> TabBarScrollController {
        TabBarScrollController {
            attached: false,
            viewport_dimension_was_non_zero: false,
            // Upstream: "The scroll position should be adjusted at least once."
            needs_pixels_correction: true,
        }
    }

    /// Upstream `debugCheckHasTabBarState`.
    pub fn debug_check_has_tab_bar_state(&self) -> bool {
        self.attached
    }

    /// Upstream `applyContentDimensions`, returning whether the pixels were
    /// corrected.
    ///
    /// The second half of the condition guards a transient: *"the viewport
    /// temporarily may have a dimension of zero before the actual dimension is
    /// calculated"*, and without the guard the super call would start a
    /// **ballistic scroll activity** from that bogus position -- a tab bar that
    /// visibly flings itself on first layout, and only in release builds.
    pub fn apply_content_dimensions(&mut self, viewport_dimension: f32) -> bool {
        if !self.viewport_dimension_was_non_zero {
            self.viewport_dimension_was_non_zero = viewport_dimension != 0.0;
        }
        if !self.viewport_dimension_was_non_zero || self.needs_pixels_correction {
            self.needs_pixels_correction = false;
            return true;
        }
        false
    }

    pub fn mark_needs_pixels_correction(&mut self) {
        self.needs_pixels_correction = true;
    }
}

/// Upstream `TabBarView`: one child per tab, kept in step with the controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabBarView {
    pub child_count: usize,
    pub viewport_fraction: f32,
}

impl TabBarView {
    pub fn new(child_count: usize) -> TabBarView {
        TabBarView {
            child_count,
            viewport_fraction: 1.0,
        }
    }

    /// Upstream: *"The length of `children` must be the same as the
    /// `controller`'s length."*
    pub fn matches(&self, controller: &TabController) -> bool {
        self.child_count == controller.length()
    }
}

/// Upstream `TabPageSelectorIndicator`: one small circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabPageSelectorIndicator {
    pub size: f32,
    pub solid_border: bool,
}

impl TabPageSelectorIndicator {
    /// Upstream wraps the circle in `EdgeInsets.all(4.0)`, so the space a dot
    /// takes is its diameter plus eight.
    pub const MARGIN: f32 = 4.0;

    pub fn new(size: f32) -> TabPageSelectorIndicator {
        TabPageSelectorIndicator {
            size,
            solid_border: true,
        }
    }

    pub fn occupied_extent(&self) -> f32 {
        self.size + 2.0 * TabPageSelectorIndicator::MARGIN
    }
}

/// Upstream `TabPageSelector`: a row of dots, one per tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabPageSelector {
    pub indicator_size: f32,
}

impl TabPageSelector {
    pub const DEFAULT_INDICATOR_SIZE: f32 = 12.0;

    pub fn new() -> TabPageSelector {
        TabPageSelector {
            indicator_size: TabPageSelector::DEFAULT_INDICATOR_SIZE,
        }
    }

    /// Upstream asserts `indicatorSize > 0.0`: a dot with no diameter.
    pub fn is_valid(&self) -> bool {
        self.indicator_size > 0.0
    }

    /// Upstream `_buildTabIndicator`, and **this is where the two ways of moving
    /// pay off.** How bright a given dot is, from 0 (unselected) to 1.
    ///
    /// The two branches are not two cases of one formula, they are two different
    /// formulas:
    ///
    /// * **Tapped.** Progress runs from the previous tab to the new one, and
    ///   every tab that is neither gets nothing -- *including the tabs being
    ///   animated past*. A jump from tab 0 to tab 4 does not light up 1, 2 and 3
    ///   on the way.
    /// * **Dragged.** Only the current tab and its two immediate neighbours can
    ///   be lit at all, because a drag cannot reach further than one page. The
    ///   current dot dims by `1 - |offset|` while the neighbour brightens by
    ///   `|offset|`, so between them the brightness is conserved.
    ///
    /// If `indexIsChanging` did not exist, neither formula could be recovered
    /// from the other: the tap case needs the previous index, and the drag case
    /// needs the offset, and each is meaningless in the other's situation.
    pub fn indicator_brightness(
        &self,
        controller: &TabController,
        tab_index: usize,
        tap_progress: f32,
    ) -> f32 {
        if controller.index_is_changing() {
            if controller.index() == tab_index {
                return tap_progress;
            }
            if controller.previous_index() == tab_index {
                return 1.0 - tap_progress;
            }
            return 0.0;
        }
        let offset = controller.offset();
        let index = controller.index() as isize;
        let tab = tab_index as isize;
        if index == tab {
            return 1.0 - offset.abs();
        }
        if index == tab - 1 && offset > 0.0 {
            return offset;
        }
        if index == tab + 1 && offset < 0.0 {
            return -offset;
        }
        0.0
    }
}

impl Default for TabPageSelector {
    fn default() -> Self {
        TabPageSelector::new()
    }
}

/// Upstream `UnderlineTabIndicator`: a line under the selected tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnderlineTabIndicator {
    pub border_width: f32,
    /// `None` draws a plain rectangle; `Some` draws a rounded one.
    pub border_radius: Option<f32>,
    pub insets: (f32, f32, f32, f32),
}

impl UnderlineTabIndicator {
    pub fn new() -> UnderlineTabIndicator {
        UnderlineTabIndicator {
            border_width: 2.0,
            border_radius: None,
            insets: (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Upstream `_indicatorRectFor`, returning `(top, height)` for a tab whose
    /// inset bottom edge is at `bottom`.
    ///
    /// The line sits at `bottom - width` with a height of `width`: **inside the
    /// bottom of the inset tab, not below it.** Enlarging the stroke grows the
    /// line upwards.
    pub fn indicator_band(&self, inset_bottom: f32) -> (f32, f32) {
        (inset_bottom - self.border_width, self.border_width)
    }

    /// Whether painting deflates the rect by half the stroke before drawing.
    ///
    /// The rounded path fills an `RRect` and the square path strokes a line, and
    /// a stroke is centred on its path while a fill is not -- so only the square
    /// case deflates, and the two end up the same thickness by different means.
    pub fn deflates_before_painting(&self) -> bool {
        self.border_radius.is_none()
    }

    /// Upstream `lerpFrom` / `lerpTo`, and this is a divergence worth naming.
    ///
    /// **Both interpolate `borderSide` and `insets` and then build the result
    /// without passing `borderRadius` at all**, so it falls back to its `null`
    /// default. Animating between two rounded underline indicators therefore
    /// produces a square one for the entire animation, snapping back to rounded
    /// when it lands.
    ///
    /// Ported as upstream behaves. Fixing it here would make the two disagree
    /// mid-animation, and the corners are upstream's call to make.
    pub fn lerp(
        a: &UnderlineTabIndicator,
        b: &UnderlineTabIndicator,
        t: f32,
    ) -> UnderlineTabIndicator {
        UnderlineTabIndicator {
            border_width: a.border_width + (b.border_width - a.border_width) * t,
            // Not `a.border_radius` or `b.border_radius`. See above.
            border_radius: None,
            insets: (
                a.insets.0 + (b.insets.0 - a.insets.0) * t,
                a.insets.1 + (b.insets.1 - a.insets.1) * t,
                a.insets.2 + (b.insets.2 - a.insets.2) * t,
                a.insets.3 + (b.insets.3 - a.insets.3) * t,
            ),
        }
    }
}

impl Default for UnderlineTabIndicator {
    fn default() -> Self {
        UnderlineTabIndicator::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller(length: usize) -> TabController {
        TabController::new(length, 0).unwrap()
    }

    // -- The guard that looks like an optimisation ---------------------------------

    #[test]
    fn a_controller_with_no_tabs_refuses_an_index_its_own_assert_would_admit() {
        // assert(value >= 0 && (value < length || length == 0)) lets anything
        // non-negative through when the length is zero. `length < 2` is what
        // actually holds the line.
        let mut empty = controller(0);
        assert_eq!(
            empty.change_index(47, false),
            ChangeOutcome::NothingToSwitchBetween
        );
        assert_eq!(empty.index(), 0, "and the index did not move");
    }

    #[test]
    fn a_single_tab_has_nothing_to_switch_between_either() {
        let mut one = controller(1);
        assert_eq!(
            one.change_index(5, false),
            ChangeOutcome::NothingToSwitchBetween
        );
        // Two tabs is the first length where anything can happen.
        let mut two = controller(2);
        assert_eq!(two.change_index(1, false), ChangeOutcome::Changed);
    }

    #[test]
    fn the_doc_and_the_assert_disagree_about_a_zero_tab_controller() {
        // The doc says "If length is zero, then initialIndex must be 0 (the
        // default)". The assert one line below it says
        // `initialIndex >= 0 && (length == 0 || initialIndex < length)`, whose
        // `length == 0` term switches the range check off entirely. The assert
        // is what runs, so the doc loses: a zero-tab controller reporting index
        // 47 can really be built.
        assert!(TabController::new(0, 0).is_ok());
        let absurd = TabController::new(0, 47).expect("upstream's assert admits this");
        assert_eq!(absurd.index(), 47, "and nothing ever corrects it");

        // Where the length is real, the range check is real too.
        assert!(TabController::new(3, 2).is_ok());
        assert!(TabController::new(3, 3).is_err());
    }

    #[test]
    fn nothing_repairs_that_index_it_is_only_prevented_from_moving() {
        let mut absurd = TabController::new(0, 47).unwrap();
        assert_eq!(
            absurd.change_index(0, false),
            ChangeOutcome::NothingToSwitchBetween
        );
        assert_eq!(absurd.index(), 47);
    }

    // -- The flag is a count -------------------------------------------------------

    #[test]
    fn a_second_tap_during_the_first_animation_keeps_the_flag_set() {
        // This is the reason it is not a bool: the first completion arrives
        // while the second move is still running, and a bool would report the
        // selection as settled.
        let mut tabs = controller(5);
        tabs.animate_to(2);
        tabs.animate_to(3);
        assert!(tabs.index_is_changing());

        tabs.complete_animation();
        assert!(
            tabs.index_is_changing(),
            "one of the two is still in flight"
        );

        tabs.complete_animation();
        assert!(!tabs.index_is_changing());
    }

    #[test]
    fn an_unanimated_change_leaves_no_trace_of_the_flag_afterwards() {
        // Upstream increments and decrements around one synchronous assignment.
        let mut tabs = controller(5);
        assert_eq!(tabs.change_index(3, false), ChangeOutcome::Changed);
        assert!(!tabs.index_is_changing());
        assert_eq!(tabs.offset(), 0.0, "and it lands exactly on the new tab");
    }

    #[test]
    fn a_completed_animation_on_a_disposed_controller_says_nothing() {
        let mut tabs = controller(5);
        tabs.animate_to(2);
        tabs.dispose();
        tabs.complete_animation();
        assert_eq!(tabs.animation(), None);
    }

    // -- The two ways of moving are exclusive -----------------------------------

    #[test]
    fn you_cannot_drag_the_pages_while_a_tap_is_still_animating() {
        let mut tabs = controller(5);
        tabs.animate_to(3);
        assert!(tabs.set_offset(0.4).is_err());

        tabs.complete_animation();
        assert!(tabs.set_offset(0.4).is_ok());
    }

    #[test]
    fn a_drag_never_reaches_past_an_adjacent_page() {
        let mut tabs = controller(5);
        assert!(tabs.set_offset(1.0).is_ok());
        assert!(tabs.set_offset(-1.0).is_ok());
        assert!(tabs.set_offset(1.5).is_err());
    }

    #[test]
    fn the_offset_is_the_residue_of_the_animation_value_and_the_index() {
        let mut tabs = controller(5);
        tabs.change_index(2, false);
        assert_eq!(tabs.offset(), 0.0);
        tabs.set_offset(0.25).unwrap();
        assert_eq!(tabs.animation(), Some(2.25));
        assert_eq!(tabs.offset(), 0.25);
    }

    #[test]
    fn a_drag_does_not_set_the_flag_even_though_the_selection_is_moving() {
        // Which is why the name is about the cause, not the fact.
        let mut tabs = controller(5);
        tabs.set_offset(0.6).unwrap();
        assert!(!tabs.index_is_changing());
    }

    // -- And that is what the dots are for -----------------------------------------

    #[test]
    fn a_tap_across_the_bar_does_not_light_up_the_tabs_it_passes() {
        let mut tabs = controller(5);
        tabs.animate_to(4);
        let selector = TabPageSelector::new();
        let half = |tab| selector.indicator_brightness(&tabs, tab, 0.5);

        assert_eq!(half(4), 0.5, "the destination is filling");
        assert_eq!(half(0), 0.5, "the origin is emptying");
        for passed in [1, 2, 3] {
            assert_eq!(half(passed), 0.0, "tab {passed} is passed over, not lit");
        }
    }

    #[test]
    fn a_drag_shares_its_brightness_between_exactly_two_dots() {
        let mut tabs = controller(5);
        tabs.change_index(1, false);
        tabs.set_offset(0.25).unwrap();
        let selector = TabPageSelector::new();
        let at = |tab| selector.indicator_brightness(&tabs, tab, 0.0);

        assert_eq!(at(1), 0.75);
        assert_eq!(at(2), 0.25);
        assert_eq!(at(1) + at(2), 1.0, "the brightness is conserved");
        assert_eq!(at(0), 0.0, "and the far side stays dark");
        assert_eq!(at(3), 0.0);
    }

    #[test]
    fn dragging_the_other_way_lights_the_other_neighbour() {
        let mut tabs = controller(5);
        tabs.change_index(2, false);
        tabs.set_offset(-0.25).unwrap();
        let selector = TabPageSelector::new();
        assert_eq!(selector.indicator_brightness(&tabs, 1, 0.0), 0.25);
        assert_eq!(selector.indicator_brightness(&tabs, 3, 0.0), 0.0);
    }

    #[test]
    fn the_two_formulas_disagree_about_the_same_arrangement() {
        // The same controller state, read the two ways, gives different dots --
        // which is the whole reason the flag has to exist. Neither formula can
        // be recovered from the other.
        let mut tapped = controller(5);
        tapped.animate_to(2);
        let selector = TabPageSelector::new();
        let while_tapping = selector.indicator_brightness(&tapped, 1, 0.5);

        let mut dragged = controller(5);
        dragged.change_index(2, false);
        dragged.set_offset(-0.5).unwrap();
        let while_dragging = selector.indicator_brightness(&dragged, 1, 0.5);

        assert_eq!(while_tapping, 0.0, "tab 1 was passed over");
        assert_eq!(while_dragging, 0.5, "tab 1 is being dragged towards");
        assert_ne!(while_tapping, while_dragging);
    }

    #[test]
    fn a_dot_needs_a_diameter() {
        assert!(TabPageSelector::new().is_valid());
        assert!(
            !TabPageSelector {
                indicator_size: 0.0
            }
            .is_valid()
        );
    }

    #[test]
    fn a_dot_takes_eight_more_pixels_than_it_draws() {
        let dot = TabPageSelectorIndicator::new(12.0);
        assert_eq!(dot.occupied_extent(), 20.0);
    }

    // -- What a tab may be made of ------------------------------------------------

    #[test]
    fn a_tab_needs_something_in_it() {
        assert!(Tab::default().validate().is_err());
        assert!(
            Tab {
                has_icon: true,
                ..Tab::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn text_is_an_alternative_to_child_rather_than_a_shorthand_for_it() {
        // Offering both is an error, not a precedence question.
        let both = Tab {
            has_text: true,
            has_child: true,
            ..Tab::default()
        };
        assert!(both.validate().is_err());
    }

    #[test]
    fn the_taller_row_is_for_stacking_two_things_not_for_having_an_icon() {
        let icon_only = Tab {
            has_icon: true,
            ..Tab::default()
        };
        assert_eq!(icon_only.preferred_height(), TAB_HEIGHT);

        let icon_and_label = Tab {
            has_icon: true,
            has_text: true,
            ..Tab::default()
        };
        assert_eq!(icon_and_label.preferred_height(), TEXT_AND_ICON_TAB_HEIGHT);
        assert_eq!(icon_and_label.preferred_height(), 72.0);

        let label_only = Tab {
            has_text: true,
            ..Tab::default()
        };
        assert_eq!(label_only.preferred_height(), 46.0);
    }

    #[test]
    fn an_explicit_height_wins_over_the_content() {
        let tab = Tab {
            has_icon: true,
            has_text: true,
            has_child: false,
            height: Some(20.0),
        };
        assert_eq!(tab.preferred_height(), 20.0);
    }

    // -- The controller that only exists because of layout order ---------------------

    #[test]
    fn a_zero_width_viewport_keeps_asking_to_be_corrected() {
        // Otherwise the first real dimension would start a ballistic scroll from
        // a position that was never meant to be believed.
        let mut scroll = TabBarScrollController::new();
        assert!(scroll.apply_content_dimensions(0.0));
        assert!(
            scroll.apply_content_dimensions(0.0),
            "and again -- that term is not one-shot, so every zero-width pass corrects"
        );

        // But those passes have consumed `_needsPixelsCorrection`, so the first
        // believable width does not get a correction of its own. What re-arms
        // the flag is the TabBar swapping controllers, not the width arriving.
        assert!(!scroll.apply_content_dimensions(400.0));
    }

    #[test]
    fn a_bar_that_was_never_zero_width_corrects_exactly_once() {
        let mut scroll = TabBarScrollController::new();
        assert!(
            scroll.apply_content_dimensions(400.0),
            "the position should be adjusted at least once"
        );
        assert!(
            !scroll.apply_content_dimensions(400.0),
            "and then left alone"
        );
    }

    #[test]
    fn a_correction_can_be_asked_for_again_later() {
        let mut scroll = TabBarScrollController::new();
        scroll.apply_content_dimensions(400.0);
        assert!(!scroll.apply_content_dimensions(400.0));
        scroll.mark_needs_pixels_correction();
        assert!(scroll.apply_content_dimensions(400.0));
    }

    #[test]
    fn an_unattached_scroll_controller_has_no_tab_bar_to_ask() {
        let mut scroll = TabBarScrollController::new();
        assert!(!scroll.debug_check_has_tab_bar_state());
        scroll.attached = true;
        assert!(scroll.debug_check_has_tab_bar_state());
    }

    // -- A divergence upstream owns --------------------------------------------------

    #[test]
    fn interpolating_two_rounded_underlines_produces_a_square_one() {
        // lerpFrom and lerpTo both rebuild without passing borderRadius, so it
        // falls back to null for the whole animation and snaps back on landing.
        let a = UnderlineTabIndicator {
            border_radius: Some(8.0),
            ..UnderlineTabIndicator::new()
        };
        let b = UnderlineTabIndicator {
            border_radius: Some(8.0),
            border_width: 6.0,
            ..UnderlineTabIndicator::new()
        };
        let midway = UnderlineTabIndicator::lerp(&a, &b, 0.5);

        assert_eq!(midway.border_width, 4.0, "the width does interpolate");
        assert_eq!(
            midway.border_radius, None,
            "and both endpoints agreed on 8.0"
        );
        assert_eq!(a.border_radius, b.border_radius);
    }

    #[test]
    fn the_line_hangs_inside_the_bottom_of_the_tab_rather_than_below_it() {
        let thin = UnderlineTabIndicator::new();
        assert_eq!(thin.indicator_band(46.0), (44.0, 2.0));

        let thick = UnderlineTabIndicator {
            border_width: 10.0,
            ..UnderlineTabIndicator::new()
        };
        let (top, height) = thick.indicator_band(46.0);
        assert_eq!((top, height), (36.0, 10.0), "a thicker line grows upwards");
        assert_eq!(top + height, 46.0, "the bottom edge does not move");
    }

    #[test]
    fn only_the_stroked_path_deflates_because_only_a_stroke_is_centred() {
        assert!(UnderlineTabIndicator::new().deflates_before_painting());
        assert!(
            !UnderlineTabIndicator {
                border_radius: Some(4.0),
                ..UnderlineTabIndicator::new()
            }
            .deflates_before_painting()
        );
    }

    // -- The default controller ---------------------------------------------------

    #[test]
    fn a_changed_length_keeps_the_animation_it_was_already_running() {
        let mut default = DefaultTabController::new(5, 3, 7).unwrap();
        default.controller.set_offset(0.5).unwrap();
        default.update_length(6);

        assert_eq!(default.controller.length(), 6);
        assert_eq!(default.controller.index(), 3, "and stays where it was");
        assert_eq!(
            default.controller.offset(),
            0.5,
            "mid-drag and still mid-drag"
        );
    }

    #[test]
    fn a_shortened_list_pulls_the_selection_back_into_range() {
        let mut default = DefaultTabController::new(5, 4, 7).unwrap();
        default.update_length(2);
        assert_eq!(default.controller.index(), 1);
    }

    #[test]
    fn a_view_has_to_have_one_child_per_tab() {
        let tabs = controller(3);
        assert!(TabBarView::new(3).matches(&tabs));
        assert!(!TabBarView::new(4).matches(&tabs));
    }
}
