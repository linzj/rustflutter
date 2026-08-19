//! The intent/action system, from upstream `widgets/actions.dart`: intents
//! name what the reader meant, actions decide what it does, a dispatcher
//! pairs them, and `Actions` is the widget that scopes a set of actions to
//! a subtree. The crate's focus/keyboard layer (focus.rs, keyboard/)
//! already produces the events; this module gives them something to mean.
//!
//! Recorded divergences (see PORTING_STATUS.md):
//!
//! * `Actions` the widget scopes an action map through the element tree;
//!   here the map is a value handed to the dispatcher, and the widget
//!   spelling arrives with the widget wave that needs it.
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
