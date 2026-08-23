//! Who owns a scroll and who hears about it -- ports of upstream's
//! `scroll_configuration.dart`, `scroll_controller.dart` (the tracking half),
//! `scroll_notification.dart`, `scroll_notification_observer.dart`,
//! `scroll_position.dart` (the metrics notification) and
//! `primary_scroll_controller.dart`.
//!
//! Two questions run through the lot.
//!
//! **What kind of scroll is this?** Answered by a [`ScrollBehavior`], which is
//! a bundle of platform decisions -- physics, scrollbar, glow, which input
//! devices may drag -- handed down the tree so a whole application scrolls the
//! same way without every scroll view being told.
//!
//! **Who is listening?** Answered twice, deliberately: a notification bubbles
//! up the tree and any ancestor may catch it, while a
//! [`ScrollNotificationObserver`] holds a flat list of listeners that do not
//! have to be ancestors at all. An app bar that elevates when the page scrolls
//! is not above the page, so bubbling would never reach it.

use crate::scrolling::ScrollMetrics;

/// Upstream `TargetPlatform`, for the platform-dependent defaults below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollPlatform {
    Android,
    Fuchsia,
    IOS,
    Linux,
    MacOS,
    #[default]
    Windows,
}

/// Which physics family a platform gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsFamily {
    /// iOS: stretch and spring back.
    Bouncing,
    /// macOS: bouncing, but at the fast deceleration rate.
    BouncingDesktop,
    /// Everything else: stop dead at the edge.
    Clamping,
}

/// Upstream `MultitouchDragStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultitouchDragStrategy {
    /// The most recent finger wins.
    LatestPointer,
    /// The average of the outermost fingers moves the content.
    AverageBoundaryPointers,
    /// Every finger's movement is added together.
    SumAllPointers,
}

impl MultitouchDragStrategy {
    /// Upstream's `_shouldTrackMoveEvent`: whether a pointer that is not the
    /// active one still moves the content.
    ///
    /// ```dart
    /// case MultitouchDragStrategy.sumAllPointers:
    /// case MultitouchDragStrategy.averageBoundaryPointers:
    ///   result = true;
    /// case MultitouchDragStrategy.latestPointer:
    ///   result = _activePointer == null || pointer == _activePointer;
    /// ```
    pub fn tracks_every_pointer(self) -> bool {
        !matches!(self, MultitouchDragStrategy::LatestPointer)
    }

    /// Upstream's `_recordMoveDeltaForMultitouch`, which opens by returning
    /// unless the strategy is `averageBoundaryPointers`.
    ///
    /// **This is what separates the two strategies that both track every
    /// pointer.** Averaging the boundary fingers needs each finger's movement
    /// kept per frame so the outermost pair can be found; summing needs no
    /// such bookkeeping, because a sum does not care which finger contributed
    /// what. Two questions, and the three strategies answer them in three
    /// different combinations.
    pub fn records_per_frame_deltas(self) -> bool {
        matches!(self, MultitouchDragStrategy::AverageBoundaryPointers)
    }
}

/// Upstream `ScrollBehavior`: how scrolling feels, per platform.
///
/// Every one of its methods is a `switch` on the platform, and every one of
/// them carries the same comment -- "when modifying this function, consider
/// modifying the implementation in the Material and Cupertino subclasses as
/// well". That repeated note is worth reading as documentation of the shape:
/// the base class is a **complete** answer, and the design libraries override
/// rather than extend, so each of them has to be kept in step by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollBehavior {
    pub platform: ScrollPlatform,
    /// Upstream's `copyWith(scrollbars:)`, true unless turned off.
    pub scrollbars: bool,
    pub overscroll: bool,
}

impl ScrollBehavior {
    pub fn new(platform: ScrollPlatform) -> ScrollBehavior {
        ScrollBehavior {
            platform,
            scrollbars: true,
            overscroll: true,
        }
    }

    /// Upstream's `getScrollPhysics`.
    ///
    /// All three are wrapped around a `RangeMaintainingScrollPhysics`, which
    /// is the part a caller never asks for and always wants: keeping the
    /// reader's place when the content changes size is not a platform
    /// preference, it is correct everywhere.
    pub fn physics(&self) -> PhysicsFamily {
        match self.platform {
            ScrollPlatform::IOS => PhysicsFamily::Bouncing,
            ScrollPlatform::MacOS => PhysicsFamily::BouncingDesktop,
            _ => PhysicsFamily::Clamping,
        }
    }

    /// Upstream's `buildScrollbar`: **desktop only**.
    ///
    /// A touch platform's scrollbar is a transient thing the scroll view draws
    /// while moving, not a control; the base behaviour leaves it to the design
    /// library. A permanent scrollbar on a phone would take a strip of the
    /// screen for something nobody can grab.
    ///
    /// Note it **asserts a controller is present** on those platforms: a
    /// scrollbar needs a position to read, and there is nothing sensible to
    /// draw without one.
    pub fn builds_scrollbar(&self) -> bool {
        matches!(
            self.platform,
            ScrollPlatform::Linux | ScrollPlatform::MacOS | ScrollPlatform::Windows
        ) && self.scrollbars
    }

    /// Upstream's `buildOverscrollIndicator`: **Android and Fuchsia only**.
    ///
    /// It is the glow, and the platforms that do not get it are the ones whose
    /// physics already show the overscroll by stretching. Doing both would say
    /// the same thing twice.
    pub fn builds_overscroll_indicator(&self) -> bool {
        matches!(
            self.platform,
            ScrollPlatform::Android | ScrollPlatform::Fuchsia
        ) && self.overscroll
    }

    /// Upstream's `getMultitouchDragStrategy`.
    ///
    /// Apple platforms average the outermost fingers; everyone else follows
    /// the latest one. The difference shows when a second finger lands
    /// mid-scroll: on iOS the content keeps moving smoothly, elsewhere it
    /// jumps to the new finger.
    pub fn multitouch_drag_strategy(&self) -> MultitouchDragStrategy {
        match self.platform {
            ScrollPlatform::IOS | ScrollPlatform::MacOS => {
                MultitouchDragStrategy::AverageBoundaryPointers
            }
            _ => MultitouchDragStrategy::LatestPointer,
        }
    }

    /// Upstream's `velocityTrackerBuilder`, which hands iOS and macOS their
    /// own fling trackers.
    ///
    /// A fling's velocity is not simply the last pointer delta over the last
    /// interval -- each platform has its own idea, and matching it is what
    /// makes a Flutter list fling the same distance as a native one.
    pub fn velocity_tracker(&self) -> &'static str {
        match self.platform {
            ScrollPlatform::IOS => "IOSScrollViewFlingVelocityTracker",
            ScrollPlatform::MacOS => "MacOSScrollViewFlingVelocityTracker",
            _ => "VelocityTracker",
        }
    }

    /// Upstream's `shouldNotify`, whose default is **false**.
    ///
    /// A `ScrollConfiguration` is rebuilt whenever anything above it is, and
    /// its behaviour is usually a `const` object that is identical each time.
    /// Notifying by default would rebuild every scroll view in the
    /// application on every frame that touched an ancestor.
    pub fn should_notify(&self, _old: &ScrollBehavior) -> bool {
        false
    }
}

/// Upstream `ScrollConfiguration`: the inherited widget that carries one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollConfiguration {
    pub behavior: ScrollBehavior,
}

impl ScrollConfiguration {
    pub fn new(behavior: ScrollBehavior) -> ScrollConfiguration {
        ScrollConfiguration { behavior }
    }

    /// Upstream's `updateShouldNotify`, which asks the **behaviour** rather
    /// than comparing the widgets -- and only when the runtime types match.
    ///
    /// Two behaviours of different types are always a change, because there is
    /// no meaningful comparison between them; the same type asks
    /// `shouldNotify`, which knows what its own fields mean.
    pub fn update_should_notify(&self, old: &ScrollConfiguration, same_type: bool) -> bool {
        if !same_type {
            return true;
        }
        self.behavior.should_notify(&old.behavior)
    }
}

/// Upstream `PrimaryScrollController`: the scroll a page-level gesture drives.
///
/// It is what makes tapping the iOS status bar scroll the right list, and what
/// a `Scaffold`'s body attaches to without being told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryScrollController {
    pub scroll_direction: ScrollAxis,
    /// Upstream's `automaticallyInheritForPlatforms`, whose default is the
    /// **mobile** platforms only.
    pub inherits_on: &'static [ScrollPlatform],
}

/// Which way a scroll runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAxis {
    #[default]
    Vertical,
    Horizontal,
}

impl PrimaryScrollController {
    /// Upstream's default set: `TargetPlatform.values` on the mobile side.
    pub const MOBILE: &'static [ScrollPlatform] = &[
        ScrollPlatform::Android,
        ScrollPlatform::Fuchsia,
        ScrollPlatform::IOS,
    ];

    pub fn new() -> PrimaryScrollController {
        PrimaryScrollController {
            scroll_direction: ScrollAxis::Vertical,
            inherits_on: Self::MOBILE,
        }
    }

    /// Upstream's `shouldInherit`, and both of its conditions matter.
    ///
    /// The **platform** check is why a desktop list does not silently attach:
    /// desktop scroll views get scrollbars, and a scrollbar needs a controller
    /// of its own -- one shared implicitly across several views would drive
    /// whichever attached last.
    ///
    /// The **axis** check is why a horizontal carousel inside a vertical page
    /// does not steal the page's controller. A primary controller is for one
    /// direction, and the carousel is not going that way.
    pub fn should_inherit(&self, platform: ScrollPlatform, direction: ScrollAxis) -> bool {
        if !self.inherits_on.contains(&platform) {
            return false;
        }
        self.scroll_direction == direction
    }
}

impl Default for PrimaryScrollController {
    fn default() -> PrimaryScrollController {
        PrimaryScrollController::new()
    }
}

/// Upstream `TrackingScrollController`: a controller that remembers which of
/// its positions moved last.
///
/// It exists for a tab view, where several lists share one controller and only
/// one of them is on screen. "The scroll offset" is then ambiguous, and this
/// answers it with "whichever the reader last touched".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackingScrollController {
    pub initial_scroll_offset: f32,
    attached: Vec<u64>,
    last_updated: Option<u64>,
    last_updated_offset: Option<f32>,
}

impl TrackingScrollController {
    pub fn new(initial_scroll_offset: f32) -> TrackingScrollController {
        TrackingScrollController {
            initial_scroll_offset,
            attached: Vec::new(),
            last_updated: None,
            last_updated_offset: None,
        }
    }

    pub fn most_recently_updated_position(&self) -> Option<u64> {
        self.last_updated
    }

    /// Upstream's `initialScrollOffset` override, which returns the **last
    /// remembered offset** in preference to the constructor's.
    ///
    /// So a list created after the reader has already scrolled a sibling
    /// starts where that sibling is, rather than at the top. In a tab view
    /// that is the difference between switching tabs and losing your place.
    pub fn effective_initial_offset(&self) -> f32 {
        self.last_updated_offset
            .unwrap_or(self.initial_scroll_offset)
    }

    /// Upstream's `attach`, which asserts the position is not already there --
    /// a double attach would leave one listener unremovable.
    pub fn attach(&mut self, position: u64) {
        debug_assert!(
            !self.attached.contains(&position),
            "a position attaches once"
        );
        self.attached.push(position);
    }

    /// A position reporting that it moved.
    pub fn position_changed(&mut self, position: u64, pixels: f32) {
        if !self.attached.contains(&position) {
            return;
        }
        self.last_updated = Some(position);
        self.last_updated_offset = Some(pixels);
    }

    /// Upstream's `detach`, and its two clean-ups are not the same.
    ///
    /// The **position** is forgotten as soon as it detaches, because a
    /// reference to a position that is gone is worse than none. But the
    /// **offset** is kept until the last position detaches, which is exactly
    /// what makes the tab-view case work: the list scrolls away, and the next
    /// one to arrive still starts where it was.
    pub fn detach(&mut self, position: u64) {
        debug_assert!(
            self.attached.contains(&position),
            "detaching a position that was never attached"
        );
        self.attached.retain(|held| *held != position);
        if self.last_updated == Some(position) {
            self.last_updated = None;
        }
        if self.attached.is_empty() {
            self.last_updated_offset = None;
        }
    }
}

/// Upstream `ScrollMetricsNotification`: the metrics changed, but nobody
/// scrolled.
///
/// The distinction from a `ScrollUpdateNotification` is the whole class. This
/// one fires when the **content or the viewport** changed size -- a list grew,
/// the keyboard appeared -- while the offset stayed put. A listener drawing a
/// scrollbar needs both; one counting how far the reader scrolled needs only
/// the other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollMetricsNotification {
    pub metrics: ScrollMetrics,
    /// How many viewports the notification has passed through on its way up.
    pub depth: usize,
}

impl ScrollMetricsNotification {
    pub fn new(metrics: ScrollMetrics) -> ScrollMetricsNotification {
        ScrollMetricsNotification { metrics, depth: 0 }
    }

    /// Upstream's `ViewportNotificationMixin`, which increments the depth as
    /// the notification crosses each viewport.
    ///
    /// A nested scroll view's inner list bubbles its notifications through the
    /// outer one, and a listener that acted on every one would act twice.
    /// Depth is how it tells "mine" from "somebody else's inside me".
    pub fn crossed_viewport(mut self) -> ScrollMetricsNotification {
        self.depth += 1;
        self
    }
}

/// Upstream `ViewportElementMixin`: what makes the depth above get counted.
///
/// It is a mixin on the **element** rather than the widget because the count
/// happens while the notification travels, and only the element knows it is
/// on the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewportElementMixin;

impl ViewportElementMixin {
    /// Upstream's `onNotification`, which returns **false**: the notification
    /// is amended and passed on, never consumed. A viewport is a waypoint, not
    /// a destination.
    pub fn consumes_notification() -> bool {
        false
    }
}

/// Upstream `ScrollNotificationObserver`: a flat list of listeners for scroll
/// notifications.
///
/// It exists because bubbling is not enough. An app bar that elevates when the
/// page scrolls under it is a **sibling** of that page, not an ancestor, so no
/// amount of bubbling reaches it. The observer sits above both and hands the
/// notification sideways.
#[derive(Debug, Default)]
pub struct ScrollNotificationObserver;

/// Upstream `ScrollNotificationObserverState`.
#[derive(Debug, Default)]
pub struct ScrollNotificationObserverState {
    listeners: Vec<u64>,
    /// `None` once disposed. Upstream uses a null list for the same purpose.
    disposed: bool,
    delivered: Vec<u64>,
}

impl ScrollNotificationObserverState {
    pub fn new() -> ScrollNotificationObserverState {
        ScrollNotificationObserverState::default()
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    pub fn delivered(&self) -> &[u64] {
        &self.delivered
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    pub fn add_listener(&mut self, listener: u64) {
        debug_assert!(!self.disposed, "a disposed observer can no longer be used");
        self.listeners.push(listener);
    }

    /// Upstream's `removeListener`, which removes the **first** match and
    /// returns. The same callback added twice is removed once, which mirrors
    /// how `ChangeNotifier` behaves and keeps add/remove symmetric.
    pub fn remove_listener(&mut self, listener: u64) {
        debug_assert!(!self.disposed, "a disposed observer can no longer be used");
        if let Some(at) = self.listeners.iter().position(|held| *held == listener) {
            self.listeners.remove(at);
        }
    }

    /// Upstream's `_notifyListeners`.
    ///
    /// It iterates a **copy**, and then checks each entry is **still linked**
    /// before calling it. The copy alone is not enough: a listener that
    /// removes another during dispatch would otherwise have that other one
    /// called anyway, after it had already been told it was gone.
    pub fn notify(&mut self, removes_during_dispatch: &[(u64, u64)]) {
        if self.listeners.is_empty() {
            return;
        }
        let snapshot = self.listeners.clone();
        for listener in snapshot {
            if !self.listeners.contains(&listener) {
                // Unlinked by an earlier listener in this same dispatch.
                continue;
            }
            self.delivered.push(listener);
            for (remover, removed) in removes_during_dispatch {
                if *remover == listener {
                    self.remove_listener(*removed);
                }
            }
        }
    }

    pub fn dispose(&mut self) {
        self.listeners.clear();
        self.disposed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> ScrollMetrics {
        ScrollMetrics {
            pixels: 0.0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 800.0,
            viewport_dimension: 400.0,
        }
    }

    // -- The behaviour's platform switches ---------------------------------

    #[test]
    fn ios_stretches_where_android_stops_dead() {
        assert_eq!(
            ScrollBehavior::new(ScrollPlatform::IOS).physics(),
            PhysicsFamily::Bouncing
        );
        assert_eq!(
            ScrollBehavior::new(ScrollPlatform::MacOS).physics(),
            PhysicsFamily::BouncingDesktop,
            "the same family at the fast rate"
        );
        for platform in [
            ScrollPlatform::Android,
            ScrollPlatform::Fuchsia,
            ScrollPlatform::Linux,
            ScrollPlatform::Windows,
        ] {
            assert_eq!(
                ScrollBehavior::new(platform).physics(),
                PhysicsFamily::Clamping,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_permanent_scrollbar_is_a_desktop_thing() {
        // On a phone it would take a strip of the screen for something nobody
        // can grab.
        for platform in [
            ScrollPlatform::Linux,
            ScrollPlatform::MacOS,
            ScrollPlatform::Windows,
        ] {
            assert!(
                ScrollBehavior::new(platform).builds_scrollbar(),
                "{platform:?}"
            );
        }
        for platform in [
            ScrollPlatform::Android,
            ScrollPlatform::Fuchsia,
            ScrollPlatform::IOS,
        ] {
            assert!(
                !ScrollBehavior::new(platform).builds_scrollbar(),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_glow_goes_exactly_where_the_stretch_does_not() {
        // Doing both would say the same thing twice.
        for platform in [ScrollPlatform::Android, ScrollPlatform::Fuchsia] {
            let behavior = ScrollBehavior::new(platform);
            assert!(behavior.builds_overscroll_indicator());
            assert_eq!(behavior.physics(), PhysicsFamily::Clamping);
        }
        let ios = ScrollBehavior::new(ScrollPlatform::IOS);
        assert!(!ios.builds_overscroll_indicator());
        assert_eq!(ios.physics(), PhysicsFamily::Bouncing);
    }

    #[test]
    fn turning_the_decorations_off_beats_the_platform() {
        let mut desktop = ScrollBehavior::new(ScrollPlatform::Windows);
        desktop.scrollbars = false;
        assert!(!desktop.builds_scrollbar());

        let mut android = ScrollBehavior::new(ScrollPlatform::Android);
        android.overscroll = false;
        assert!(!android.builds_overscroll_indicator());
    }

    #[test]
    fn a_second_finger_mid_scroll_behaves_differently_on_apple_platforms() {
        // On iOS the content keeps moving smoothly; elsewhere it jumps to the
        // new finger.
        for platform in [ScrollPlatform::IOS, ScrollPlatform::MacOS] {
            assert_eq!(
                ScrollBehavior::new(platform).multitouch_drag_strategy(),
                MultitouchDragStrategy::AverageBoundaryPointers,
                "{platform:?}"
            );
        }
        assert_eq!(
            ScrollBehavior::new(ScrollPlatform::Android).multitouch_drag_strategy(),
            MultitouchDragStrategy::LatestPointer
        );
    }

    #[test]
    fn each_apple_platform_gets_its_own_fling_tracker() {
        // Matching the platform's own idea of a fling is what makes a Flutter
        // list fling the same distance as a native one.
        assert_eq!(
            ScrollBehavior::new(ScrollPlatform::IOS).velocity_tracker(),
            "IOSScrollViewFlingVelocityTracker"
        );
        assert_eq!(
            ScrollBehavior::new(ScrollPlatform::MacOS).velocity_tracker(),
            "MacOSScrollViewFlingVelocityTracker"
        );
        assert_eq!(
            ScrollBehavior::new(ScrollPlatform::Windows).velocity_tracker(),
            "VelocityTracker"
        );
    }

    #[test]
    fn a_rebuilt_configuration_notifies_nobody_by_default() {
        // Or every scroll view in the application would rebuild on every frame
        // that touched an ancestor.
        let behavior = ScrollBehavior::new(ScrollPlatform::Android);
        let config = ScrollConfiguration::new(behavior);
        assert!(!config.update_should_notify(&config, true));
    }

    #[test]
    fn a_behaviour_of_a_different_type_is_always_a_change() {
        // There is no meaningful comparison between two different types.
        let config = ScrollConfiguration::new(ScrollBehavior::new(ScrollPlatform::Android));
        assert!(config.update_should_notify(&config, false));
    }

    // -- The primary controller --------------------------------------------

    #[test]
    fn a_desktop_list_does_not_silently_take_the_primary_controller() {
        // Desktop scroll views get scrollbars, and a scrollbar needs a
        // controller of its own -- one shared implicitly would drive whichever
        // attached last.
        let primary = PrimaryScrollController::new();
        assert!(primary.should_inherit(ScrollPlatform::IOS, ScrollAxis::Vertical));
        assert!(primary.should_inherit(ScrollPlatform::Android, ScrollAxis::Vertical));
        assert!(!primary.should_inherit(ScrollPlatform::Windows, ScrollAxis::Vertical));
        assert!(!primary.should_inherit(ScrollPlatform::MacOS, ScrollAxis::Vertical));
    }

    #[test]
    fn a_horizontal_carousel_does_not_steal_the_pages_controller() {
        // A primary controller is for one direction, and the carousel is not
        // going that way.
        let primary = PrimaryScrollController::new();
        assert!(!primary.should_inherit(ScrollPlatform::IOS, ScrollAxis::Horizontal));
    }

    // -- The tracking controller -------------------------------------------

    #[test]
    fn whichever_list_the_reader_last_touched_is_the_one_that_counts() {
        // Several lists share one controller in a tab view, and only one is on
        // screen.
        let mut controller = TrackingScrollController::new(0.0);
        controller.attach(1);
        controller.attach(2);
        assert_eq!(controller.most_recently_updated_position(), None);

        controller.position_changed(1, 120.0);
        assert_eq!(controller.most_recently_updated_position(), Some(1));

        controller.position_changed(2, 40.0);
        assert_eq!(controller.most_recently_updated_position(), Some(2));
    }

    #[test]
    fn a_list_arriving_later_starts_where_its_sibling_is() {
        // Which in a tab view is the difference between switching tabs and
        // losing your place.
        let mut controller = TrackingScrollController::new(0.0);
        controller.attach(1);
        controller.position_changed(1, 250.0);
        assert_eq!(controller.effective_initial_offset(), 250.0);
    }

    #[test]
    fn the_position_is_forgotten_at_once_but_the_offset_outlives_it() {
        // A reference to a position that is gone is worse than none; but the
        // offset is what makes the next list start in the right place.
        let mut controller = TrackingScrollController::new(0.0);
        controller.attach(1);
        controller.attach(2);
        controller.position_changed(1, 250.0);

        controller.detach(1);
        assert_eq!(controller.most_recently_updated_position(), None);
        assert_eq!(
            controller.effective_initial_offset(),
            250.0,
            "still remembered while another position is attached"
        );

        controller.detach(2);
        assert_eq!(
            controller.effective_initial_offset(),
            0.0,
            "and forgotten once the last one has gone"
        );
    }

    #[test]
    #[should_panic(expected = "a position attaches once")]
    fn a_double_attach_would_leave_a_listener_unremovable() {
        let mut controller = TrackingScrollController::new(0.0);
        controller.attach(1);
        controller.attach(1);
    }

    // -- Notifications ------------------------------------------------------

    #[test]
    fn depth_is_how_a_listener_tells_its_own_scroll_from_a_nested_one() {
        // A nested scroll view's inner list bubbles through the outer one, and
        // a listener acting on every notification would act twice.
        let inner = ScrollMetricsNotification::new(metrics());
        assert_eq!(inner.depth, 0, "as it leaves its own viewport");

        let crossed = inner.crossed_viewport();
        assert_eq!(crossed.depth, 1, "seen by the outer one");
        assert_eq!(crossed.crossed_viewport().depth, 2);
    }

    #[test]
    fn a_viewport_amends_a_notification_rather_than_consuming_it() {
        // A viewport is a waypoint, not a destination.
        assert!(!ViewportElementMixin::consumes_notification());
    }

    // -- The observer -------------------------------------------------------

    #[test]
    fn an_app_bar_hears_about_a_page_it_is_not_above() {
        // Which is why the observer exists: bubbling would never reach a
        // sibling.
        let mut observer = ScrollNotificationObserverState::new();
        observer.add_listener(1);
        observer.add_listener(2);
        observer.notify(&[]);
        assert_eq!(observer.delivered(), &[1, 2]);
    }

    #[test]
    fn a_listener_removed_during_dispatch_is_not_called_afterwards() {
        // Iterating a copy alone is not enough -- the removed listener would
        // be called after it had been told it was gone.
        let mut observer = ScrollNotificationObserverState::new();
        observer.add_listener(1);
        observer.add_listener(2);
        observer.add_listener(3);

        // Listener 1 removes listener 3 while it runs.
        observer.notify(&[(1, 3)]);
        assert_eq!(observer.delivered(), &[1, 2], "3 was never reached");
        assert_eq!(observer.listener_count(), 2);
    }

    #[test]
    fn the_same_callback_added_twice_is_removed_once() {
        // Which mirrors ChangeNotifier and keeps add and remove symmetric.
        let mut observer = ScrollNotificationObserverState::new();
        observer.add_listener(1);
        observer.add_listener(1);
        observer.remove_listener(1);
        assert_eq!(observer.listener_count(), 1);
    }

    #[test]
    fn notifying_with_nobody_listening_does_nothing() {
        let mut observer = ScrollNotificationObserverState::new();
        observer.notify(&[]);
        assert!(observer.delivered().is_empty());
    }

    #[test]
    #[should_panic(expected = "a disposed observer can no longer be used")]
    fn a_disposed_observer_says_so_rather_than_failing_quietly() {
        let mut observer = ScrollNotificationObserverState::new();
        observer.dispose();
        observer.add_listener(1);
    }
}

#[cfg(test)]
mod drag_strategy_tests {
    use super::MultitouchDragStrategy;

    const ALL: [MultitouchDragStrategy; 3] = [
        MultitouchDragStrategy::LatestPointer,
        MultitouchDragStrategy::AverageBoundaryPointers,
        MultitouchDragStrategy::SumAllPointers,
    ];

    #[test]
    fn two_of_the_three_watch_every_finger() {
        assert!(!MultitouchDragStrategy::LatestPointer.tracks_every_pointer());
        assert!(MultitouchDragStrategy::AverageBoundaryPointers.tracks_every_pointer());
        assert!(MultitouchDragStrategy::SumAllPointers.tracks_every_pointer());
    }

    #[test]
    fn but_only_one_keeps_a_note_of_what_each_did() {
        // The difference between the two that watch everything. Averaging the
        // outermost fingers needs each finger's movement kept per frame to
        // find the pair; a sum does not care which finger contributed what.
        assert!(MultitouchDragStrategy::AverageBoundaryPointers.records_per_frame_deltas());
        assert!(!MultitouchDragStrategy::SumAllPointers.records_per_frame_deltas());
        assert!(!MultitouchDragStrategy::LatestPointer.records_per_frame_deltas());
    }

    #[test]
    fn and_the_two_questions_tell_the_three_apart() {
        // Neither question alone separates all three -- tracking groups sum
        // with average, recording groups sum with latest -- so the pair of
        // answers is what identifies a strategy.
        let mut answers: Vec<(bool, bool)> = ALL
            .iter()
            .map(|s| (s.tracks_every_pointer(), s.records_per_frame_deltas()))
            .collect();
        answers.sort();
        answers.dedup();
        assert_eq!(answers.len(), 3);
    }
}
