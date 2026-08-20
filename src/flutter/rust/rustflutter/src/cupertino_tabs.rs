//! Ports of `cupertino/tab_scaffold.dart`'s `CupertinoTabController` and
//! `RestorableCupertinoTabController`, and `cupertino/tab_view.dart`'s
//! `CupertinoTabView`.
//!
//! Tick 84 ported the Material `TabController` and found a doc disagreeing with
//! the assert beside it. This one disagrees too, for a completely different
//! reason.

/// Upstream `CupertinoTabController`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoTabController {
    index: usize,
    disposed: bool,
}

impl CupertinoTabController {
    /// Upstream's constructor doc says, and its `index` setter doc repeats:
    ///
    /// > The value must be greater than or equal to 0, **and less than the total
    /// > number of tabs.**
    ///
    /// And the assert is `assert(initialIndex >= 0)`. That is the whole of it --
    /// **there is no upper bound, because this class has no idea what the upper
    /// bound is.** Unlike Material's `TabController` it takes no `length`; the
    /// tabs live in the `CupertinoTabScaffold` it will be handed to.
    ///
    /// So the same sentence appears three times -- here, on the setter, and
    /// again on [`RestorableCupertinoTabController`] -- and not one of the three
    /// places that states the contract is able to check it. Compare tick 84,
    /// where `TabController`'s doc and assert also disagreed: **there the assert
    /// had a hole in it, here the class has not got the information.** The same
    /// shape of discrepancy from opposite causes, and this one is honest -- it
    /// checks what it can see.
    ///
    /// Where the check actually happens is
    /// [`CupertinoTabScaffoldState::on_current_index_change`].
    pub fn new(initial_index: usize) -> CupertinoTabController {
        CupertinoTabController {
            index: initial_index,
            disposed: false,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Upstream's setter, which returns early on an unchanged value so listeners
    /// are not woken for nothing.
    pub fn set_index(&mut self, value: usize) -> bool {
        if self.index == value {
            return false;
        }
        self.index = value;
        true
    }

    /// Upstream's `_isDisposed`, set in `dispose` and read by the scaffold.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
    }
}

impl Default for CupertinoTabController {
    fn default() -> Self {
        CupertinoTabController::new(0)
    }
}

/// Upstream `RestorableCupertinoTabController`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestorableCupertinoTabController {
    initial_index: usize,
    pub value: CupertinoTabController,
}

impl RestorableCupertinoTabController {
    /// Carries the same doc sentence about the total number of tabs, and the
    /// same lone `assert(initialIndex >= 0)`.
    pub fn new(initial_index: usize) -> RestorableCupertinoTabController {
        RestorableCupertinoTabController {
            initial_index,
            value: CupertinoTabController::new(initial_index),
        }
    }

    pub fn create_default_value(&self) -> CupertinoTabController {
        CupertinoTabController::new(self.initial_index)
    }

    /// Upstream `toPrimitives`, which is `value.index` -- **a bare integer.**
    ///
    /// Worth setting beside tick 91's `RestorableTimeOfDay`, which serialises
    /// `[minute, hour]` and invites the reader to tidy the order into something
    /// broken. One value has no order to get wrong.
    pub fn to_primitives(&self) -> usize {
        self.value.index()
    }

    /// Upstream `fromPrimitives`, which asserts the data is non-null and casts.
    pub fn from_primitives(data: usize) -> CupertinoTabController {
        CupertinoTabController::new(data)
    }
}

/// Why a scaffold refused its controller's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabIndexError {
    OutOfBounds { index: usize, tab_count: usize },
}

/// Upstream's `_CupertinoTabScaffoldState`, as far as the controller contract
/// goes. The widget itself is [`crate::cupertino::CupertinoTabScaffold`]; this
/// is the state that enforces the half the controller could not check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoTabScaffoldState {
    pub tab_count: usize,
}

impl CupertinoTabScaffoldState {
    pub fn new(tab_count: usize) -> CupertinoTabScaffoldState {
        CupertinoTabScaffoldState { tab_count }
    }

    /// Upstream `_onCurrentIndexChange`, whose assert supplies the upper bound
    /// the controller had no way to state:
    ///
    /// ```dart
    /// assert(
    ///   _controller.index >= 0 && _controller.index < widget.tabBar.items.length,
    ///   "The $runtimeType's current index ${_controller.index} is "
    ///   'out of bounds for the tab bar with ${widget.tabBar.items.length} tabs',
    /// );
    /// ```
    ///
    /// **The contract is split across two classes** -- the controller checks the
    /// half it can see, the scaffold the half it can -- and the error message is
    /// written from the scaffold's side, naming both numbers, because it is the
    /// only place both are known.
    pub fn on_current_index_change(
        &self,
        controller: &CupertinoTabController,
    ) -> Result<(), TabIndexError> {
        if controller.index() >= self.tab_count {
            return Err(TabIndexError::OutOfBounds {
                index: controller.index(),
                tab_count: self.tab_count,
            });
        }
        Ok(())
    }

    /// Upstream `didUpdateWidget`, and the shape of the branch matters:
    ///
    /// ```dart
    /// if (widget.controller != oldWidget.controller) {
    ///   _updateTabController(oldWidget.controller);
    /// } else if (_controller.index >= widget.tabBar.items.length) {
    ///   // If a new [tabBar] with less than (_controller.index + 1) items is provided,
    ///   // clamp the current index.
    ///   _controller.index = widget.tabBar.items.length - 1;
    /// }
    /// ```
    ///
    /// **`else if`, not a second `if`.** A tab bar that has shrunk pulls the
    /// selection back into range -- but only when the controller itself did not
    /// also change. Swap both at once and the clamp is skipped, and an
    /// out-of-range index survives to be caught by the assert above on the next
    /// change instead.
    ///
    /// Returns whether the index was clamped.
    pub fn did_update_widget(
        &self,
        controller: &mut CupertinoTabController,
        controller_changed: bool,
    ) -> bool {
        if controller_changed {
            return false;
        }
        if controller.index() >= self.tab_count && self.tab_count > 0 {
            controller.set_index(self.tab_count - 1);
            return true;
        }
        false
    }

    /// Upstream `_updateTabController`'s listener move:
    ///
    /// ```dart
    /// if (oldWidgetController?._isDisposed == false) {
    ///   oldWidgetController!.removeListener(_onCurrentIndexChange);
    /// }
    /// widget.controller?.addListener(_onCurrentIndexChange);
    /// ```
    ///
    /// The `== false` is doing null handling rather than boolean comparison:
    /// `?.` on a null yields null, and `null == false` is false, so the one test
    /// means **"exists and is not disposed"**. A disposed `ChangeNotifier`
    /// throws when you touch its listeners, and the controller being left behind
    /// is precisely the one a caller may have disposed already.
    ///
    /// Note the asymmetry -- only the removal is guarded. The controller being
    /// taken up is assumed live.
    pub fn should_remove_listener_from(old: Option<&CupertinoTabController>) -> bool {
        old.is_some_and(|controller| !controller.is_disposed())
    }
}

/// Upstream `CupertinoTabView`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CupertinoTabView {
    /// A key the caller supplied, if any.
    pub navigator_key: Option<u64>,
    owned_navigator_key: Option<u64>,
    pub has_builder: bool,
    pub has_routes: bool,
    pub has_on_generate_route: bool,
    pub has_on_unknown_route: bool,
}

impl CupertinoTabView {
    pub fn new() -> CupertinoTabView {
        CupertinoTabView {
            navigator_key: None,
            owned_navigator_key: None,
            has_builder: true,
            has_routes: false,
            has_on_generate_route: false,
            has_on_unknown_route: false,
        }
    }

    /// Upstream's `_navigatorKey` getter, which makes one **only if it was not
    /// given one** and then keeps it:
    ///
    /// ```dart
    /// GlobalKey<NavigatorState> get _navigatorKey {
    ///   if (widget.navigatorKey != null) {
    ///     return widget.navigatorKey!;
    ///   }
    ///   _ownedNavigatorKey ??= GlobalKey<NavigatorState>();
    ///   return _ownedNavigatorKey!;
    /// }
    /// ```
    ///
    /// The same only-clean-up-what-you-made rule the expansion tile and the
    /// search anchor follow, in its other half: **only build what you were not
    /// handed.**
    pub fn navigator_key(&mut self, next_owned: u64) -> u64 {
        if let Some(key) = self.navigator_key {
            return key;
        }
        *self.owned_navigator_key.get_or_insert(next_owned)
    }

    /// Whether this view made its own key, which is what would decide who
    /// disposes it.
    pub fn owns_its_navigator_key(&self) -> bool {
        self.navigator_key.is_none() && self.owned_navigator_key.is_some()
    }

    /// Upstream `_onUnknownRoute` throws a `FlutterError` that lists the four
    /// route sources **in the order they are tried**: `builder` for "/", then
    /// `routes`, then `onGenerateRoute`, then `onUnknownRoute`.
    ///
    /// An error message that teaches the lookup order rather than reporting a
    /// failure -- you find out how routing works at the moment it did not.
    pub fn route_sources_in_order() -> [&'static str; 4] {
        ["builder", "routes", "onGenerateRoute", "onUnknownRoute"]
    }

    /// Whether a route can be produced at all, before falling to the error.
    pub fn can_generate(&self, is_default_route: bool) -> bool {
        (is_default_route && self.has_builder) || self.has_routes || self.has_on_generate_route
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- A contract stated three times and checked in none of them ------------------

    #[test]
    fn the_controller_accepts_an_index_far_past_any_tab_it_could_have() {
        // Its doc forbids this; its assert cannot see far enough to.
        let controller = CupertinoTabController::new(47);
        assert_eq!(controller.index(), 47);
    }

    #[test]
    fn the_restorable_one_states_the_same_rule_and_checks_the_same_half() {
        let restorable = RestorableCupertinoTabController::new(47);
        assert_eq!(restorable.value.index(), 47);
        assert_eq!(restorable.create_default_value().index(), 47);
    }

    #[test]
    fn the_scaffold_supplies_the_bound_the_controller_could_not() {
        let scaffold = CupertinoTabScaffoldState::new(3);
        let mut controller = CupertinoTabController::new(2);
        assert_eq!(scaffold.on_current_index_change(&controller), Ok(()));

        controller.set_index(3);
        assert_eq!(
            scaffold.on_current_index_change(&controller),
            Err(TabIndexError::OutOfBounds {
                index: 3,
                tab_count: 3
            }),
            "and names both numbers, being the only place both are known"
        );
    }

    // -- else if, not a second if ------------------------------------------------------

    #[test]
    fn a_tab_bar_that_shrank_pulls_the_selection_back_into_range() {
        let scaffold = CupertinoTabScaffoldState::new(2);
        let mut controller = CupertinoTabController::new(4);
        assert!(scaffold.did_update_widget(&mut controller, false));
        assert_eq!(controller.index(), 1);
    }

    #[test]
    fn but_not_when_the_controller_changed_in_the_same_breath() {
        // The clamp is the else arm, so swapping both at once skips it and the
        // out-of-range index survives to meet the assert instead.
        let scaffold = CupertinoTabScaffoldState::new(2);
        let mut controller = CupertinoTabController::new(4);
        assert!(!scaffold.did_update_widget(&mut controller, true));
        assert_eq!(controller.index(), 4, "unclamped");
        assert!(scaffold.on_current_index_change(&controller).is_err());
    }

    #[test]
    fn an_index_already_in_range_is_left_alone() {
        let scaffold = CupertinoTabScaffoldState::new(5);
        let mut controller = CupertinoTabController::new(2);
        assert!(!scaffold.did_update_widget(&mut controller, false));
        assert_eq!(controller.index(), 2);
    }

    // -- Exists and is not disposed ----------------------------------------------------

    #[test]
    fn the_listener_is_taken_off_only_a_controller_that_exists_and_is_alive() {
        // `oldWidgetController?._isDisposed == false` is one test doing two jobs.
        assert!(!CupertinoTabScaffoldState::should_remove_listener_from(
            None
        ));

        let live = CupertinoTabController::new(0);
        assert!(CupertinoTabScaffoldState::should_remove_listener_from(
            Some(&live)
        ));

        let mut dead = CupertinoTabController::new(0);
        dead.dispose();
        assert!(!CupertinoTabScaffoldState::should_remove_listener_from(
            Some(&dead)
        ));
    }

    #[test]
    fn setting_the_same_index_wakes_nobody() {
        let mut controller = CupertinoTabController::new(1);
        assert!(!controller.set_index(1));
        assert!(controller.set_index(2));
    }

    // -- One value has no order to get wrong -------------------------------------------

    #[test]
    fn restoring_a_tab_index_round_trips_through_a_bare_integer() {
        let restorable = RestorableCupertinoTabController::new(3);
        let stored = restorable.to_primitives();
        assert_eq!(stored, 3);
        assert_eq!(
            RestorableCupertinoTabController::from_primitives(stored).index(),
            3
        );
    }

    // -- Only build what you were not handed --------------------------------------------

    #[test]
    fn a_supplied_navigator_key_is_used_and_none_is_made() {
        let mut view = CupertinoTabView::new();
        view.navigator_key = Some(7);
        assert_eq!(view.navigator_key(99), 7);
        assert!(!view.owns_its_navigator_key());
    }

    #[test]
    fn and_a_view_given_none_makes_one_once_and_keeps_it() {
        let mut view = CupertinoTabView::new();
        assert_eq!(view.navigator_key(99), 99);
        assert_eq!(view.navigator_key(123), 99, "not remade on the next ask");
        assert!(view.owns_its_navigator_key());
    }

    // -- An error message that teaches the order -----------------------------------------

    #[test]
    fn the_unknown_route_error_lists_the_four_sources_in_the_order_they_are_tried() {
        assert_eq!(
            CupertinoTabView::route_sources_in_order(),
            ["builder", "routes", "onGenerateRoute", "onUnknownRoute"]
        );
    }

    #[test]
    fn the_builder_answers_only_the_default_route() {
        let view = CupertinoTabView::new();
        assert!(view.can_generate(true));
        assert!(
            !view.can_generate(false),
            "which is why routes and onGenerateRoute come after it"
        );
    }

    #[test]
    fn a_view_with_a_route_table_can_answer_anything_in_it() {
        let mut view = CupertinoTabView::new();
        view.has_routes = true;
        assert!(view.can_generate(false));
    }
}
