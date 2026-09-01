//! The intent/action system, from upstream `widgets/actions.dart`: intents
//! name what the reader meant, actions decide what it does, a dispatcher
//! pairs them, and `Actions` is the widget that scopes a set of actions to
//! a subtree. The crate's focus/keyboard layer (focus.rs, keyboard/)
//! already produces the events; this module gives them something to mean.
//!
//! Recorded divergences (see PORTING_STATUS.md):
//!
//! * `Action.overridable`'s context lookup has no context to look through
//!   yet -- the default action is used directly.

use std::collections::HashMap;
use std::rc::Rc;

use crate::focus::KeyResult;
use crate::keyboard::KeyEvent;

/// Upstream `Intent`: what the reader meant, carried to the action that
/// knows what to do about it. One closed set here -- upstream's concrete
/// intents are the variants.
#[derive(Clone)]
pub enum Intent {
    /// `VoidCallbackIntent`: call the callback.
    VoidCallback { on_call: Rc<dyn Fn()> },
    /// `DoNothingIntent`: deliberately nothing, and the event is consumed.
    DoNothing,
    /// `DoNothingAndStopPropagationIntent`: nothing, and no further handler
    /// runs either.
    DoNothingAndStopPropagation,
    /// `ActivateIntent`: activate the focused thing.
    Activate,
    /// `ButtonActivateIntent`: activate, as a button would.
    ButtonActivate,
    /// `SelectIntent`: select (toggle) the focused thing.
    Select,
    /// `DismissIntent`: dismiss the focused thing.
    Dismiss,
    /// `PrioritizedIntents`: try these in order until one is enabled.
    Prioritized { intents: Vec<Intent> },
    /// `RequestFocusIntent`: give the keyboard to a particular node.
    ///
    /// Upstream carries the `FocusNode`; a node here is its id.
    RequestFocus { id: u64 },
    /// `NextFocusIntent`: move the keyboard on to the next node.
    NextFocus,
    /// `PreviousFocusIntent`.
    PreviousFocus,
    /// `DirectionalFocusIntent`: move the keyboard to whatever lies that way.
    ///
    /// Not the same as [`Intent::NextFocus`]: next is the **reading order**
    /// the traversal policy keeps, while this is a direction on the screen.
    /// The arrows mean this one on every platform but the web, where they
    /// scroll instead -- see `WidgetsApp::default_shortcuts`.
    DirectionalFocus {
        direction: crate::directional_traversal::TraversalDirection,
    },
    /// `ScrollIntent`: scroll the nearest scrollable that way, by a line or
    /// a page.
    Scroll {
        direction: crate::render::AxisDirection,
        increment_type: crate::scrollable_helpers::ScrollIncrementType,
    },
}

/// Upstream `Action<T>`: knows how to serve one kind of intent. The closed
/// intent set makes this one type with a match instead of a generic.
#[derive(Clone)]
pub struct Action {
    pub on_invoke: Rc<dyn Fn(&Intent) -> Option<InvokeResult>>,
    /// Upstream `isEnabled`; a disabled action is skipped by the
    /// dispatcher.
    pub is_enabled: Rc<dyn Fn(&Intent) -> bool>,
    /// Upstream `consumesKey`: whether a key that fired this action should
    /// count as handled even when the action did nothing.
    pub consumes_key: bool,
}

/// What an action answers, upstream's `invoke` return -- a value for the
/// caller when there is one.
pub type InvokeResult = crate::services::Value;

impl Action {
    /// Upstream `CallbackAction`.
    pub fn callback(on_invoke: impl Fn(&Intent) -> Option<InvokeResult> + 'static) -> Action {
        Action {
            on_invoke: Rc::new(on_invoke),
            is_enabled: Rc::new(|_| true),
            consumes_key: true,
        }
    }

    /// Upstream `consumesKey`, as a builder: an action that answers false
    /// lets the key reach whatever would have seen it next.
    pub fn with_consumes_key(mut self, consumes_key: bool) -> Action {
        self.consumes_key = consumes_key;
        self
    }

    /// Upstream `DoNothingAction`.
    pub fn do_nothing() -> Action {
        Action {
            on_invoke: Rc::new(|_| None),
            is_enabled: Rc::new(|_| true),
            consumes_key: true,
        }
    }

    /// Upstream `isEnabled` / `isActionEnabled`.
    pub fn is_enabled(&self, intent: &Intent) -> bool {
        (self.is_enabled)(intent)
    }

    /// Upstream `invoke`.
    pub fn invoke(&self, intent: &Intent) -> Option<InvokeResult> {
        (self.on_invoke)(intent)
    }

    /// Upstream `toKeyEventResult`: a consuming action handled the key,
    /// a non-consuming one lets later handlers see it.
    pub fn to_key_result(&self, intent: &Intent) -> KeyResult {
        if self.consumes_key {
            KeyResult::Handled
        } else {
            KeyResult::Ignored
        }
    }
}

/// Upstream `ActionDispatcher`: walks the action map, honouring
/// `PrioritizedIntents` by trying each in order, and stops at the first
/// enabled action.
#[derive(Clone, Default)]
pub struct ActionDispatcher {
    /// Intent name to action. The name is the variant's -- upstream keys
    /// by `Type`, the closed set makes a string key do.
    pub actions: HashMap<String, Action>,
}

impl ActionDispatcher {
    pub fn new() -> ActionDispatcher {
        ActionDispatcher::default()
    }

    pub fn with_action(mut self, intent_name: &str, action: Action) -> ActionDispatcher {
        self.actions.insert(intent_name.to_string(), action);
        self
    }

    fn first_enabled(&self, intent: &Intent) -> Option<&Action> {
        match intent {
            // Try each in order; the first enabled one wins.
            Intent::Prioritized { intents } => {
                intents.iter().find_map(|inner| self.first_enabled(inner))
            }
            _ => self
                .actions
                .get(intent.action_name())
                .filter(|action| action.is_enabled(intent)),
        }
    }

    /// Whether this map has an enabled action for `intent`, which is the
    /// question [`Actions::maybe_find`] asks of each scope on its way up.
    pub fn has_enabled(&self, intent: &Intent) -> bool {
        self.first_enabled(intent).is_some()
    }

    /// Upstream `invokeAction`: run the action for this intent if there is
    /// one enabled.
    pub fn invoke_action(&self, intent: &Intent) -> Option<InvokeResult> {
        let action = self.first_enabled(intent)?;
        action.invoke(intent)
    }

    /// The key-event half: decide, run, and answer whether the key was
    /// handled -- upstream `Actions.maybeInvoke`.
    pub fn maybe_invoke(&self, intent: &Intent, _event: &KeyEvent) -> KeyResult {
        match self.first_enabled(intent) {
            Some(action) => {
                action.invoke(intent);
                action.to_key_result(intent)
            }
            // Nothing enabled: let the key propagate.
            None => KeyResult::Ignored,
        }
    }
}

impl Intent {
    /// The action-map key for this intent, standing in for upstream's
    /// `runtimeType` keying.
    pub fn action_name(&self) -> &'static str {
        match self {
            Intent::VoidCallback { .. } => "VoidCallback",
            Intent::DoNothing => "DoNothing",
            Intent::DoNothingAndStopPropagation => "DoNothingAndStopPropagation",
            Intent::Activate => "Activate",
            Intent::ButtonActivate => "ButtonActivate",
            Intent::Select => "Select",
            Intent::Dismiss => "Dismiss",
            Intent::Prioritized { .. } => "Prioritized",
            Intent::RequestFocus { .. } => "RequestFocus",
            Intent::NextFocus => "NextFocus",
            Intent::PreviousFocus => "PreviousFocus",
            Intent::DirectionalFocus { .. } => "DirectionalFocus",
            Intent::Scroll { .. } => "Scroll",
        }
    }
}

// -- The focus actions (upstream `widgets/focus_traversal.dart`) --------------

/// Upstream `RequestFocusAction`: gives the keyboard to the intent's node.
///
/// Upstream's is the one action that does not merely call a policy -- it
/// requests focus directly, and its documentation says so, because focusing
/// a node that is not a traversal stop is a thing a caller may legitimately
/// want.
pub struct RequestFocusAction;

impl RequestFocusAction {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Action {
        Action::callback(|intent| {
            if let Intent::RequestFocus { id } = intent {
                crate::focus::focus(*id);
            }
            None
        })
    }
}

/// Upstream `NextFocusAction`.
pub struct NextFocusAction;

impl NextFocusAction {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Action {
        // Upstream's `consumesKey` is false here: if there was nowhere to go,
        // the key should reach the platform so the browser or the shell can
        // move focus out of the application.
        Action::callback(|_| {
            crate::focus::next();
            None
        })
        .with_consumes_key(false)
    }
}

/// Upstream `PreviousFocusAction`.
pub struct PreviousFocusAction;

impl PreviousFocusAction {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Action {
        Action::callback(|_| {
            crate::focus::previous();
            None
        })
        .with_consumes_key(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_action_runs_and_reports_handled() {
        let fired = Rc::new(std::cell::Cell::new(false));
        let action = {
            let fired = Rc::clone(&fired);
            Action::callback(move |_intent| {
                fired.set(true);
                None
            })
        };
        let intent = Intent::Activate;
        assert!(action.is_enabled(&intent));
        action.invoke(&intent);
        assert!(fired.get());
        assert_eq!(action.to_key_result(&intent), KeyResult::Handled);
    }

    #[test]
    fn the_dispatcher_skips_disabled_actions() {
        let mut dispatcher = ActionDispatcher::new();
        let never = Action {
            on_invoke: Rc::new(|_| None),
            is_enabled: Rc::new(|_| false),
            consumes_key: true,
        };
        dispatcher.actions.insert("Activate".to_string(), never);
        // Nothing enabled: the key propagates.
        assert_eq!(
            dispatcher.maybe_invoke(&Intent::Activate, &test_event()),
            KeyResult::Ignored
        );
    }

    #[test]
    fn prioritized_intents_try_in_order() {
        let fired = Rc::new(std::cell::Cell::new(""));
        let first = {
            let fired = Rc::clone(&fired);
            Action::callback(move |_| {
                fired.set("first");
                None
            })
        };
        let second = {
            let fired = Rc::clone(&fired);
            Action::callback(move |_| {
                fired.set("second");
                None
            })
        };
        let mut dispatcher = ActionDispatcher::new();
        // Activate disabled, Select enabled: the prioritized intent falls
        // through to the second.
        dispatcher.actions.insert(
            "Activate".to_string(),
            Action {
                is_enabled: Rc::new(|_| false),
                ..first
            },
        );
        dispatcher.actions.insert("Select".to_string(), second);
        dispatcher.invoke_action(&Intent::Prioritized {
            intents: vec![Intent::Activate, Intent::Select],
        });
        assert_eq!(fired.get(), "second");
    }

    #[test]
    fn do_nothing_consumes_the_key() {
        let mut dispatcher = ActionDispatcher::new();
        dispatcher
            .actions
            .insert("DoNothing".to_string(), Action::do_nothing());
        assert_eq!(
            dispatcher.maybe_invoke(&Intent::DoNothing, &test_event()),
            KeyResult::Handled
        );
    }

    fn test_event() -> KeyEvent {
        KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey(0x04),
            logical: crate::keyboard::LogicalKey::from_char('a'),
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        }
    }
}

// -- The widget (upstream `Actions`) ------------------------------------------

/// Upstream `Actions`: an action map scoped to a subtree.
///
/// Until this existed, [`ActionDispatcher`] was a map somebody had to be
/// holding: every rule in this file was ported and there was **no way to find
/// an action from inside a widget**, so nothing in the crate could raise an
/// intent and have it answered. That is why `app.rs` routes escape by hand
/// instead of turning it into a [`Intent::Dismiss`].
///
/// Upstream is an `InheritedWidget` (`_ActionsScope`) and this is the same
/// thing spelled with [`crate::framework::provide`].
#[derive(Clone, Default)]
pub struct ActionsScope {
    pub dispatcher: Rc<ActionDispatcher>,
}

/// Two scopes are the same scope when they are the **same map**, not when
/// they hold equal maps.
///
/// An action is a closure and closures do not compare, so there is no deep
/// equality to be had here. That is not a shortcut: upstream's
/// `_ActionsScope.updateShouldNotify` compares `actions != oldWidget.actions`,
/// and a Dart `Map` compares by identity too unless somebody made it not.
/// Same idiom as [`crate::components::ScaffoldGeometry`].
impl PartialEq for ActionsScope {
    fn eq(&self, other: &ActionsScope) -> bool {
        Rc::ptr_eq(&self.dispatcher, &other.dispatcher)
    }
}

/// Upstream `Actions`, as its static half: the lookup and the invoke.
pub struct Actions;

impl Actions {
    /// Publishes `dispatcher` over `child`.
    pub fn scope(
        dispatcher: Rc<ActionDispatcher>,
        child: crate::framework::AnyWidget,
    ) -> crate::framework::AnyWidget {
        crate::framework::provide(ActionsScope { dispatcher }, child)
    }

    /// The nearest **enabled** action for `intent`, upstream's
    /// `Actions.maybeFindAction` walk.
    ///
    /// The word doing the work is *enabled*. A scope that has no entry for the
    /// intent, or one whose action says it is not enabled right now, does not
    /// stop the search: it carries on upwards. So a dialog that installs its
    /// own `Dismiss` action shadows the application's only while that action
    /// is enabled, and the moment it is not, escape means what it meant
    /// before -- which is a behaviour, not an implementation detail.
    pub fn maybe_find(
        context: &crate::framework::BuildContext,
        intent: &Intent,
    ) -> Option<Rc<ActionDispatcher>> {
        context
            .inherited_ancestor::<ActionsScope>(|scope| scope.dispatcher.has_enabled(intent))
            .map(|scope| Rc::clone(&scope.dispatcher))
    }

    /// Upstream `Actions.maybeInvoke`: find the action and run it, or answer
    /// `None` because nothing above wanted this intent.
    pub fn maybe_invoke(
        context: &crate::framework::BuildContext,
        intent: &Intent,
    ) -> Option<InvokeResult> {
        Actions::maybe_find(context, intent)?.invoke_action(intent)
    }

    /// The key-event half, upstream's `Actions.handler` reaching a `Shortcuts`
    /// callback: run it and say whether the key was taken.
    pub fn maybe_invoke_key(
        context: &crate::framework::BuildContext,
        intent: &Intent,
        event: &KeyEvent,
    ) -> KeyResult {
        match Actions::maybe_find(context, intent) {
            Some(dispatcher) => dispatcher.maybe_invoke(intent, event),
            // Nothing above claims it, so the key belongs to whoever is next.
            None => KeyResult::Ignored,
        }
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Runs `look` during a build, so the lookup happens with a real
    /// `BuildContext` in a real tree -- which is the only place an ancestor
    /// walk means anything.
    struct Probe<F>(F);

    impl<F: Fn(&mut crate::framework::BuildContext) + 'static> crate::framework::Component
        for Probe<F>
    {
        fn build(
            &self,
            context: &mut crate::framework::BuildContext,
        ) -> crate::framework::AnyWidget {
            (self.0)(context);
            crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
        }
    }

    /// A one-action map that pushes `mark` when invoked.
    fn scope_that(
        intent_name: &str,
        mark: &'static str,
        log: &Rc<RefCell<Vec<&'static str>>>,
        enabled: bool,
    ) -> Rc<ActionDispatcher> {
        let log = Rc::clone(log);
        let mut action = Action::callback(move |_intent| {
            log.borrow_mut().push(mark);
            None
        });
        action.is_enabled = Rc::new(move |_| enabled);
        Rc::new(ActionDispatcher::new().with_action(intent_name, action))
    }

    /// Nests `scopes` outermost-first and invokes `intent` from inside them
    /// all. Answers whether an action was found, and what ran.
    fn ask(scopes: Vec<Rc<ActionDispatcher>>, intent: Intent) -> (bool, Vec<&'static str>) {
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let found = Rc::new(Cell::new(false));
        let seen = Rc::clone(&found);
        let mut child = crate::framework::component(Probe(move |context: &mut _| {
            seen.set(Actions::maybe_find(context, &intent).is_some());
            Actions::maybe_invoke(context, &intent);
        }));
        for dispatcher in scopes.into_iter().rev() {
            child = Actions::scope(dispatcher, child);
        }
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(child);
        let marks = log.borrow().clone();
        (found.get(), marks)
    }

    /// The maps above share one log, so `ask` can only report what its own
    /// scopes pushed. Rebuilt per call, so the tests do not share state.
    fn logged(
        pairs: Vec<(&str, &'static str, bool)>,
    ) -> (Vec<Rc<ActionDispatcher>>, Rc<RefCell<Vec<&'static str>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let scopes = pairs
            .into_iter()
            .map(|(intent, mark, enabled)| scope_that(intent, mark, &log, enabled))
            .collect();
        (scopes, log)
    }

    #[test]
    fn an_intent_nobody_claims_is_not_taken() {
        let (scopes, log) = logged(vec![("Activate", "activate", true)]);
        let mut child = crate::framework::component(Probe(move |context: &mut _| {
            assert!(Actions::maybe_find(context, &Intent::Dismiss).is_none());
            Actions::maybe_invoke(context, &Intent::Dismiss);
        }));
        for dispatcher in scopes.into_iter().rev() {
            child = Actions::scope(dispatcher, child);
        }
        crate::framework::ElementTree::new().rebuild(child);
        assert!(log.borrow().is_empty(), "nothing ran: {:?}", log.borrow());
    }

    #[test]
    fn the_nearest_scope_that_can_take_it_does() {
        let (scopes, log) = logged(vec![("Dismiss", "outer", true), ("Dismiss", "inner", true)]);
        let (found, _) = ask(scopes, Intent::Dismiss);
        assert!(found);
        assert_eq!(
            log.borrow().clone(),
            vec!["inner"],
            "the inner one shadows the outer"
        );
    }

    #[test]
    fn a_scope_that_does_not_handle_it_does_not_stop_the_search() {
        // The whole reason this needed a walk rather than a lookup. An
        // `Actions` that installs two intents must not hide the application's
        // map from a third -- and an inherited lookup, which takes the nearest
        // scope of the type and stops, would have done exactly that.
        let (scopes, log) = logged(vec![
            ("Dismiss", "application", true),
            ("Activate", "dialog", true),
        ]);
        let (found, _) = ask(scopes, Intent::Dismiss);
        assert!(found, "the application's map is still reachable");
        assert_eq!(log.borrow().clone(), vec!["application"]);
    }

    #[test]
    fn a_scope_whose_action_is_disabled_is_walked_past_too() {
        // Upstream asks `isEnabled`, not merely "is there an entry". So a
        // dialog that installs a Dismiss action shadows the application's only
        // while its own is enabled, and the moment it is not, the intent means
        // what it meant before. That is a behaviour rather than a detail.
        let (scopes, log) = logged(vec![
            ("Dismiss", "application", true),
            ("Dismiss", "dialog", false),
        ]);
        let (found, _) = ask(scopes, Intent::Dismiss);
        assert!(found);
        assert_eq!(
            log.borrow().clone(),
            vec!["application"],
            "the disabled one was passed over rather than taken"
        );
    }

    #[test]
    fn a_key_reaching_no_action_is_left_for_whoever_is_next() {
        // `Ignored` and not `Handled`: an intent nobody claimed has to let the
        // key carry on, or the first `Actions` in the tree would swallow every
        // shortcut it had never heard of.
        let (scopes, log) = logged(vec![("Activate", "activate", true)]);
        let result = Rc::new(Cell::new(KeyResult::Handled));
        let seen = Rc::clone(&result);
        let child = crate::framework::component(Probe(move |context: &mut _| {
            seen.set(Actions::maybe_invoke_key(
                context,
                &Intent::Dismiss,
                &crate::keyboard::KeyEvent {
                    change: crate::keyboard::KeyChange::Down,
                    physical: crate::keyboard::PhysicalKey(0),
                    logical: crate::keyboard::LogicalKey::ESCAPE,
                    character: None,
                    synthesized: false,
                    time_stamp_micros: 0,
                },
            ));
        }));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(Actions::scope(Rc::clone(&scopes[0]), child));
        assert_eq!(result.get(), KeyResult::Ignored);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn two_scopes_holding_the_same_map_are_the_same_scope() {
        // An action is a closure and closures do not compare, so identity is
        // all there is -- and it is what upstream compares too.
        let (scopes, _) = logged(vec![("Dismiss", "one", true), ("Dismiss", "two", true)]);
        let same = ActionsScope {
            dispatcher: Rc::clone(&scopes[0]),
        };
        assert!(
            same == ActionsScope {
                dispatcher: Rc::clone(&scopes[0])
            }
        );
        assert!(
            same != ActionsScope {
                dispatcher: Rc::clone(&scopes[1])
            }
        );
    }
}
