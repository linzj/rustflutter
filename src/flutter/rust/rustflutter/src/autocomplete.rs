//! A field that suggests as you type -- a port of upstream's
//! `widgets/autocomplete.dart`.
//!
//! `RawAutocomplete` is the plumbing under Material's `Autocomplete`: a text
//! field, a list of options built from what has been typed, and a highlighted
//! one that the keyboard moves. It draws nothing itself, which is what "raw"
//! means.
//!
//! Two things here are worth more attention than they look:
//!
//! * **the options builder is asynchronous**, so two answers can be in flight
//!   at once and the older one must not win. Upstream numbers every call and
//!   drops any reply that is not the newest -- see [`AutocompleteState::apply_options`].
//! * **the highlight saturates rather than wrapping.** Pressing down on the
//!   last option leaves it on the last option. That is a decision, not an
//!   omission: a list that wraps sends a reader who is holding the key back to
//!   the top without their noticing.
//!
//! ## Where the options view is
//!
//! Upstream's is an `OverlayPortal` positioned against the field's paint
//! transform, and so is this one: [`crate::autocomplete_view`] hosts it. This
//! module is still only the decisions -- focus, whether there is anything to
//! show, the highlight, the keyboard intents -- and the field and the list
//! remain the caller's to build, exactly as upstream's `fieldViewBuilder` and
//! `optionsViewBuilder` are.
//!
//! This paragraph used to say that `crate::overlay` carried the entry list and
//! the portal's z-ordering and that nothing hosted the widgets. Something does
//! now: [`crate::theatre`].

use crate::foundation::ValueNotifier;
use crate::keyboard::LogicalKey;
use crate::shortcuts::ShortcutActivator;
use std::rc::Rc;

/// Upstream `OptionsViewOpenDirection`: which way the list of options grows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OptionsViewOpenDirection {
    /// Below the field, upstream's default.
    #[default]
    Down,
    /// Above it -- what a field near the bottom of the screen needs.
    Up,
    /// Whichever side has more room, decided when the overlay is laid out.
    ///
    /// The odd one of the three: `Up` and `Down` are answers, and this is a
    /// question. See [`OptionsViewOpenDirection::opens_upward`].
    MostSpace,
}

impl OptionsViewOpenDirection {
    /// Upstream's `opensUp`, from `_RawAutocompleteState`'s overlay layout:
    ///
    /// ```dart
    /// final bool opensUp = switch (widget.optionsViewOpenDirection) {
    ///   OptionsViewOpenDirection.up => true,
    ///   OptionsViewOpenDirection.down => false,
    ///   OptionsViewOpenDirection.mostSpace => spaceAbove > spaceBelow,
    /// };
    /// ```
    ///
    /// The two fixed directions ignore both arguments -- a field told to open
    /// upward opens upward with nowhere to put the list, and upstream lets it.
    ///
    /// The comparison is **strict**, so equal room opens downward. That is not
    /// a coin toss going one way: `Down` is the default, and a tie is the case
    /// where nothing has been learned to move away from it.
    pub fn opens_upward(self, space_above: f32, space_below: f32) -> bool {
        match self {
            OptionsViewOpenDirection::Up => true,
            OptionsViewOpenDirection::Down => false,
            OptionsViewOpenDirection::MostSpace => space_above > space_below,
        }
    }

    /// Upstream's `optionsViewMaxHeight`: the room on whichever side was
    /// chosen.
    pub fn max_height(self, space_above: f32, space_below: f32) -> f32 {
        if self.opens_upward(space_above, space_below) {
            space_above
        } else {
            space_below
        }
    }
}

/// Upstream `AutocompletePreviousOptionIntent`: move the highlight up one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutocompletePreviousOptionIntent;

/// Upstream `AutocompleteNextOptionIntent`: move it down one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutocompleteNextOptionIntent;

/// Upstream `AutocompleteFirstOptionIntent`: jump to the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutocompleteFirstOptionIntent;

/// Upstream `AutocompleteLastOptionIntent`: jump to the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutocompleteLastOptionIntent;

/// Upstream `AutocompleteNextPageOptionIntent`: down by a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutocompleteNextPageOptionIntent;

/// Upstream `AutocompletePreviousPageOptionIntent`: up by a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutocompletePreviousPageOptionIntent;

/// The six intents above as one dispatchable value.
///
/// Upstream they are six subclasses of `Intent` and the action map keys on
/// their types. This crate's [`Intent`](crate::actions::Intent) is a closed
/// enum, so the six live beside it as their own types with this enum for
/// dispatch -- which is the same statement about a fixed set, made the way
/// Rust makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutocompleteIntent {
    Previous,
    Next,
    First,
    Last,
    NextPage,
    PreviousPage,
}

macro_rules! autocomplete_intent {
    ($name:ident, $variant:expr) => {
        impl $name {
            /// This intent as the dispatchable value.
            pub fn intent(self) -> AutocompleteIntent {
                $variant
            }
        }

        impl From<$name> for AutocompleteIntent {
            fn from(_intent: $name) -> AutocompleteIntent {
                $variant
            }
        }
    };
}

autocomplete_intent!(
    AutocompletePreviousOptionIntent,
    AutocompleteIntent::Previous
);
autocomplete_intent!(AutocompleteNextOptionIntent, AutocompleteIntent::Next);
autocomplete_intent!(AutocompleteFirstOptionIntent, AutocompleteIntent::First);
autocomplete_intent!(AutocompleteLastOptionIntent, AutocompleteIntent::Last);
autocomplete_intent!(
    AutocompleteNextPageOptionIntent,
    AutocompleteIntent::NextPage
);
autocomplete_intent!(
    AutocompletePreviousPageOptionIntent,
    AutocompleteIntent::PreviousPage
);

/// Upstream `AutocompleteHighlightedOption`: which option is highlighted,
/// published to the option widgets below.
///
/// Upstream it is an `InheritedNotifier` over a `ValueNotifier<int>`, so an
/// option rebuilds when the highlight moves onto or off it and not otherwise.
/// The notifier is shared rather than copied, which is why moving the
/// highlight does not rebuild the field.
pub struct AutocompleteHighlightedOption {
    pub index: Rc<ValueNotifier<usize>>,
}

impl AutocompleteHighlightedOption {
    pub fn new(index: Rc<ValueNotifier<usize>>) -> AutocompleteHighlightedOption {
        AutocompleteHighlightedOption { index }
    }

    /// Upstream's `AutocompleteHighlightedOption.of`.
    pub fn of(&self) -> usize {
        self.index.value()
    }
}

/// Upstream's Apple key bindings for first and last: command with an arrow.
pub fn apple_shortcuts() -> Vec<(ShortcutActivator, AutocompleteIntent)> {
    vec![
        (
            ShortcutActivator::Single {
                key: LogicalKey::ARROW_UP.0,
                control: false,
                shift: false,
                alt: false,
                meta: true,
            },
            AutocompleteIntent::First,
        ),
        (
            ShortcutActivator::Single {
                key: LogicalKey::ARROW_DOWN.0,
                control: false,
                shift: false,
                alt: false,
                meta: true,
            },
            AutocompleteIntent::Last,
        ),
    ]
}

/// The same two, with control instead of command, everywhere else.
pub fn non_apple_shortcuts() -> Vec<(ShortcutActivator, AutocompleteIntent)> {
    vec![
        (
            ShortcutActivator::Single {
                key: LogicalKey::ARROW_UP.0,
                control: true,
                shift: false,
                alt: false,
                meta: false,
            },
            AutocompleteIntent::First,
        ),
        (
            ShortcutActivator::Single {
                key: LogicalKey::ARROW_DOWN.0,
                control: true,
                shift: false,
                alt: false,
                meta: false,
            },
            AutocompleteIntent::Last,
        ),
    ]
}

/// The four bindings every platform has: the bare arrows and the page keys.
pub fn common_shortcuts() -> Vec<(ShortcutActivator, AutocompleteIntent)> {
    let plain = |key: LogicalKey| ShortcutActivator::Single {
        key: key.0,
        control: false,
        shift: false,
        alt: false,
        meta: false,
    };
    vec![
        (plain(LogicalKey::ARROW_UP), AutocompleteIntent::Previous),
        (plain(LogicalKey::ARROW_DOWN), AutocompleteIntent::Next),
        (plain(LogicalKey::PAGE_UP), AutocompleteIntent::PreviousPage),
        (plain(LogicalKey::PAGE_DOWN), AutocompleteIntent::NextPage),
    ]
}

/// Upstream's `_shortcuts`: the common four, plus the platform's two.
pub fn shortcuts_for(apple: bool) -> Vec<(ShortcutActivator, AutocompleteIntent)> {
    let mut shortcuts = common_shortcuts();
    shortcuts.extend(if apple {
        apple_shortcuts()
    } else {
        non_apple_shortcuts()
    });
    shortcuts
}

/// Upstream `RawAutocomplete`: the field, the options and the highlight,
/// without any drawing.
pub struct RawAutocomplete<T> {
    /// Upstream's `optionsViewOpenDirection`.
    pub options_view_open_direction: OptionsViewOpenDirection,
    /// Upstream's `displayStringForOption`, whose default is `toString`. It is
    /// what goes into the field when an option is chosen, so it also decides
    /// whether a later keystroke counts as changing the text.
    pub display_string_for_option: Rc<dyn Fn(&T) -> String>,
}

impl<T: std::fmt::Debug> RawAutocomplete<T> {
    /// Upstream's `defaultStringForOption`.
    pub fn new() -> RawAutocomplete<T> {
        RawAutocomplete {
            options_view_open_direction: OptionsViewOpenDirection::Down,
            display_string_for_option: Rc::new(|option| format!("{option:?}")),
        }
    }
}

impl<T: std::fmt::Debug> Default for RawAutocomplete<T> {
    fn default() -> RawAutocomplete<T> {
        RawAutocomplete::new()
    }
}

impl<T: Clone + PartialEq> RawAutocomplete<T> {
    pub fn with_display_string(
        mut self,
        display: impl Fn(&T) -> String + 'static,
    ) -> RawAutocomplete<T> {
        self.display_string_for_option = Rc::new(display);
        self
    }

    pub fn with_options_view_open_direction(
        mut self,
        direction: OptionsViewOpenDirection,
    ) -> RawAutocomplete<T> {
        self.options_view_open_direction = direction;
        self
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> AutocompleteState<T> {
        AutocompleteState::new()
    }
}

/// Upstream's `_RawAutocompleteState`: everything the field remembers.
pub struct AutocompleteState<T> {
    options: Vec<T>,
    selection: Option<T>,
    /// Upstream's `_lastFieldText`, starting as `None` so that the very first
    /// focus runs the options builder rather than deciding nothing has changed.
    last_field_text: Option<String>,
    /// Upstream's `_onChangedCallId`.
    call_id: u64,
    /// Upstream's `_selecting`: true while the field's text is being set from
    /// a chosen option, so that write does not look like typing.
    selecting: bool,
    has_focus: bool,
    highlighted: usize,
    showing_options: bool,
}

impl<T: Clone + PartialEq> Default for AutocompleteState<T> {
    fn default() -> AutocompleteState<T> {
        AutocompleteState::new()
    }
}

impl<T: Clone + PartialEq> AutocompleteState<T> {
    /// Upstream's `_pageSize`: how far a page key moves the highlight.
    ///
    /// Four, a fixed number rather than a viewport's worth. The list is
    /// usually short and the caller's option view is free to be any height, so
    /// there is no viewport to ask.
    pub const PAGE_SIZE: usize = 4;

    pub fn new() -> AutocompleteState<T> {
        AutocompleteState {
            options: Vec::new(),
            selection: None,
            last_field_text: None,
            call_id: 0,
            selecting: false,
            has_focus: false,
            highlighted: 0,
            showing_options: false,
        }
    }

    pub fn options(&self) -> &[T] {
        &self.options
    }

    pub fn selection(&self) -> Option<&T> {
        self.selection.as_ref()
    }

    pub fn highlighted_option_index(&self) -> usize {
        self.highlighted
    }

    pub fn is_showing_options(&self) -> bool {
        self.showing_options
    }

    /// Upstream's `_canShowOptionsView`: focus **and** something to show.
    pub fn can_show_options_view(&self) -> bool {
        self.has_focus && !self.options.is_empty()
    }

    /// Upstream's `_onFocusChange` and `_updateOptionsViewVisibility`.
    ///
    /// Gaining focus can open the list; losing it always closes the list.
    pub fn set_focus(&mut self, has_focus: bool) {
        if self.has_focus != has_focus {
            self.has_focus = has_focus;
            self.update_options_view_visibility();
        }
    }

    fn update_options_view_visibility(&mut self) {
        if self.can_show_options_view() {
            self.showing_options = true;
        } else if self.showing_options {
            self.showing_options = false;
        }
    }

    /// Upstream's `_onChangedField`, first half: decide whether this edit is
    /// worth asking the options builder about, and take a call number if it is.
    ///
    /// Returns the call number to pass back to [`Self::apply_options`], or
    /// `None` if nothing should be asked. Two reasons for `None`:
    ///
    /// * a selection is being written into the field, which is this widget's
    ///   own doing and not typing;
    /// * the *text* has not changed, so this was a caret move or a selection
    ///   change, and re-running the builder would throw away a highlight the
    ///   reader had moved.
    pub fn field_changed(&mut self, text: &str) -> Option<u64> {
        if self.selecting {
            return None;
        }
        let changed = self.last_field_text.as_deref() != Some(text);
        if changed {
            self.call_id += 1;
        }
        self.last_field_text = Some(text.to_string());
        changed.then_some(self.call_id)
    }

    /// Upstream's `_onChangedField`, second half: the builder's answer arrives.
    ///
    /// **An answer from an older call is dropped.** The builder is
    /// asynchronous, so a reader typing quickly can have two in flight, and
    /// the slower one may well be the earlier one -- without the call number
    /// the field would settle on the options for a prefix the reader has
    /// already finished typing.
    ///
    /// Returns whether the answer was taken.
    pub fn apply_options(
        &mut self,
        call_id: u64,
        options: Vec<T>,
        display: &dyn Fn(&T) -> String,
        text: &str,
    ) -> bool {
        if call_id != self.call_id {
            return false;
        }
        self.options = options;
        self.update_highlight(self.highlighted);
        // A selection whose text the reader has since edited is no longer the
        // selection.
        if let Some(selection) = &self.selection {
            if display(selection) != text {
                self.selection = None;
            }
        }
        self.update_options_view_visibility();
        true
    }

    /// Whether the announcement upstream makes should be made: it speaks only
    /// when the list goes from empty to not, or back, rather than on every
    /// keystroke.
    pub fn should_announce(&self, next: &[T]) -> bool {
        self.options.is_empty() != next.is_empty()
    }

    /// Upstream's `_select`.
    ///
    /// Selecting the option that is already selected does nothing at all --
    /// not even re-writing the field, which would move the caret.
    pub fn select(&mut self, option: T, display: &dyn Fn(&T) -> String) -> Option<String> {
        if self.selection.as_ref() == Some(&option) {
            return None;
        }
        self.selecting = true;
        let text = display(&option);
        self.selection = Some(option);
        self.last_field_text = Some(text.clone());
        if self.showing_options {
            self.showing_options = false;
        }
        self.selecting = false;
        Some(text)
    }

    /// Upstream's `_onFieldSubmitted`: enter takes the highlighted option, but
    /// only while the list is open.
    pub fn field_submitted(&mut self, display: &dyn Fn(&T) -> String) -> Option<String> {
        if !self.showing_options {
            return None;
        }
        let option = self.options.get(self.highlighted)?.clone();
        self.select(option, display)
    }

    /// Upstream's `_updateHighlight`.
    ///
    /// **The highlight saturates; it does not wrap.** Pressing down on the
    /// last option leaves it there. A list that wrapped would carry a reader
    /// holding the key back to the top without their noticing.
    pub fn update_highlight(&mut self, next: usize) {
        self.highlighted = if self.options.is_empty() {
            0
        } else {
            next.min(self.options.len() - 1)
        };
    }

    /// Upstream's six `_highlight*Option` handlers, through one door.
    ///
    /// Returns whether the intent was acted on -- upstream gates all six on
    /// `_canShowOptionsView`, so an arrow key with nothing to show falls
    /// through to whatever else wants it.
    pub fn invoke(&mut self, intent: AutocompleteIntent) -> bool {
        if !self.can_show_options_view() {
            return false;
        }
        self.update_options_view_visibility();
        let last = self.options.len() - 1;
        let next = match intent {
            AutocompleteIntent::Previous => self.highlighted.saturating_sub(1),
            AutocompleteIntent::Next => self.highlighted + 1,
            AutocompleteIntent::First => 0,
            AutocompleteIntent::Last => last,
            AutocompleteIntent::NextPage => self.highlighted + Self::PAGE_SIZE,
            AutocompleteIntent::PreviousPage => self.highlighted.saturating_sub(Self::PAGE_SIZE),
        };
        self.update_highlight(next);
        true
    }

    /// Upstream's `_hideOptions`, reached by the dismiss intent.
    ///
    /// Returns whether the dismissal was used here. When the list is not
    /// showing, upstream passes the intent on up rather than swallowing it --
    /// so escape closes the list if one is open, and otherwise means whatever
    /// it means to the rest of the tree.
    pub fn hide_options(&mut self) -> bool {
        if self.showing_options {
            self.showing_options = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn display(option: &String) -> String {
        option.clone()
    }

    /// A focused field with six options and the list open.
    fn open() -> AutocompleteState<String> {
        let mut state = AutocompleteState::new();
        state.set_focus(true);
        let call = state.field_changed("a").expect("the first edit asks");
        state.apply_options(
            call,
            shown(&["a1", "a2", "a3", "a4", "a5", "a6"]),
            &display,
            "a",
        );
        state
    }

    #[test]
    fn an_older_answer_never_replaces_a_newer_one() {
        // The options builder is asynchronous, so a reader typing quickly can
        // have two answers in flight, and the slower one may well be for the
        // earlier prefix. Without the call number the field settles on options
        // for a prefix the reader has already finished typing.
        let mut state = AutocompleteState::new();
        state.set_focus(true);
        let first = state.field_changed("ba").expect("asks");
        let second = state.field_changed("ban").expect("asks again");
        assert_ne!(first, second);

        assert!(state.apply_options(second, shown(&["banana"]), &display, "ban"));
        assert_eq!(state.options(), &shown(&["banana"]));

        assert!(
            !state.apply_options(first, shown(&["bat", "bar"]), &display, "ba"),
            "the stale answer is dropped"
        );
        assert_eq!(state.options(), &shown(&["banana"]), "and changes nothing");
    }

    #[test]
    fn a_caret_move_is_not_a_reason_to_rebuild_the_options() {
        // Upstream only takes a new call number when the *text* changed. A
        // selection change would otherwise throw away a highlight the reader
        // had moved.
        let mut state: AutocompleteState<String> = AutocompleteState::new();
        state.set_focus(true);
        assert!(state.field_changed("ab").is_some());
        assert!(
            state.field_changed("ab").is_none(),
            "same text, nothing to ask"
        );
        assert!(state.field_changed("abc").is_some());
    }

    #[test]
    fn writing_a_selection_into_the_field_does_not_look_like_typing() {
        // Upstream guards with _selecting, because _select writes the option's
        // text into the controller and that write would otherwise re-run the
        // options builder for text the reader did not type.
        let mut state = open();
        let text = state.select("a3".to_string(), &display).expect("selected");
        assert_eq!(text, "a3");
        assert_eq!(
            state.field_changed("a3"),
            None,
            "the field already knows that text"
        );
    }

    #[test]
    fn selecting_the_same_option_again_does_nothing_at_all() {
        // Not even re-writing the field, which would move the caret.
        let mut state = open();
        assert!(state.select("a2".to_string(), &display).is_some());
        assert!(
            state.select("a2".to_string(), &display).is_none(),
            "already selected"
        );
    }

    #[test]
    fn editing_after_a_selection_un_selects_it() {
        let mut state = open();
        state.select("a3".to_string(), &display);
        assert_eq!(state.selection(), Some(&"a3".to_string()));

        let call = state.field_changed("a3x").expect("that is a change");
        state.apply_options(call, shown(&["a3x1"]), &display, "a3x");
        assert_eq!(state.selection(), None, "the text is no longer the option");
    }

    #[test]
    fn the_highlight_saturates_rather_than_wrapping() {
        // A list that wrapped would carry a reader holding the key back to the
        // top without their noticing.
        let mut state = open();
        assert_eq!(state.highlighted_option_index(), 0);
        assert!(state.invoke(AutocompleteIntent::Previous));
        assert_eq!(state.highlighted_option_index(), 0, "stays at the top");

        state.invoke(AutocompleteIntent::Last);
        assert_eq!(state.highlighted_option_index(), 5);
        state.invoke(AutocompleteIntent::Next);
        assert_eq!(state.highlighted_option_index(), 5, "stays at the bottom");
    }

    #[test]
    fn a_page_is_four_options_and_the_ends_still_hold() {
        // Four, a fixed number rather than a viewport's worth: the caller's
        // option view can be any height, so there is no viewport to ask.
        assert_eq!(AutocompleteState::<String>::PAGE_SIZE, 4);
        let mut state = open();
        state.invoke(AutocompleteIntent::NextPage);
        assert_eq!(state.highlighted_option_index(), 4);
        state.invoke(AutocompleteIntent::NextPage);
        assert_eq!(state.highlighted_option_index(), 5, "clamped to the last");
        state.invoke(AutocompleteIntent::PreviousPage);
        assert_eq!(state.highlighted_option_index(), 1);
        state.invoke(AutocompleteIntent::PreviousPage);
        assert_eq!(state.highlighted_option_index(), 0);
    }

    #[test]
    fn an_arrow_key_with_nothing_to_show_falls_through_to_the_rest_of_the_tree() {
        // All six handlers are gated on _canShowOptionsView, so a field with
        // no matches does not swallow the arrow keys.
        let mut empty = AutocompleteState::<String>::new();
        empty.set_focus(true);
        assert!(!empty.invoke(AutocompleteIntent::Next));

        let mut unfocused: AutocompleteState<String> = AutocompleteState::new();
        let call = unfocused.field_changed("a").expect("asks");
        unfocused.apply_options(call, shown(&["a1"]), &display, "a");
        assert!(
            !unfocused.invoke(AutocompleteIntent::Next),
            "options but no focus"
        );
    }

    #[test]
    fn the_list_needs_both_focus_and_something_to_show() {
        let mut state: AutocompleteState<String> = AutocompleteState::new();
        let call = state.field_changed("a").expect("asks");
        state.apply_options(call, shown(&["a1"]), &display, "a");
        assert!(!state.is_showing_options(), "no focus yet");

        state.set_focus(true);
        assert!(state.is_showing_options());

        // Losing focus always closes it.
        state.set_focus(false);
        assert!(!state.is_showing_options());
    }

    #[test]
    fn options_running_out_closes_the_list() {
        let mut state = open();
        assert!(state.is_showing_options());
        let call = state.field_changed("azzz").expect("asks");
        state.apply_options(call, Vec::new(), &display, "azzz");
        assert!(!state.is_showing_options());
        assert_eq!(
            state.highlighted_option_index(),
            0,
            "and the highlight goes home"
        );
    }

    #[test]
    fn the_announcement_is_made_at_the_edges_and_not_on_every_keystroke() {
        let state = open();
        assert!(
            !state.should_announce(&shown(&["b1", "b2"])),
            "still some results"
        );
        assert!(state.should_announce(&[]), "results have run out");

        let empty = AutocompleteState::<String>::new();
        assert!(empty.should_announce(&shown(&["b1"])), "results arrived");
        assert!(!empty.should_announce(&[]));
    }

    #[test]
    fn enter_takes_the_highlighted_option_only_while_the_list_is_open() {
        let mut state = open();
        state.invoke(AutocompleteIntent::Next);
        assert_eq!(state.field_submitted(&display), Some("a2".to_string()));
        assert!(!state.is_showing_options(), "and the list closes");

        assert_eq!(
            state.field_submitted(&display),
            None,
            "nothing to submit with the list closed"
        );
    }

    #[test]
    fn escape_closes_an_open_list_and_otherwise_means_something_else() {
        // Upstream returns the intent onwards rather than swallowing it, so a
        // dialog behind the field still closes on the second escape.
        let mut state = open();
        assert!(state.hide_options(), "the list was open");
        assert!(!state.hide_options(), "now it is somebody else's escape");
    }

    #[test]
    fn the_platform_moves_first_and_last_between_command_and_control() {
        let apple = shortcuts_for(true);
        let other = shortcuts_for(false);
        assert_eq!(apple.len(), 6);
        assert_eq!(other.len(), 6);

        let first_of = |shortcuts: &[(ShortcutActivator, AutocompleteIntent)]| {
            shortcuts
                .iter()
                .find(|(_, intent)| *intent == AutocompleteIntent::First)
                .map(|(activator, _)| activator.clone())
                .expect("there is a first-option binding")
        };
        assert_eq!(
            first_of(&apple),
            ShortcutActivator::Single {
                key: LogicalKey::ARROW_UP.0,
                control: false,
                shift: false,
                alt: false,
                meta: true,
            }
        );
        assert_eq!(
            first_of(&other),
            ShortcutActivator::Single {
                key: LogicalKey::ARROW_UP.0,
                control: true,
                shift: false,
                alt: false,
                meta: false,
            }
        );

        // The bare arrows and page keys are the same everywhere.
        let bare: Vec<AutocompleteIntent> = common_shortcuts()
            .into_iter()
            .map(|(_, intent)| intent)
            .collect();
        assert_eq!(
            bare,
            vec![
                AutocompleteIntent::Previous,
                AutocompleteIntent::Next,
                AutocompleteIntent::PreviousPage,
                AutocompleteIntent::NextPage,
            ]
        );
    }

    #[test]
    fn the_six_intents_each_name_one_movement() {
        assert_eq!(
            AutocompletePreviousOptionIntent.intent(),
            AutocompleteIntent::Previous
        );
        assert_eq!(
            AutocompleteNextOptionIntent.intent(),
            AutocompleteIntent::Next
        );
        assert_eq!(
            AutocompleteFirstOptionIntent.intent(),
            AutocompleteIntent::First
        );
        assert_eq!(
            AutocompleteLastOptionIntent.intent(),
            AutocompleteIntent::Last
        );
        assert_eq!(
            AutocompleteNextPageOptionIntent.intent(),
            AutocompleteIntent::NextPage
        );
        assert_eq!(
            AutocompletePreviousPageOptionIntent.intent(),
            AutocompleteIntent::PreviousPage
        );
        assert_eq!(
            AutocompleteIntent::from(AutocompleteFirstOptionIntent),
            AutocompleteIntent::First
        );
    }

    #[test]
    fn the_highlighted_option_is_published_by_a_shared_notifier() {
        // Shared rather than copied, which is why moving the highlight
        // rebuilds the options and not the field.
        let notifier = Rc::new(ValueNotifier::new(0usize));
        let published = AutocompleteHighlightedOption::new(notifier.clone());
        assert_eq!(published.of(), 0);
        notifier.set_value(3);
        assert_eq!(published.of(), 3, "the same notifier, not a copy");
    }

    #[test]
    fn the_raw_widget_carries_its_display_rule_and_its_direction() {
        let default: RawAutocomplete<String> = RawAutocomplete::new();
        assert_eq!(
            default.options_view_open_direction,
            OptionsViewOpenDirection::Down
        );

        let upwards = RawAutocomplete::<String>::new()
            .with_options_view_open_direction(OptionsViewOpenDirection::Up)
            .with_display_string(|option: &String| format!("<{option}>"));
        assert_eq!(
            upwards.options_view_open_direction,
            OptionsViewOpenDirection::Up
        );
        assert_eq!((upwards.display_string_for_option)(&"a".to_string()), "<a>");

        let mut state = upwards.create_state();
        state.set_focus(true);
        let call = state.field_changed("a").expect("asks");
        state.apply_options(
            call,
            shown(&["a1"]),
            upwards.display_string_for_option.as_ref(),
            "a",
        );
        assert_eq!(
            state.select("a1".to_string(), upwards.display_string_for_option.as_ref()),
            Some("<a1>".to_string()),
            "the display rule is what goes into the field"
        );
    }
}

// -- material/autocomplete.dart ----------------------------------------------------

/// Upstream `Autocomplete`: the Material dressing over [`RawAutocomplete`].
///
/// It is a `StatelessWidget` that supplies two builders and one constraint, and
/// hands everything else straight through. Almost all of the behaviour lives in
/// the raw widget above.
#[derive(Clone, Debug, PartialEq)]
pub struct Autocomplete {
    pub options_view_open_direction: OptionsViewOpenDirection,
    /// Upstream's `optionsMaxHeight`, applied as a `BoxConstraints(maxHeight:)`
    /// on the options list.
    pub options_max_height: f32,
    pub has_field_view_builder: bool,
    pub has_options_view_builder: bool,
    pub has_initial_value: bool,
    pub has_text_editing_controller: bool,
}

impl Autocomplete {
    /// Upstream's default, and the only number this class contributes.
    pub const DEFAULT_OPTIONS_MAX_HEIGHT: f32 = 200.0;

    pub fn new() -> Autocomplete {
        Autocomplete {
            options_view_open_direction: OptionsViewOpenDirection::Down,
            options_max_height: Autocomplete::DEFAULT_OPTIONS_MAX_HEIGHT,
            // Both builders default to Material implementations rather than to
            // nothing, which is the difference between this and the raw widget.
            has_field_view_builder: true,
            has_options_view_builder: true,
            has_initial_value: false,
            has_text_editing_controller: false,
        }
    }

    /// The height cap is a `maxHeight` constraint, not a fixed height: a short
    /// list is short, and only a long one is cut off and scrolled.
    pub fn options_height_for(&self, natural_height: f32) -> f32 {
        natural_height.min(self.options_max_height)
    }

    /// `Autocomplete` passes `initialValue` and `textEditingController`
    /// straight through, so the three asserts that judge them are
    /// `RawAutocomplete`'s -- and one of them has a shape worth naming.
    ///
    /// ```dart
    /// assert(
    ///   fieldViewBuilder != null ||
    ///       (key != null && focusNode != null && textEditingController != null),
    ///   'Pass in a fieldViewBuilder, or otherwise create a separate field and pass in ...',
    /// ),
    /// assert((focusNode == null) == (textEditingController == null)),
    /// assert(
    ///   !(textEditingController != null && initialValue != null),
    ///   'textEditingController and initialValue cannot be simultaneously defined.',
    /// );
    /// ```
    ///
    /// The middle one is **an equality between two null checks**. Not "at most
    /// one of these" and not "at least one" -- **both or neither.** You may let
    /// the widget own the field's focus and its text, or own both yourself, and
    /// there is no half of it: a focus node you hold and a controller the widget
    /// made would leave the two halves of one field belonging to different
    /// owners.
    ///
    /// Every other multi-argument assert this week has been some flavour of "at
    /// most one" -- the stepper's three extent sources, the chip's two
    /// callbacks, the reorderable list's two. This is the first of the opposite
    /// kind.
    ///
    /// The first assert is the reason `Autocomplete` never has to think about
    /// any of it: it always supplies a `fieldViewBuilder`, so the branch
    /// demanding a key and a focus node and a controller is unreachable from
    /// here.
    pub fn defers_its_field_asserts_to_the_raw_widget() -> bool {
        true
    }

    /// Upstream's `assert((focusNode == null) == (textEditingController == null))`.
    pub fn field_ownership_is_whole(
        has_focus_node: bool,
        has_text_editing_controller: bool,
    ) -> bool {
        has_focus_node == has_text_editing_controller
    }
}

impl Default for Autocomplete {
    fn default() -> Autocomplete {
        Autocomplete::new()
    }
}

#[cfg(test)]
mod material_autocomplete_tests {
    use super::*;

    #[test]
    fn the_height_is_a_cap_and_not_a_size() {
        let field = Autocomplete::new();
        assert_eq!(
            field.options_height_for(60.0),
            60.0,
            "a short list is short"
        );
        assert_eq!(field.options_height_for(600.0), 200.0, "a long one is cut");
        assert_eq!(field.options_height_for(200.0), 200.0);
    }

    #[test]
    fn the_material_wrapper_brings_builders_where_the_raw_one_requires_them() {
        let field = Autocomplete::new();
        assert!(field.has_field_view_builder);
        assert!(field.has_options_view_builder);
        assert_eq!(field.options_max_height, 200.0);
    }

    #[test]
    fn a_field_is_owned_whole_or_not_at_all() {
        // The one assert this week that is not "at most one of these".
        assert!(Autocomplete::field_ownership_is_whole(false, false));
        assert!(Autocomplete::field_ownership_is_whole(true, true));
        assert!(
            !Autocomplete::field_ownership_is_whole(true, false),
            "a focus node you hold and a controller the widget made"
        );
        assert!(!Autocomplete::field_ownership_is_whole(false, true));
    }

    #[test]
    fn it_opens_downwards_unless_told_otherwise() {
        assert_eq!(
            Autocomplete::new().options_view_open_direction,
            OptionsViewOpenDirection::Down
        );
    }
}
