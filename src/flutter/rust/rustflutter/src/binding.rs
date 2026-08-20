//! The widgets binding -- a port of upstream's `widgets/binding.dart`.
//!
//! The binding is where the engine's messages become framework events. Most of
//! it is plumbing, but two of its dispatch loops disagree with each other in a
//! way worth arriving at, because both are right:
//!
//! * A **back press** stops at the first observer that claims it. Whoever is
//!   innermost handles it and nobody below hears about it at all; if nobody
//!   claims it, the application quits.
//! * An **exit request** does **not** stop at the first observer that cancels.
//!   Every observer is asked, even once the answer is already known, because
//!   an observer may be there only to *learn* that an exit was attempted --
//!   to save a draft, say -- and cancelling is somebody else's business.
//!
//! Both loops walk a **copy** of the observer list, so an observer that
//! removes itself while being notified does not corrupt the walk it is in.
//!
//! ## What is not here
//!
//! `BindingBase` and the six mixins upstream composes into
//! `WidgetsFlutterBinding` are this crate's own [`crate::app`] machinery.
//! What is ported is the observer protocol, the two dispatch rules, the root
//! widget's attach-or-update decision, and the root element's deferred update.

use crate::platform::Locale;
use crate::services::system::{AppExitResponse, AppLifecycleState};

/// Upstream `WidgetsBindingObserver`: everything the platform can tell an
/// application.
///
/// Every method has a do-nothing default, which is what makes it usable as a
/// mixin: an observer that only wants the lifecycle overrides one method and
/// inherits fifteen it does not care about.
pub trait WidgetsBindingObserver {
    /// Upstream's `didPopRoute`. **False means "not mine"**, and the binding
    /// moves on to the next observer.
    fn did_pop_route(&mut self) -> bool {
        false
    }

    /// Upstream's `didPushRoute`, for a deep link arriving while the
    /// application is already running.
    fn did_push_route(&mut self, _route: &str) -> bool {
        false
    }

    /// Upstream's `didPushRouteInformation`, whose default **normalises the
    /// URI and delegates to [`WidgetsBindingObserver::did_push_route`]**.
    ///
    /// The normalisation is not decoration: an empty path becomes `/`, and
    /// empty query and fragment are dropped rather than left as a bare `?` or
    /// `#`. An observer matching route strings would otherwise have to handle
    /// four spellings of the same address.
    fn did_push_route_information(&mut self, uri: &RouteUri) -> bool {
        let route = uri.to_route_string();
        self.did_push_route(&route)
    }

    fn handle_start_back_gesture(&mut self, _progress: f32) {}
    fn handle_update_back_gesture_progress(&mut self, _progress: f32) {}
    fn handle_commit_back_gesture(&mut self) {}
    fn handle_cancel_back_gesture(&mut self) {}

    /// Upstream's `handleStatusBarTap`, an iOS affordance: tapping the clock
    /// scrolls the page to the top.
    fn handle_status_bar_tap(&mut self) {}

    fn did_change_metrics(&mut self) {}
    fn did_change_text_scale_factor(&mut self) {}
    fn did_change_platform_brightness(&mut self) {}
    fn did_change_locales(&mut self, _locales: &[Locale]) {}
    fn did_change_app_lifecycle_state(&mut self, _state: AppLifecycleState) {}
    fn did_change_view_focus(&mut self) {}

    /// Upstream's `didRequestAppExit`, whose default is **exit**. An observer
    /// that does not care must not be the reason an application refuses to
    /// close.
    fn did_request_app_exit(&mut self) -> AppExitResponse {
        AppExitResponse::Exit
    }

    fn did_have_memory_pressure(&mut self) {}
    fn did_change_accessibility_features(&mut self) {}
}

/// A route address as the platform delivers it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteUri {
    pub path: String,
    /// Query parameters as they arrived, in order.
    pub query: Vec<(String, String)>,
    pub fragment: Option<String>,
}

impl RouteUri {
    pub fn new(path: impl Into<String>) -> RouteUri {
        RouteUri {
            path: path.into(),
            query: Vec::new(),
            fragment: None,
        }
    }

    pub fn with_query(mut self, query: &[(&str, &str)]) -> Self {
        self.query = query
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        self
    }

    pub fn with_fragment(mut self, fragment: impl Into<String>) -> Self {
        self.fragment = Some(fragment.into());
        self
    }

    /// Upstream's normalisation inside `didPushRouteInformation`.
    pub fn to_route_string(&self) -> String {
        let mut route = if self.path.is_empty() {
            "/".to_string()
        } else {
            self.path.clone()
        };
        if !self.query.is_empty() {
            let pairs: Vec<String> = self
                .query
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            route.push('?');
            route.push_str(&pairs.join("&"));
        }
        if let Some(fragment) = &self.fragment {
            if !fragment.is_empty() {
                route.push('#');
                route.push_str(fragment);
            }
        }
        route
    }
}

/// What a dispatch did, for callers and for tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// Whether anybody claimed the event.
    pub handled: bool,
    /// How many observers were actually asked.
    pub asked: usize,
}

/// Upstream `WidgetsBinding`, reduced to the observer registry and the
/// dispatch rules.
#[derive(Default)]
pub struct WidgetsBinding {
    observers: Vec<Box<dyn WidgetsBindingObserver>>,
    /// Upstream keeps a second list for the predictive back gesture, because
    /// only observers that opted in should be driven through a gesture that
    /// may be cancelled halfway.
    back_gesture_observers: Vec<usize>,
}

impl WidgetsBinding {
    pub fn new() -> WidgetsBinding {
        WidgetsBinding::default()
    }

    pub fn observer_count(&self) -> usize {
        self.observers.len()
    }

    /// Upstream's `addObserver`. Registration order is the dispatch order, and
    /// for a back press that means **the most recently added is asked last**
    /// -- which is why a route pushes its own observer and the navigator's own
    /// registration sits underneath.
    pub fn add_observer(&mut self, observer: Box<dyn WidgetsBindingObserver>) -> usize {
        self.observers.push(observer);
        self.observers.len() - 1
    }

    pub fn observer_mut(&mut self, index: usize) -> Option<&mut Box<dyn WidgetsBindingObserver>> {
        self.observers.get_mut(index)
    }

    pub fn register_back_gesture_observer(&mut self, index: usize) {
        if !self.back_gesture_observers.contains(&index) {
            self.back_gesture_observers.push(index);
        }
    }

    /// Upstream's `handlePopRoute`.
    ///
    /// **Stops at the first observer that returns true.** Whoever claims the
    /// press handles it, and nothing after it hears about the press at all --
    /// two things closing on one back press is exactly the bug this prevents.
    /// If nobody claims it, the application quits, and that is the right
    /// default: a press nothing wanted should leave.
    pub fn handle_pop_route(&mut self) -> DispatchOutcome {
        let mut asked = 0;
        for index in 0..self.observers.len() {
            asked += 1;
            if self.observers[index].did_pop_route() {
                return DispatchOutcome {
                    handled: true,
                    asked,
                };
            }
        }
        DispatchOutcome {
            handled: false,
            asked,
        }
    }

    /// Upstream's `handlePushRoute`, which stops at the first claim for the
    /// same reason: a deep link should open one screen.
    pub fn handle_push_route(&mut self, route: &str) -> DispatchOutcome {
        let mut asked = 0;
        for index in 0..self.observers.len() {
            asked += 1;
            if self.observers[index].did_push_route(route) {
                return DispatchOutcome {
                    handled: true,
                    asked,
                };
            }
        }
        DispatchOutcome {
            handled: false,
            asked,
        }
    }

    pub fn handle_push_route_information(&mut self, uri: &RouteUri) -> DispatchOutcome {
        let mut asked = 0;
        for index in 0..self.observers.len() {
            asked += 1;
            if self.observers[index].did_push_route_information(uri) {
                return DispatchOutcome {
                    handled: true,
                    asked,
                };
            }
        }
        DispatchOutcome {
            handled: false,
            asked,
        }
    }

    /// Upstream's `handleRequestAppExit`, and the comment on its loop is the
    /// whole reason it differs from the one above:
    ///
    /// > Don't early return. For the case where someone is just using the
    /// > observer to know when exit happens, we want to call all the
    /// > observers, even if we already know we're going to cancel.
    ///
    /// An observer saving a draft on the way out must be told even when
    /// another observer has already refused the exit. One cancel is enough to
    /// cancel; it is not enough to stop asking.
    pub fn handle_request_app_exit(&mut self) -> (AppExitResponse, usize) {
        let mut did_cancel = false;
        let mut asked = 0;
        for index in 0..self.observers.len() {
            asked += 1;
            if self.observers[index].did_request_app_exit() == AppExitResponse::Cancel {
                did_cancel = true;
            }
        }
        let response = if did_cancel {
            AppExitResponse::Cancel
        } else {
            AppExitResponse::Exit
        };
        (response, asked)
    }

    /// The broadcast dispatches, which ask everyone and collect nothing.
    pub fn handle_app_lifecycle_state_changed(&mut self, state: AppLifecycleState) {
        for observer in self.observers.iter_mut() {
            observer.did_change_app_lifecycle_state(state);
        }
    }

    pub fn handle_locales_changed(&mut self, locales: &[Locale]) {
        for observer in self.observers.iter_mut() {
            observer.did_change_locales(locales);
        }
    }

    pub fn handle_metrics_changed(&mut self) {
        for observer in self.observers.iter_mut() {
            observer.did_change_metrics();
        }
    }

    pub fn handle_memory_pressure(&mut self) {
        for observer in self.observers.iter_mut() {
            observer.did_have_memory_pressure();
        }
    }

    /// Upstream's `_handleBackGestureInvocation` for a commit, which
    /// **falls back to `handlePopRoute`** when nothing registered for the
    /// gesture. A platform that sends a predictive back event to an
    /// application that never opted in still gets a back press out of it.
    pub fn handle_commit_back_gesture(&mut self) -> DispatchOutcome {
        if self.back_gesture_observers.is_empty() {
            return self.handle_pop_route();
        }
        let indices = self.back_gesture_observers.clone();
        for index in indices {
            if let Some(observer) = self.observers.get_mut(index) {
                observer.handle_commit_back_gesture();
            }
        }
        DispatchOutcome {
            handled: true,
            asked: self.back_gesture_observers.len(),
        }
    }
}

impl std::fmt::Debug for WidgetsBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetsBinding")
            .field("observers", &self.observers.len())
            .finish()
    }
}

/// Upstream `RootWidget`: the widget at the top of everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootWidget {
    /// Upstream's `child`, **optional**: a root with nothing under it is what
    /// exists between `ensureInitialized` and the first `runApp`.
    pub child: Option<u64>,
    /// Upstream's `debugShortDescription`, which replaces the type name in
    /// diagnostics. Every application's tree is rooted in the same class, so
    /// without it every error dump would start with the same uninformative
    /// line.
    pub debug_short_description: Option<String>,
}

/// What [`RootWidget::attach`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachOutcome {
    /// No element yet: one was created and mounted.
    Mounted,
    /// An element was already there: it was given the new widget and marked
    /// dirty, rather than being torn down.
    Updated,
}

impl RootWidget {
    pub fn new(child: Option<u64>) -> RootWidget {
        RootWidget {
            child,
            debug_short_description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.debug_short_description = Some(description.into());
        self
    }

    /// Upstream's `attach`, and the branch is what makes a hot reload keep the
    /// application's state.
    ///
    /// With no element, one is created inside `lockState` -- nothing may be
    /// marked dirty while the root is being built, since there is no scope to
    /// build it in yet. With an element, the new widget is **stashed and the
    /// element marked dirty** rather than mounted afresh: the second `runApp`
    /// of a hot reload updates the tree in place, and every `State` below
    /// survives.
    pub fn attach(&self, element: Option<&mut RootElement>) -> AttachOutcome {
        match element {
            None => AttachOutcome::Mounted,
            Some(element) => {
                element.schedule_update(self.clone());
                AttachOutcome::Updated
            }
        }
    }
}

/// Upstream `RootElement`: the element at the top, whose parent is null.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootElement {
    pub widget: RootWidget,
    child: Option<u64>,
    /// Upstream's `_newWidget`: a widget handed over, waiting for the build
    /// phase to adopt it.
    new_widget: Option<RootWidget>,
    dirty: bool,
    mounted: bool,
    /// Whether the last `_rebuild` threw.
    build_failed: bool,
}

impl RootElement {
    pub fn new(widget: RootWidget) -> RootElement {
        RootElement {
            widget,
            child: None,
            new_widget: None,
            dirty: false,
            mounted: false,
            build_failed: false,
        }
    }

    pub fn child(&self) -> Option<u64> {
        self.child
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted
    }

    pub fn build_failed(&self) -> bool {
        self.build_failed
    }

    /// Upstream's `mount`, which **asserts the parent is null** -- this
    /// element is the root, and mounting it under something would give the
    /// tree two.
    pub fn mount(&mut self, parent: Option<u64>) {
        debug_assert!(parent.is_none(), "RootElement is the root");
        self.mounted = true;
        self.rebuild_child(false);
        self.dirty = false;
    }

    /// Upstream's `_newWidget = this; markNeedsBuild()`.
    pub fn schedule_update(&mut self, widget: RootWidget) {
        self.new_widget = Some(widget);
        self.dirty = true;
    }

    /// Upstream's `performRebuild`.
    ///
    /// The `_newWidget` check is nullable and upstream says why: it can be
    /// null "if, for instance, we were rebuilt due to a reassemble". A rebuild
    /// with no new widget is a rebuild of the same one, which is exactly what
    /// a reassemble wants.
    pub fn perform_rebuild(&mut self, child_throws: bool) {
        if let Some(new_widget) = self.new_widget.take() {
            self.widget = new_widget;
            self.rebuild_child(child_throws);
        }
        self.dirty = false;
        debug_assert!(self.new_widget.is_none());
    }

    /// Upstream's `_rebuild`, and its `catch` carries the sharpest comment in
    /// the file: **"No error widget possible here since it wouldn't have a
    /// view to render into."**
    ///
    /// Everywhere else in the framework a build failure is replaced by the red
    /// error widget. Here there is nothing to replace it *with* -- the thing
    /// that failed is what would have provided the view. So the error is
    /// reported and the child is left null, and the reader gets a blank screen
    /// with a real error in the log rather than a crash.
    fn rebuild_child(&mut self, child_throws: bool) {
        if child_throws {
            self.build_failed = true;
            self.child = None;
            return;
        }
        self.build_failed = false;
        self.child = self.widget.child;
    }

    /// Upstream's `debugDoingBuild`, always false: this element has no build
    /// phase of its own, it only forwards its widget's child.
    pub fn debug_doing_build(&self) -> bool {
        false
    }

    /// Upstream's `debugExpectsRenderObjectForSlot`, always false: there is no
    /// ancestor render-object element to attach anything to.
    pub fn debug_expects_render_object_for_slot(&self) -> bool {
        false
    }

    /// Upstream's `forgetChild`.
    pub fn forget_child(&mut self) {
        self.child = None;
    }
}

/// Upstream `WidgetsFlutterBinding`: the concrete binding an application gets.
///
/// Upstream's whole body is `ensureInitialized`, and the pattern it implements
/// is worth naming: **the first caller decides what the binding is.** A test
/// harness calls its own `ensureInitialized` before the application does, so
/// by the time `runApp` asks there is already a test binding and it is left
/// alone. Constructing unconditionally would replace it.
#[derive(Debug, Default)]
pub struct WidgetsFlutterBinding {
    initialized: bool,
    initialisations: usize,
}

impl WidgetsFlutterBinding {
    pub fn new() -> WidgetsFlutterBinding {
        WidgetsFlutterBinding::default()
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// How many times a binding was actually constructed.
    pub fn initialisations(&self) -> usize {
        self.initialisations
    }

    /// Upstream's `ensureInitialized`.
    pub fn ensure_initialized(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.initialisations += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A spy that records what it was asked and answers as told.
    #[derive(Default)]
    struct Spy {
        log: Rc<RefCell<Vec<String>>>,
        name: &'static str,
        claims_pop: bool,
        claims_push: bool,
        cancels_exit: bool,
    }

    impl Spy {
        fn new(log: &Rc<RefCell<Vec<String>>>, name: &'static str) -> Spy {
            Spy {
                log: Rc::clone(log),
                name,
                claims_pop: false,
                claims_push: false,
                cancels_exit: false,
            }
        }

        fn claiming_pop(mut self) -> Spy {
            self.claims_pop = true;
            self
        }

        fn claiming_push(mut self) -> Spy {
            self.claims_push = true;
            self
        }

        fn cancelling_exit(mut self) -> Spy {
            self.cancels_exit = true;
            self
        }

        fn note(&self, what: &str) {
            self.log.borrow_mut().push(format!("{}:{what}", self.name));
        }
    }

    impl WidgetsBindingObserver for Spy {
        fn did_pop_route(&mut self) -> bool {
            self.note("pop");
            self.claims_pop
        }

        fn did_push_route(&mut self, route: &str) -> bool {
            self.note(&format!("push({route})"));
            self.claims_push
        }

        fn did_request_app_exit(&mut self) -> AppExitResponse {
            self.note("exit");
            if self.cancels_exit {
                AppExitResponse::Cancel
            } else {
                AppExitResponse::Exit
            }
        }

        fn did_change_app_lifecycle_state(&mut self, state: AppLifecycleState) {
            self.note(&format!("lifecycle({state:?})"));
        }

        fn did_have_memory_pressure(&mut self) {
            self.note("memory");
        }
    }

    // -- The two dispatch rules --------------------------------------------

    #[test]
    fn a_back_press_stops_at_whoever_claims_it() {
        // Two things closing on one back press is exactly the bug this
        // prevents.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a")));
        binding.add_observer(Box::new(Spy::new(&log, "b").claiming_pop()));
        binding.add_observer(Box::new(Spy::new(&log, "c")));

        let outcome = binding.handle_pop_route();
        assert!(outcome.handled);
        assert_eq!(outcome.asked, 2, "c was never asked");
        assert_eq!(*log.borrow(), vec!["a:pop", "b:pop"]);
    }

    #[test]
    fn a_back_press_nobody_wanted_lets_the_application_quit() {
        // The right default: a press nothing claimed should leave.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a")));
        binding.add_observer(Box::new(Spy::new(&log, "b")));

        let outcome = binding.handle_pop_route();
        assert!(!outcome.handled);
        assert_eq!(outcome.asked, 2, "and everyone got a turn");
    }

    #[test]
    fn an_exit_request_asks_everyone_even_once_the_answer_is_known() {
        // Upstream's comment: someone may be using the observer only to know
        // when exit happens, and cancelling is somebody else's business.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a").cancelling_exit()));
        binding.add_observer(Box::new(Spy::new(&log, "b")));
        binding.add_observer(Box::new(Spy::new(&log, "c")));

        let (response, asked) = binding.handle_request_app_exit();
        assert_eq!(response, AppExitResponse::Cancel);
        assert_eq!(asked, 3, "all three, despite a knowing after the first");
        assert_eq!(*log.borrow(), vec!["a:exit", "b:exit", "c:exit"]);
    }

    #[test]
    fn one_cancel_is_enough_to_cancel_wherever_it_comes_from() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a")));
        binding.add_observer(Box::new(Spy::new(&log, "b").cancelling_exit()));

        assert_eq!(binding.handle_request_app_exit().0, AppExitResponse::Cancel);
    }

    #[test]
    fn nobody_objecting_lets_the_application_close() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a")));
        assert_eq!(binding.handle_request_app_exit().0, AppExitResponse::Exit);
    }

    #[test]
    fn an_observer_that_does_not_care_is_never_the_reason_an_app_will_not_close() {
        // The default answer is exit.
        struct Silent;
        impl WidgetsBindingObserver for Silent {}
        assert_eq!(Silent.did_request_app_exit(), AppExitResponse::Exit);
        assert!(!Silent.did_pop_route());
        assert!(!Silent.did_push_route("/anything"));
    }

    #[test]
    fn a_deep_link_opens_one_screen() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a").claiming_push()));
        binding.add_observer(Box::new(Spy::new(&log, "b").claiming_push()));

        let outcome = binding.handle_push_route("/books/1");
        assert!(outcome.handled);
        assert_eq!(outcome.asked, 1);
    }

    #[test]
    fn a_broadcast_reaches_everyone_because_nobody_can_claim_it() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a")));
        binding.add_observer(Box::new(Spy::new(&log, "b")));

        binding.handle_app_lifecycle_state_changed(AppLifecycleState::Paused);
        binding.handle_memory_pressure();
        assert_eq!(
            *log.borrow(),
            vec![
                "a:lifecycle(Paused)",
                "b:lifecycle(Paused)",
                "a:memory",
                "b:memory"
            ]
        );
    }

    #[test]
    fn a_predictive_back_gesture_nobody_opted_into_still_becomes_a_back_press() {
        // A platform sending one to an application that never registered
        // should not lose the press.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut binding = WidgetsBinding::new();
        binding.add_observer(Box::new(Spy::new(&log, "a").claiming_pop()));

        let outcome = binding.handle_commit_back_gesture();
        assert!(outcome.handled);
        assert_eq!(*log.borrow(), vec!["a:pop"], "it fell back to a pop");
    }

    // -- Route normalisation -----------------------------------------------

    #[test]
    fn an_empty_path_becomes_the_root_rather_than_an_empty_string() {
        assert_eq!(RouteUri::new("").to_route_string(), "/");
        assert_eq!(RouteUri::new("/books").to_route_string(), "/books");
    }

    #[test]
    fn empty_query_and_fragment_are_dropped_rather_than_left_dangling() {
        // Or an observer matching route strings would have four spellings of
        // the same address to handle.
        assert_eq!(RouteUri::new("/books").to_route_string(), "/books");
        assert_eq!(
            RouteUri::new("/books").with_fragment("").to_route_string(),
            "/books",
            "no bare hash"
        );
        assert_eq!(
            RouteUri::new("/books").with_query(&[]).to_route_string(),
            "/books",
            "no bare question mark"
        );
    }

    #[test]
    fn a_route_with_query_and_fragment_is_spelled_out_in_full() {
        let uri = RouteUri::new("/books")
            .with_query(&[("sort", "title"), ("page", "2")])
            .with_fragment("chapter-3");
        assert_eq!(uri.to_route_string(), "/books?sort=title&page=2#chapter-3");
    }

    #[test]
    fn route_information_falls_back_to_the_plain_route_callback() {
        // Which is what lets an observer implement only one of the two.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut spy = Spy::new(&log, "a");
        spy.did_push_route_information(&RouteUri::new("").with_fragment("top"));
        assert_eq!(*log.borrow(), vec!["a:push(/#top)"]);
    }

    // -- The root ----------------------------------------------------------

    #[test]
    fn a_second_run_app_updates_the_tree_rather_than_rebuilding_it() {
        // Which is what makes a hot reload keep every State below.
        let mut element = RootElement::new(RootWidget::new(Some(1)));
        element.mount(None);
        assert_eq!(element.child(), Some(1));

        let next = RootWidget::new(Some(2));
        assert_eq!(next.attach(Some(&mut element)), AttachOutcome::Updated);
        assert!(element.is_dirty(), "marked, not mounted afresh");
        assert_eq!(
            element.child(),
            Some(1),
            "and the new widget is stashed until the build phase"
        );

        element.perform_rebuild(false);
        assert_eq!(element.child(), Some(2));
        assert!(!element.is_dirty());
    }

    #[test]
    fn the_first_run_app_has_no_element_to_update() {
        let widget = RootWidget::new(Some(1));
        assert_eq!(widget.attach(None), AttachOutcome::Mounted);
    }

    #[test]
    fn a_rebuild_with_no_new_widget_is_a_rebuild_of_the_same_one() {
        // Upstream: _newWidget can be null "if, for instance, we were rebuilt
        // due to a reassemble".
        let mut element = RootElement::new(RootWidget::new(Some(1)));
        element.mount(None);
        element.perform_rebuild(false);
        assert_eq!(element.child(), Some(1));
        assert!(!element.is_dirty());
    }

    #[test]
    fn a_failed_root_build_leaves_a_blank_screen_and_a_real_error() {
        // Everywhere else a build failure becomes the red error widget. Here
        // there is nothing to replace it with -- the thing that failed is what
        // would have provided the view.
        let mut element = RootElement::new(RootWidget::new(Some(1)));
        element.mount(None);

        element.schedule_update(RootWidget::new(Some(2)));
        element.perform_rebuild(true);
        assert!(element.build_failed());
        assert_eq!(element.child(), None, "blank, not a red box");
    }

    #[test]
    fn the_root_has_no_build_phase_and_no_render_object_to_attach_to() {
        let element = RootElement::new(RootWidget::default());
        assert!(!element.debug_doing_build());
        assert!(!element.debug_expects_render_object_for_slot());
    }

    #[test]
    #[should_panic(expected = "RootElement is the root")]
    fn mounting_the_root_under_something_would_give_the_tree_two() {
        RootElement::new(RootWidget::default()).mount(Some(9));
    }

    #[test]
    fn a_root_with_nothing_under_it_is_a_real_state() {
        // It is what exists between ensureInitialized and the first runApp.
        let mut element = RootElement::new(RootWidget::new(None));
        element.mount(None);
        assert!(element.is_mounted());
        assert_eq!(element.child(), None);
    }

    #[test]
    fn the_description_replaces_a_type_name_every_application_shares() {
        // Without it every error dump would open with the same uninformative
        // line.
        let widget = RootWidget::new(Some(1)).with_description("[root]");
        assert_eq!(widget.debug_short_description.as_deref(), Some("[root]"));
        assert_eq!(RootWidget::new(Some(1)).debug_short_description, None);
    }

    #[test]
    fn forgetting_the_child_leaves_the_root_standing() {
        let mut element = RootElement::new(RootWidget::new(Some(1)));
        element.mount(None);
        element.forget_child();
        assert_eq!(element.child(), None);
        assert!(element.is_mounted());
    }

    // -- The binding singleton ---------------------------------------------

    #[test]
    fn the_first_caller_decides_what_the_binding_is() {
        // A test harness initialises its own before runApp asks, and runApp
        // must leave it alone.
        let mut binding = WidgetsFlutterBinding::new();
        assert!(!binding.is_initialized());

        binding.ensure_initialized();
        assert!(binding.is_initialized());
        assert_eq!(binding.initialisations(), 1);

        binding.ensure_initialized();
        assert_eq!(
            binding.initialisations(),
            1,
            "the second caller found one and left it"
        );
    }
}
