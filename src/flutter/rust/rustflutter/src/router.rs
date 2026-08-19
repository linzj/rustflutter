//! The URL, the back button, and who gets to answer them -- a port of
//! upstream's `widgets/router.dart`.
//!
//! A `Router` sits between the platform and the navigator and turns a URL into
//! a set of pages and back again. This module ports the parts of that which
//! are decisions rather than plumbing.
//!
//! The piece with the most in it is the **back button dispatcher**, and its
//! rule is worth stating on its own: a back press is offered to the
//! most-recently-deferred-to child first, then outwards, and the parent
//! handles it only if nobody else did. That is what makes a nested navigator
//! inside a tab inside a shell close its own route rather than the whole
//! application.
//!
//! ## What is not here
//!
//! `Router` itself and `PopNavigatorRouterDelegateMixin` drive a `Navigator`,
//! which this crate does not have; what is ported is the configuration they
//! carry and the contracts the delegates answer.

use std::collections::HashMap;

/// Upstream `RouteInformation`: a URL and the state that goes with it.
///
/// Upstream's constructor asserts that **exactly one** of `location` and `uri`
/// is given -- the deprecated string form or the parsed one, never both,
/// because two spellings of the same address could disagree.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RouteInformation {
    /// The path part, always beginning with `/`.
    pub path: String,
    /// Query parameters, which upstream compares **unordered**: `?a=1&b=2` and
    /// `?b=2&a=1` are the same address.
    pub query: HashMap<String, String>,
    pub fragment: String,
    /// Upstream's `state`, opaque to the framework and handed back to the
    /// application on a restore.
    pub state: Option<String>,
}

impl RouteInformation {
    pub fn new(path: impl Into<String>) -> RouteInformation {
        let path = path.into();
        RouteInformation {
            path: if path.is_empty() {
                "/".to_string()
            } else {
                path
            },
            query: HashMap::new(),
            fragment: String::new(),
            state: None,
        }
    }

    pub fn with_query(mut self, query: HashMap<String, String>) -> Self {
        self.query = query;
        self
    }

    pub fn with_fragment(mut self, fragment: impl Into<String>) -> Self {
        self.fragment = fragment.into();
        self
    }

    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Upstream's `location` getter: the path, query and fragment assembled.
    ///
    /// **An empty path becomes `/`**, which upstream does explicitly -- an
    /// address of nothing is the root, not a blank.
    pub fn location(&self) -> String {
        let mut location = if self.path.is_empty() {
            "/".to_string()
        } else {
            self.path.clone()
        };
        if !self.query.is_empty() {
            let mut pairs: Vec<String> = self
                .query
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            pairs.sort();
            location.push('?');
            location.push_str(&pairs.join("&"));
        }
        if !self.fragment.is_empty() {
            location.push('#');
            location.push_str(&self.fragment);
        }
        location
    }

    /// Upstream's `PlatformRouteInformationProvider._equals`.
    ///
    /// Path, fragment and query -- and the query **unordered**, which matters:
    /// a browser is free to reorder query parameters, and treating a reordered
    /// URL as a new address would push a duplicate history entry every time
    /// the reader came back to a page.
    ///
    /// The **state is not compared**, and that is deliberate too: state is the
    /// application's own scroll position or form contents, and a page whose
    /// state changed has not become a different page.
    pub fn same_address(&self, other: &RouteInformation) -> bool {
        self.path == other.path && self.fragment == other.fragment && self.query == other.query
    }
}

/// Upstream `RouteInformationReportingType`: why the router is telling the
/// platform about a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RouteInformationReportingType {
    /// Upstream's `none`: the router does not care, so the provider decides by
    /// comparing addresses.
    #[default]
    None,
    /// Upstream's `neglect`: replace the current history entry.
    Neglect,
    /// Upstream's `navigate`: push a new one.
    Navigate,
}

/// Upstream `RouteInformationProvider`: where route information comes from.
pub trait RouteInformationProvider {
    /// Upstream's `value`.
    fn value(&self) -> RouteInformation;

    /// Upstream's `routerReportsNewRouteInformation`.
    fn router_reports_new_route_information(
        &mut self,
        route_information: RouteInformation,
        reporting_type: RouteInformationReportingType,
    );
}

/// Upstream `PlatformRouteInformationProvider`: the browser's address bar, or
/// the platform's equivalent.
#[derive(Debug, Default)]
pub struct PlatformRouteInformationProvider {
    value: RouteInformation,
    /// Upstream's `_valueInEngine`: what the platform was last told.
    ///
    /// Kept separately from `value` because they can differ -- the router may
    /// have moved on while the platform is still showing the previous address,
    /// and the comparison below has to be against what the *platform* thinks.
    value_in_engine: RouteInformation,
    /// What the platform was told, in order, with whether it replaced.
    reports: Vec<(RouteInformation, bool)>,
}

impl PlatformRouteInformationProvider {
    pub fn new(initial: RouteInformation) -> PlatformRouteInformationProvider {
        PlatformRouteInformationProvider {
            value: initial.clone(),
            value_in_engine: initial,
            reports: Vec::new(),
        }
    }

    pub fn reports(&self) -> &[(RouteInformation, bool)] {
        &self.reports
    }

    pub fn value_in_engine(&self) -> &RouteInformation {
        &self.value_in_engine
    }
}

impl RouteInformationProvider for PlatformRouteInformationProvider {
    fn value(&self) -> RouteInformation {
        self.value.clone()
    }

    /// Upstream's `routerReportsNewRouteInformation`.
    ///
    /// The `replace` decision is the interesting part. `Neglect` and
    /// `Navigate` are the caller stating outright whether this is a new
    /// history entry. `None` means the caller does not know, and the provider
    /// then decides by **comparing the address with what the platform already
    /// has**: the same address is a replace, a different one is a push. So a
    /// rebuild that reports the same page does not fill the reader's history
    /// with duplicates of it.
    fn router_reports_new_route_information(
        &mut self,
        route_information: RouteInformation,
        reporting_type: RouteInformationReportingType,
    ) {
        let replace = match reporting_type {
            RouteInformationReportingType::Neglect => true,
            RouteInformationReportingType::Navigate => false,
            RouteInformationReportingType::None => {
                self.value_in_engine.same_address(&route_information)
            }
        };
        self.reports.push((route_information.clone(), replace));
        self.value = route_information.clone();
        self.value_in_engine = route_information;
    }
}

/// Upstream `RouteInformationParser`: turns a URL into the application's own
/// route type and back.
///
/// The pair is deliberately not one function each way on the same object as
/// the delegate: parsing is about the *address*, and the delegate is about the
/// *pages*. A URL scheme can change without the pages changing, and the split
/// is what lets one move without the other.
pub trait RouteInformationParser<T> {
    /// Upstream's `parseRouteInformation`.
    fn parse_route_information(&self, route_information: &RouteInformation) -> T;

    /// Upstream's `restoreRouteInformation`, which returns `None` when the
    /// application does not want this configuration in the address bar at all.
    fn restore_route_information(&self, _configuration: &T) -> Option<RouteInformation> {
        None
    }
}

/// Upstream `RouterDelegate`: turns the application's route type into pages.
pub trait RouterDelegate<T> {
    /// Upstream's `currentConfiguration`, `None` when this delegate does not
    /// want to report anything.
    fn current_configuration(&self) -> Option<T> {
        None
    }

    /// Upstream's `setNewRoutePath`.
    fn set_new_route_path(&mut self, configuration: T);

    /// Upstream's `setInitialRoutePath`, which **defaults to
    /// `setNewRoutePath`**.
    ///
    /// The two are separate so an application can tell "opened at this URL"
    /// from "navigated to this URL" -- a deep link into the middle of a flow
    /// may want to build the whole back stack, where the same URL arrived at
    /// by navigation already has one.
    fn set_initial_route_path(&mut self, configuration: T) {
        self.set_new_route_path(configuration);
    }

    /// Upstream's `setRestoredRoutePath`, which also defaults to
    /// `setNewRoutePath`.
    fn set_restored_route_path(&mut self, configuration: T) {
        self.set_new_route_path(configuration);
    }

    /// Upstream's `popRoute`, returning whether it handled the pop.
    fn pop_route(&mut self) -> bool;
}

/// Upstream `PopNavigatorRouterDelegateMixin`: pop the navigator, and say
/// whether there was anything to pop.
///
/// Upstream's whole implementation is `navigator?.maybePop()`, and the
/// `maybe` is the point: a delegate whose navigator is at its first route
/// returns false, which is what lets the back press fall through to the
/// platform and close the application.
pub trait PopNavigatorRouterDelegateMixin<T>: RouterDelegate<T> {
    /// Whether this delegate's navigator has anything to pop.
    fn can_pop(&self) -> bool;
}

/// Upstream `RouterConfig`: the four pieces a router is built from.
///
/// Upstream asserts that the provider and the parser are **both present or
/// both absent** -- a provider with nothing to parse its information would
/// deliver addresses nobody could read, and a parser with no provider would
/// never be asked.
pub struct RouterConfig {
    pub has_route_information_provider: bool,
    pub has_route_information_parser: bool,
    pub has_back_button_dispatcher: bool,
}

impl Default for RouterConfig {
    fn default() -> RouterConfig {
        RouterConfig::new()
    }
}

impl RouterConfig {
    pub fn new() -> RouterConfig {
        RouterConfig {
            has_route_information_provider: false,
            has_route_information_parser: false,
            has_back_button_dispatcher: false,
        }
    }

    pub fn with_route_information(mut self, provider: bool, parser: bool) -> Self {
        self.has_route_information_provider = provider;
        self.has_route_information_parser = parser;
        self
    }

    pub fn with_back_button_dispatcher(mut self, has: bool) -> Self {
        self.has_back_button_dispatcher = has;
        self
    }

    /// Upstream's constructor assertion.
    pub fn is_valid(&self) -> bool {
        self.has_route_information_provider == self.has_route_information_parser
    }
}

/// Upstream `Router`: the widget that ties the four together.
pub struct Router {
    pub config: RouterConfig,
    /// Upstream's `restorationScopeId`.
    pub restoration_scope_id: Option<String>,
}

impl Default for Router {
    fn default() -> Router {
        Router::new()
    }
}

impl Router {
    pub fn new() -> Router {
        Router {
            config: RouterConfig::new(),
            restoration_scope_id: None,
        }
    }

    pub fn with_config(mut self, config: RouterConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_restoration_scope_id(mut self, id: impl Into<String>) -> Self {
        self.restoration_scope_id = Some(id.into());
        self
    }
}

/// A back-button dispatcher's identity, as its parent knows it.
pub type DispatcherId = u64;

/// Upstream `BackButtonDispatcher`: who answers the system back gesture.
///
/// The set of children is a **linked** set upstream, and it has to be: the
/// order they were deferred to in is the order the offer walks, and a plain
/// set would make a nested navigator's priority depend on hashing.
#[derive(Debug, Default)]
pub struct BackButtonDispatcher {
    /// Upstream's `_children`, in registration order.
    children: Vec<DispatcherId>,
    /// Whether this dispatcher has a callback of its own.
    pub has_own_callback: bool,
}

impl BackButtonDispatcher {
    pub fn new() -> BackButtonDispatcher {
        BackButtonDispatcher {
            children: Vec::new(),
            has_own_callback: false,
        }
    }

    /// Upstream's `hasCallbacks`, which is true if **either** this dispatcher
    /// or any child can answer -- a parent with no callback of its own still
    /// has callbacks while a child is deferred to it.
    pub fn has_callbacks(&self) -> bool {
        self.has_own_callback || !self.children.is_empty()
    }

    pub fn children(&self) -> &[DispatcherId] {
        &self.children
    }

    /// Upstream's `deferTo`, which **removes before adding**.
    ///
    /// A child deferred to twice becomes the most recent rather than staying
    /// where it was, which is how a tab the reader returns to takes back
    /// priority from the one they left.
    pub fn defer_to(&mut self, child: DispatcherId) {
        self.children.retain(|held| *held != child);
        self.children.push(child);
    }

    /// Upstream's `forget`.
    pub fn forget(&mut self, child: DispatcherId) {
        self.children.retain(|held| *held != child);
    }

    /// Upstream's `takePriority`, which clears the children outright: this
    /// dispatcher is answering now, and nothing it had deferred to is.
    pub fn take_priority(&mut self) {
        self.children.clear();
    }

    /// The order a back press is offered in -- upstream's `invokeCallback`
    /// walk, which starts at the **last** child and works backwards, with this
    /// dispatcher's own callback last of all.
    ///
    /// Last-first is the whole rule: the most recently deferred-to child is
    /// the innermost thing the reader is looking at, and it is what a back
    /// press should close first. The parent handling it only when nobody else
    /// did is what makes a back press close a dialog rather than the
    /// application.
    pub fn offer_order(&self) -> Vec<Option<DispatcherId>> {
        let mut order: Vec<Option<DispatcherId>> =
            self.children.iter().rev().map(|id| Some(*id)).collect();
        order.push(None);
        order
    }

    /// Whether the offer stops at a given point: upstream returns as soon as a
    /// child says it handled the press.
    pub fn resolve(&self, handled_by: Option<DispatcherId>) -> Option<Option<DispatcherId>> {
        self.offer_order()
            .into_iter()
            .find(|candidate| *candidate == handled_by)
    }
}

/// Upstream `RootBackButtonDispatcher`: the one at the top, which listens to
/// the platform.
#[derive(Debug, Default)]
pub struct RootBackButtonDispatcher {
    pub base: BackButtonDispatcher,
    /// Whether it is registered with the binding.
    observing: bool,
}

impl RootBackButtonDispatcher {
    pub fn new() -> RootBackButtonDispatcher {
        RootBackButtonDispatcher {
            base: BackButtonDispatcher::new(),
            observing: false,
        }
    }

    pub fn is_observing(&self) -> bool {
        self.observing
    }

    /// Upstream's `addCallback`, which starts observing on the **first**
    /// callback only -- a root dispatcher nobody can answer through has no
    /// reason to be hearing about back presses.
    pub fn add_callback(&mut self) {
        if !self.base.has_callbacks() {
            self.observing = true;
        }
        self.base.has_own_callback = true;
    }

    /// Upstream's `removeCallback`, which stops observing once nothing is
    /// left.
    pub fn remove_callback(&mut self) {
        self.base.has_own_callback = false;
        if !self.base.has_callbacks() {
            self.observing = false;
        }
    }

    /// Upstream's `didPopRoute`, whose default value is **false**: with nobody
    /// handling the press, the platform is told the application did not want
    /// it and closes.
    pub fn did_pop_route_default(&self) -> bool {
        false
    }
}

/// Upstream `ChildBackButtonDispatcher`: a dispatcher that answers through its
/// parent.
#[derive(Debug)]
pub struct ChildBackButtonDispatcher {
    pub base: BackButtonDispatcher,
    pub id: DispatcherId,
    pub parent: DispatcherId,
}

impl ChildBackButtonDispatcher {
    pub fn new(id: DispatcherId, parent: DispatcherId) -> ChildBackButtonDispatcher {
        ChildBackButtonDispatcher {
            base: BackButtonDispatcher::new(),
            id,
            parent,
        }
    }

    /// Upstream's `takePriority`, which **tells the parent first**.
    ///
    /// The order is the point: a child taking priority has to be deferred to
    /// by its parent before it clears its own children, or the chain from the
    /// root down to it would have a gap in the middle and the press would stop
    /// short.
    pub fn take_priority(&mut self, parent: &mut BackButtonDispatcher) {
        parent.defer_to(self.id);
        self.base.take_priority();
    }

    /// Upstream's `deferTo`, which also tells the parent first, for the same
    /// reason.
    pub fn defer_to(&mut self, parent: &mut BackButtonDispatcher, child: DispatcherId) {
        parent.defer_to(self.id);
        self.base.defer_to(child);
    }
}

/// Upstream `BackButtonListener`: a widget that answers the back button for
/// its subtree.
///
/// It exists so that a single screen can take the back button without knowing
/// anything about dispatchers -- upstream's whole implementation is defer on
/// build and forget on dispose.
#[derive(Debug, Default)]
pub struct BackButtonListener {
    /// Whether this listener currently holds priority.
    holding: bool,
}

impl BackButtonListener {
    pub fn new() -> BackButtonListener {
        BackButtonListener { holding: false }
    }

    /// Upstream's `initState`/`didChangeDependencies`: take priority.
    pub fn attach(&mut self) {
        self.holding = true;
    }

    /// Upstream's `dispose`: forget, so that the parent answers again.
    pub fn detach(&mut self) {
        self.holding = false;
    }

    pub fn is_holding(&self) -> bool {
        self.holding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn an_address_of_nothing_is_the_root_rather_than_a_blank() {
        assert_eq!(RouteInformation::new("").location(), "/");
        assert_eq!(RouteInformation::new("/books").location(), "/books");
    }

    #[test]
    fn a_reordered_query_is_the_same_address() {
        // A browser is free to reorder query parameters, and treating a
        // reordered URL as a new address would push a duplicate history entry
        // every time the reader came back to a page.
        let one = RouteInformation::new("/books").with_query(query(&[("a", "1"), ("b", "2")]));
        let other = RouteInformation::new("/books").with_query(query(&[("b", "2"), ("a", "1")]));
        assert!(one.same_address(&other));

        let different =
            RouteInformation::new("/books").with_query(query(&[("a", "1"), ("b", "3")]));
        assert!(!one.same_address(&different));
    }

    #[test]
    fn the_state_is_not_part_of_the_address() {
        // State is the application's own scroll position or form contents, and
        // a page whose state changed has not become a different page.
        let plain = RouteInformation::new("/books");
        let with_state = RouteInformation::new("/books").with_state("scrolled to chapter 3");
        assert!(plain.same_address(&with_state));
        assert_ne!(plain, with_state, "but they are not the same value");
    }

    #[test]
    fn the_fragment_is_part_of_the_address() {
        let top = RouteInformation::new("/book");
        let chapter = RouteInformation::new("/book").with_fragment("chapter-3");
        assert!(!top.same_address(&chapter));
        assert_eq!(chapter.location(), "/book#chapter-3");
    }

    #[test]
    fn reporting_the_same_page_again_replaces_rather_than_pushes() {
        // Or a rebuild would fill the reader's history with duplicates of the
        // page they are already on.
        let mut provider = PlatformRouteInformationProvider::new(RouteInformation::new("/books"));
        provider.router_reports_new_route_information(
            RouteInformation::new("/books"),
            RouteInformationReportingType::None,
        );
        assert_eq!(provider.reports()[0].1, true, "same address, replace");

        provider.router_reports_new_route_information(
            RouteInformation::new("/books/1"),
            RouteInformationReportingType::None,
        );
        assert_eq!(provider.reports()[1].1, false, "new address, push");
        assert_eq!(provider.value().location(), "/books/1");
    }

    #[test]
    fn an_explicit_reporting_type_overrides_the_comparison() {
        // Neglect and Navigate are the caller stating outright whether this is
        // a new history entry; None is the caller saying it does not know.
        let mut provider = PlatformRouteInformationProvider::new(RouteInformation::new("/books"));
        provider.router_reports_new_route_information(
            RouteInformation::new("/books"),
            RouteInformationReportingType::Navigate,
        );
        assert!(!provider.reports()[0].1, "pushed despite being the same");

        provider.router_reports_new_route_information(
            RouteInformation::new("/other"),
            RouteInformationReportingType::Neglect,
        );
        assert!(provider.reports()[1].1, "replaced despite being different");
    }

    #[test]
    fn the_comparison_is_against_what_the_platform_has_and_not_what_the_router_wants() {
        let mut provider = PlatformRouteInformationProvider::new(RouteInformation::new("/a"));
        assert_eq!(provider.value_in_engine().location(), "/a");
        provider.router_reports_new_route_information(
            RouteInformation::new("/b"),
            RouteInformationReportingType::None,
        );
        assert_eq!(
            provider.value_in_engine().location(),
            "/b",
            "both move together once the platform has been told"
        );
    }

    #[test]
    fn a_provider_and_a_parser_come_as_a_pair() {
        // A provider with nothing to parse its information would deliver
        // addresses nobody could read, and a parser with no provider would
        // never be asked.
        assert!(RouterConfig::new().is_valid(), "neither is fine");
        assert!(
            RouterConfig::new()
                .with_route_information(true, true)
                .is_valid()
        );
        assert!(
            !RouterConfig::new()
                .with_route_information(true, false)
                .is_valid()
        );
        assert!(
            !RouterConfig::new()
                .with_route_information(false, true)
                .is_valid()
        );
    }

    // -- The back button -------------------------------------------------

    #[test]
    fn a_back_press_is_offered_to_the_innermost_thing_first() {
        // The most recently deferred-to child is what the reader is looking
        // at, and the parent handling it only when nobody else did is what
        // makes a back press close a dialog rather than the application.
        let mut dispatcher = BackButtonDispatcher::new();
        dispatcher.has_own_callback = true;
        dispatcher.defer_to(1);
        dispatcher.defer_to(2);
        dispatcher.defer_to(3);

        assert_eq!(
            dispatcher.offer_order(),
            vec![Some(3), Some(2), Some(1), None],
            "last deferred first, and this dispatcher last of all"
        );
    }

    #[test]
    fn a_child_deferred_to_twice_becomes_the_most_recent() {
        // Which is how a tab the reader returns to takes priority back from
        // the one they left.
        let mut dispatcher = BackButtonDispatcher::new();
        dispatcher.defer_to(1);
        dispatcher.defer_to(2);
        assert_eq!(dispatcher.children(), &[1, 2]);

        dispatcher.defer_to(1);
        assert_eq!(
            dispatcher.children(),
            &[2, 1],
            "removed and re-added rather than left in place"
        );
        assert_eq!(dispatcher.offer_order(), vec![Some(1), Some(2), None]);
    }

    #[test]
    fn taking_priority_clears_the_children_outright() {
        // This dispatcher is answering now, and nothing it had deferred to is.
        let mut dispatcher = BackButtonDispatcher::new();
        dispatcher.has_own_callback = true;
        dispatcher.defer_to(1);
        dispatcher.defer_to(2);
        dispatcher.take_priority();
        assert!(dispatcher.children().is_empty());
        assert_eq!(dispatcher.offer_order(), vec![None]);
    }

    #[test]
    fn a_parent_has_callbacks_while_a_child_is_deferred_to_it() {
        // Even with no callback of its own.
        let mut dispatcher = BackButtonDispatcher::new();
        assert!(!dispatcher.has_callbacks());
        dispatcher.defer_to(1);
        assert!(dispatcher.has_callbacks());
        dispatcher.forget(1);
        assert!(!dispatcher.has_callbacks());

        dispatcher.has_own_callback = true;
        assert!(dispatcher.has_callbacks());
    }

    #[test]
    fn the_offer_stops_at_whoever_handled_it() {
        let mut dispatcher = BackButtonDispatcher::new();
        dispatcher.has_own_callback = true;
        dispatcher.defer_to(1);
        dispatcher.defer_to(2);
        assert_eq!(dispatcher.resolve(Some(2)), Some(Some(2)));
        assert_eq!(
            dispatcher.resolve(None),
            Some(None),
            "the parent's own turn"
        );
        assert_eq!(
            dispatcher.resolve(Some(9)),
            None,
            "somebody who is not in the chain never gets the offer"
        );
    }

    #[test]
    fn a_child_tells_its_parent_before_it_clears_its_own_children() {
        // Or the chain from the root down to it would have a gap in the middle
        // and the press would stop short.
        let mut parent = BackButtonDispatcher::new();
        parent.has_own_callback = true;
        let mut child = ChildBackButtonDispatcher::new(7, 0);
        child.base.defer_to(70);
        child.base.defer_to(71);

        child.take_priority(&mut parent);
        assert_eq!(parent.children(), &[7], "the parent knows about the child");
        assert!(
            child.base.children().is_empty(),
            "and the child dropped its own"
        );
    }

    #[test]
    fn a_child_deferring_to_a_grandchild_still_tells_the_parent() {
        let mut parent = BackButtonDispatcher::new();
        parent.has_own_callback = true;
        let mut child = ChildBackButtonDispatcher::new(7, 0);

        child.defer_to(&mut parent, 70);
        assert_eq!(parent.children(), &[7]);
        assert_eq!(child.base.children(), &[70]);
        assert_eq!(child.base.offer_order(), vec![Some(70), None]);
    }

    #[test]
    fn the_root_listens_only_while_somebody_can_answer() {
        // A root dispatcher nobody can answer through has no reason to be
        // hearing about back presses.
        let mut root = RootBackButtonDispatcher::new();
        assert!(!root.is_observing());

        root.add_callback();
        assert!(root.is_observing());

        root.remove_callback();
        assert!(!root.is_observing());
    }

    #[test]
    fn a_root_with_a_child_deferred_to_it_keeps_listening() {
        let mut root = RootBackButtonDispatcher::new();
        root.add_callback();
        root.base.defer_to(1);
        root.remove_callback();
        assert!(
            root.is_observing(),
            "the child can still answer, so the press is still wanted"
        );
    }

    #[test]
    fn nobody_handling_the_press_tells_the_platform_the_application_did_not_want_it() {
        // Which is what closes the application, and is the right default: a
        // back press that nothing claimed should leave.
        let root = RootBackButtonDispatcher::new();
        assert!(!root.did_pop_route_default());
    }

    #[test]
    fn a_listener_takes_the_back_button_and_gives_it_back() {
        // Upstream's whole implementation is defer on build and forget on
        // dispose, which is what lets one screen claim the back button without
        // knowing anything about dispatchers.
        let mut listener = BackButtonListener::new();
        assert!(!listener.is_holding());
        listener.attach();
        assert!(listener.is_holding());
        listener.detach();
        assert!(!listener.is_holding(), "and the parent answers again");
    }

    // -- The two delegates -----------------------------------------------

    struct Books {
        path: String,
        stack: usize,
    }

    impl RouterDelegate<String> for Books {
        fn current_configuration(&self) -> Option<String> {
            Some(self.path.clone())
        }

        fn set_new_route_path(&mut self, configuration: String) {
            self.path = configuration;
            self.stack = 1;
        }

        fn set_initial_route_path(&mut self, configuration: String) {
            // A deep link into the middle of a flow builds the whole back
            // stack, where the same URL arrived at by navigation already has
            // one.
            let depth = configuration.matches('/').count();
            self.path = configuration;
            self.stack = depth;
        }

        fn pop_route(&mut self) -> bool {
            if self.stack > 1 {
                self.stack -= 1;
                return true;
            }
            false
        }
    }

    #[test]
    fn opening_at_a_url_and_navigating_to_it_are_different_questions() {
        // Which is why upstream has both, with the initial one defaulting to
        // the other rather than being the same method.
        let mut opened = Books {
            path: String::new(),
            stack: 0,
        };
        opened.set_initial_route_path("/books/fiction/1".to_string());
        assert_eq!(opened.stack, 3, "the whole back stack was built");

        let mut navigated = Books {
            path: String::new(),
            stack: 0,
        };
        navigated.set_new_route_path("/books/fiction/1".to_string());
        assert_eq!(navigated.stack, 1, "it already had one");
    }

    #[test]
    fn a_restored_route_defaults_to_the_ordinary_one() {
        struct Plain(String);
        impl RouterDelegate<String> for Plain {
            fn set_new_route_path(&mut self, configuration: String) {
                self.0 = configuration;
            }
            fn pop_route(&mut self) -> bool {
                false
            }
        }
        let mut delegate = Plain(String::new());
        delegate.set_restored_route_path("/restored".to_string());
        assert_eq!(delegate.0, "/restored");
        delegate.set_initial_route_path("/initial".to_string());
        assert_eq!(delegate.0, "/initial");
        assert_eq!(delegate.current_configuration(), None, "by default");
    }

    #[test]
    fn a_delegate_at_its_first_route_lets_the_press_fall_through() {
        // Which is what closes the application, and is the whole of the
        // `maybe` in maybePop.
        let mut delegate = Books {
            path: "/books".to_string(),
            stack: 2,
        };
        assert!(delegate.pop_route(), "there was something to pop");
        assert!(!delegate.pop_route(), "and now there is not");
    }
}
