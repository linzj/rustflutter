//! A port of `widgets/radio_group.dart` and `widgets/raw_radio.dart`.
//!
//! A radio button is the one control that cannot be built alone: what it shows
//! depends on what its siblings are doing, and pressing it changes them. These
//! two files are that arrangement -- a group that owns the value, and radios
//! that register with it and hold no state of their own.
//!
//! The keyboard behaviour is the part worth reading. A radio group is one tab
//! stop, not one per option, and the arrow keys move *and select* within it.
//! Both of those are conventions from the platforms rather than anything that
//! falls out of the widget tree, and each needed a class to arrange.

use crate::scroll_plumbing::ScrollPlatform;
use crate::small_widgets::KeyEventResult;

/// The keys a radio group listens for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioKey {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Space,
}

impl RadioKey {
    /// Upstream binds left and up to "previous", right and down to "next" --
    /// both axes, with no directionality of its own. A group laid out in a row
    /// and one laid out in a column take the same keys, and which node is
    /// actually next comes from the reading order.
    pub fn direction(self) -> Option<bool> {
        match self {
            RadioKey::ArrowRight | RadioKey::ArrowDown => Some(true),
            RadioKey::ArrowLeft | RadioKey::ArrowUp => Some(false),
            RadioKey::Space => None,
        }
    }
}

/// What setting [`RadioClient::set_registry`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryChange {
    pub unregistered: Option<u64>,
    pub registered: Option<u64>,
}

/// Upstream `RadioClient`, a mixin on the radio's `State`.
///
/// It is a mixin rather than an interface because it carries one piece of
/// state: the registry it belongs to. Assigning that is what joins the group,
/// and assigning null is what leaves it.
#[derive(Clone, Debug, PartialEq)]
pub struct RadioClient<T> {
    pub id: u64,
    /// The value this radio stands for.
    pub value: T,
    /// Whether this radio can be interacted with. A disabled radio is skipped
    /// by the keyboard navigation entirely -- it is not a stop you arrow past,
    /// it is not there.
    pub enabled: bool,
    /// Upstream's `tristate`, which `RawRadio` fills from its `toggleable`.
    pub tristate: bool,
    /// Where this radio sits in reading order.
    pub order: i32,
    registry: Option<u64>,
}

impl<T> RadioClient<T> {
    pub fn new(id: u64, value: T, order: i32) -> RadioClient<T> {
        RadioClient {
            id,
            value,
            enabled: true,
            tristate: false,
            order,
            registry: None,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn toggleable(mut self) -> Self {
        self.tristate = true;
        self
    }

    pub fn registry(&self) -> Option<u64> {
        self.registry
    }

    /// Upstream's setter, ported as it is written -- including that the
    /// unregister is conditional on the registry differing while the register
    /// is not. Setting the same registry twice re-registers without
    /// unregistering, which is harmless only because the registry keeps a set.
    pub fn set_registry(&mut self, new_registry: Option<u64>) -> RegistryChange {
        let unregistered = if self.registry != new_registry {
            self.registry
        } else {
            None
        };
        self.registry = new_registry;
        RegistryChange {
            unregistered,
            registered: self.registry,
        }
    }
}

/// Upstream `RadioGroupRegistry`, the interface a radio registers with.
///
/// It is deliberately small: the group owns the value and the radio asks for
/// it. A radio that kept its own idea of whether it was selected would be a
/// second copy of the same fact.
pub trait RadioGroupRegistry<T> {
    fn group_value(&self) -> Option<&T>;
    fn register_client(&mut self, radio: RadioClient<T>);
    fn unregister_client(&mut self, id: u64);
    /// The radio calls this to ask for the group's value to change; it never
    /// changes anything itself.
    fn on_changed(&mut self, value: Option<T>);
}

/// Upstream `RadioGroup`, together with the state that implements the registry.
#[derive(Clone, Debug, PartialEq)]
pub struct RadioGroup<T> {
    pub id: u64,
    pub group_value: Option<T>,
    radios: Vec<RadioClient<T>>,
    focused: Option<u64>,
    changes: Vec<Option<T>>,
}

impl<T: Clone + PartialEq> RadioGroup<T> {
    pub fn new(id: u64, group_value: Option<T>) -> RadioGroup<T> {
        RadioGroup {
            id,
            group_value,
            radios: Vec::new(),
            focused: None,
            changes: Vec::new(),
        }
    }

    pub fn radios(&self) -> &[RadioClient<T>] {
        &self.radios
    }

    pub fn focused(&self) -> Option<u64> {
        self.focused
    }

    pub fn focus(&mut self, id: Option<u64>) {
        self.focused = id;
    }

    /// Every value `onChanged` has been called with, in order.
    pub fn changes(&self) -> &[Option<T>] {
        &self.changes
    }

    /// Upstream's debug check, run after every frame in which a radio
    /// registered. It compares against `< 2`, so a group with **nothing**
    /// selected is fine and only two selected at once is an error -- the
    /// unselected state is a legitimate one, and this policy simply cannot
    /// describe a group that allows more than one.
    pub fn debug_check_only_single_selection(&self) -> bool {
        self.radios
            .iter()
            .filter(|radio| Some(&radio.value) == self.group_value.as_ref())
            .count()
            < 2
    }

    /// The radios that the keyboard will move between: enabled ones, in
    /// reading order.
    fn navigable(&self) -> Vec<&RadioClient<T>> {
        let mut sorted: Vec<&RadioClient<T>> =
            self.radios.iter().filter(|radio| radio.enabled).collect();
        sorted.sort_by_key(|radio| radio.order);
        sorted
    }

    /// Upstream `_selectRadioInDirection`, which does two things at once and
    /// that is the point: **arrowing through a radio group selects as it
    /// goes**, which is the platform convention and the reason arrows are not
    /// left to ordinary focus traversal.
    pub fn select_in_direction(&mut self, forward: bool) -> Option<u64> {
        // Upstream returns early on fewer than two, so a lone radio cannot be
        // re-selected by pressing an arrow at it.
        if self.radios.len() < 2 {
            return None;
        }
        let current = self.focused?;
        // A focused node that is not one of ours -- or a radio that is not
        // interactive -- means the keys were not meant for this group.
        if !self.radios.iter().any(|radio| radio.id == current) {
            return None;
        }

        let ordered: Vec<u64> = {
            let mut ids: Vec<u64> = self.navigable().iter().map(|radio| radio.id).collect();
            if !forward {
                ids.reverse();
            }
            ids
        };
        if ordered.is_empty() {
            return None;
        }

        let next = match ordered.iter().position(|id| *id == current) {
            // The end of the ring wraps around to the beginning.
            Some(index) => ordered[(index + 1) % ordered.len()],
            None => ordered[0],
        };

        let value = self
            .radios
            .iter()
            .find(|radio| radio.id == next)
            .map(|radio| radio.value.clone())?;
        self.on_changed(Some(value));
        self.focused = Some(next);
        Some(next)
    }

    /// Upstream `_toggleFocusedRadio`, bound to the space bar.
    ///
    /// Pressing space on an unselected radio selects it. Pressing it on the
    /// selected one does **nothing** unless the radio is tristate -- a radio
    /// group that could be emptied by fumbling the space bar would be a worse
    /// control than one that cannot.
    pub fn toggle_focused(&mut self) -> bool {
        let Some(current) = self.focused else {
            return false;
        };
        let Some(radio) = self.radios.iter().find(|radio| radio.id == current) else {
            return false;
        };
        let value = radio.value.clone();
        let tristate = radio.tristate;
        if Some(&value) != self.group_value.as_ref() {
            self.on_changed(Some(value));
            return true;
        }
        if tristate {
            self.on_changed(None);
            return true;
        }
        false
    }

    /// Upstream `_RadioGroupShortcutManager.handleKeypress`.
    ///
    /// The guard has its reason written down: with no radio focused the event
    /// is ignored rather than handled, so a text field sitting inside the group
    /// still gets its own arrow keys. A shortcut manager that swallowed
    /// everything in its subtree would make the group uninhabitable.
    pub fn handle_key(&mut self, key: RadioKey) -> KeyEventResult {
        let radio_has_focus = self
            .focused
            .is_some_and(|id| self.radios.iter().any(|radio| radio.id == id));
        if !radio_has_focus {
            return KeyEventResult::Ignored;
        }
        match key.direction() {
            Some(forward) => {
                self.select_in_direction(forward);
            }
            None => {
                self.toggle_focused();
            }
        }
        KeyEventResult::Handled
    }

    /// Upstream `_SkipUnselectedRadioPolicy.sortDescendants`.
    ///
    /// This is what makes a radio group **one tab stop**. Tab reaches the
    /// selected radio and nothing else in the group; the arrows do the rest.
    /// Without it a five-option group would cost five tabs to walk past.
    ///
    /// `descendants` is given in reading order and may include nodes that are
    /// not radios; those are never skipped.
    pub fn sort_descendants(&self, descendants: &[u64], current: Option<u64>) -> Vec<u64> {
        // The selected radio is the one that stays. If nothing is selected, the
        // first radio in reading order stands in for it -- so a group nobody
        // has answered yet is still reachable, and tabbing into it lands on the
        // first option rather than on nothing.
        let mut keeper = self
            .radios
            .iter()
            .find(|radio| Some(&radio.value) == self.group_value.as_ref())
            .map(|radio| radio.id);
        if keeper.is_none() {
            keeper = descendants
                .iter()
                .copied()
                .find(|id| self.radios.iter().any(|radio| radio.id == *id));
        }
        let Some(keeper) = keeper else {
            // No radio is selected or focusable: plain reading order.
            return descendants.to_vec();
        };

        descendants
            .iter()
            .copied()
            .filter(|id| {
                let is_other_radio = self
                    .radios
                    .iter()
                    .any(|radio| radio.id == *id && radio.id != keeper);
                // The focused node is never removed -- upstream notes it cannot
                // be taken out of the sorted result it is the current node of.
                !is_other_radio || Some(*id) == current
            })
            .collect()
    }
}

impl<T: Clone + PartialEq> RadioGroupRegistry<T> for RadioGroup<T> {
    fn group_value(&self) -> Option<&T> {
        self.group_value.as_ref()
    }

    fn register_client(&mut self, radio: RadioClient<T>) {
        // Upstream keeps a `Set`, so registering twice is registering once.
        if let Some(existing) = self.radios.iter_mut().find(|other| other.id == radio.id) {
            *existing = radio;
            return;
        }
        self.radios.push(radio);
    }

    fn unregister_client(&mut self, id: u64) {
        self.radios.retain(|radio| radio.id != id);
    }

    fn on_changed(&mut self, value: Option<T>) {
        self.changes.push(value.clone());
        self.group_value = value;
    }
}

/// Upstream `RawRadio`.
///
/// It holds no selected state. `value == registry.groupValue` is the whole
/// answer, which is why a radio outside a group is not merely unselected but
/// not interactive: with no registry there is nothing to compare against and
/// nothing to tell.
#[derive(Clone, Debug, PartialEq)]
pub struct RawRadio<T> {
    pub value: T,
    /// Upstream's `toggleable`, which becomes the client's `tristate`.
    pub toggleable: bool,
    pub enabled: bool,
    pub autofocus: bool,
    registry: Option<u64>,
}

impl<T: Clone + PartialEq> RawRadio<T> {
    /// Upstream asserts `!enabled || groupRegistry != null`.
    pub fn new(value: T, registry: Option<u64>) -> RawRadio<T> {
        RawRadio {
            value,
            toggleable: false,
            enabled: true,
            autofocus: false,
            registry,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.enabled || self.registry.is_some()
    }

    pub fn with_toggleable(mut self, toggleable: bool) -> Self {
        self.toggleable = toggleable;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The client this radio registers as. Upstream assigns the registry in
    /// `initState` **before** calling `super.initState()`, with the reason
    /// written on it: `ToggleableStateMixin` reads `value` while initialising,
    /// and `value` is a question to the registry.
    pub fn client(&self, id: u64, order: i32) -> RadioClient<T> {
        let mut client = RadioClient::new(id, self.value.clone(), order);
        client.enabled = self.enabled;
        client.tristate = self.toggleable;
        client.set_registry(self.registry);
        client
    }

    /// Upstream's `value` getter.
    pub fn is_selected(&self, group_value: Option<&T>) -> bool {
        Some(&self.value) == group_value
    }

    /// Upstream's `onChanged`, which is null without a registry -- and a null
    /// `onChanged` is what the toggleable machinery reads as "not interactive".
    pub fn is_interactive(&self) -> bool {
        self.registry.is_some() && self.enabled
    }

    /// Upstream `_handleChanged`.
    ///
    /// `false` does nothing at all. A radio cannot un-check itself by being
    /// pressed the ordinary way; only the group, or a toggleable radio's second
    /// press, clears the value.
    pub fn handle_changed(&self, selected: Option<bool>) -> Option<RadioRequest<T>> {
        if !selected.unwrap_or(true) {
            return None;
        }
        if selected.unwrap_or(false) {
            Some(RadioRequest::Select(self.value.clone()))
        } else {
            Some(RadioRequest::Clear)
        }
    }
}

/// What a radio asks the group to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadioRequest<T> {
    Select(T),
    Clear,
}

/// What a screen reader is told about a radio.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RadioSemantics {
    /// Always true: a radio's meaning is that choosing it unchooses the others.
    pub in_mutually_exclusive_group: bool,
    pub checked: bool,
    /// Only set on Apple platforms, where VoiceOver reads a `selected`
    /// property of its own.
    pub selected: Option<bool>,
    /// Only given to an **unselected** radio, and only on Apple platforms.
    /// Upstream's reason: iOS already announces the selected state from
    /// `selected`, so a hint on the selected one would say it twice.
    pub hint: Option<&'static str>,
}

impl<T: Clone + PartialEq> RawRadio<T> {
    pub fn semantics(&self, platform: ScrollPlatform, group_value: Option<&T>) -> RadioSemantics {
        let checked = self.is_selected(group_value);
        match platform {
            ScrollPlatform::IOS | ScrollPlatform::MacOS => RadioSemantics {
                in_mutually_exclusive_group: true,
                checked,
                selected: Some(checked),
                hint: if checked {
                    None
                } else {
                    Some("radio button unselected")
                },
            },
            _ => RadioSemantics {
                in_mutually_exclusive_group: true,
                checked,
                selected: None,
                hint: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three radios in reading order, with the second one chosen.
    fn group() -> RadioGroup<&'static str> {
        let mut group = RadioGroup::new(1, Some("medium"));
        group.register_client(RadioClient::new(10, "small", 0));
        group.register_client(RadioClient::new(11, "medium", 1));
        group.register_client(RadioClient::new(12, "large", 2));
        group
    }

    // -- Registration -----------------------------------------------------------

    #[test]
    fn joining_a_group_is_assigning_the_registry() {
        let mut client = RadioClient::new(10, "small", 0);
        assert_eq!(client.registry(), None);

        let joined = client.set_registry(Some(1));
        assert_eq!(
            joined,
            RegistryChange {
                unregistered: None,
                registered: Some(1),
            }
        );

        let moved = client.set_registry(Some(2));
        assert_eq!(
            moved,
            RegistryChange {
                unregistered: Some(1),
                registered: Some(2),
            },
            "moving groups leaves the old one on the way"
        );

        assert_eq!(
            client.set_registry(None),
            RegistryChange {
                unregistered: Some(2),
                registered: None,
            },
            "and null is how a disposing radio leaves"
        );
    }

    #[test]
    fn assigning_the_same_registry_re_registers_without_leaving_it() {
        // Ported as written: the unregister is conditional and the register is
        // not. It is harmless only because the registry keeps a set.
        let mut client = RadioClient::new(10, "small", 0);
        client.set_registry(Some(1));
        assert_eq!(
            client.set_registry(Some(1)),
            RegistryChange {
                unregistered: None,
                registered: Some(1),
            }
        );

        let mut group = RadioGroup::new(1, None);
        group.register_client(RadioClient::new(10, "small", 0));
        group.register_client(RadioClient::new(10, "small", 0));
        assert_eq!(group.radios().len(), 1, "which the set absorbs");
    }

    #[test]
    fn nothing_selected_is_a_legitimate_state_and_two_selected_is_not() {
        // The check is `< 2`, so it permits an unanswered group and refuses one
        // this policy simply cannot describe.
        let mut none = RadioGroup::new(1, None);
        none.register_client(RadioClient::new(10, "small", 0));
        none.register_client(RadioClient::new(11, "medium", 1));
        assert!(none.debug_check_only_single_selection());

        assert!(group().debug_check_only_single_selection());

        let mut doubled = RadioGroup::new(1, Some("small"));
        doubled.register_client(RadioClient::new(10, "small", 0));
        doubled.register_client(RadioClient::new(11, "small", 1));
        assert!(!doubled.debug_check_only_single_selection());
    }

    // -- The keyboard -----------------------------------------------------------

    #[test]
    fn arrowing_through_a_group_selects_as_it_goes() {
        // Which is the platform convention, and the reason the arrows are not
        // left to ordinary focus traversal.
        let mut group = group();
        group.focus(Some(11));

        assert_eq!(group.select_in_direction(true), Some(12));
        assert_eq!(group.group_value, Some("large"), "moved and chose");
        assert_eq!(group.focused(), Some(12));
        assert_eq!(group.changes(), [Some("large")]);
    }

    #[test]
    fn both_axes_take_the_same_keys() {
        // A group in a row and a group in a column are the same group; which
        // node is next comes from the reading order, not the key.
        assert_eq!(RadioKey::ArrowRight.direction(), Some(true));
        assert_eq!(RadioKey::ArrowDown.direction(), Some(true));
        assert_eq!(RadioKey::ArrowLeft.direction(), Some(false));
        assert_eq!(RadioKey::ArrowUp.direction(), Some(false));
        assert_eq!(RadioKey::Space.direction(), None);
    }

    #[test]
    fn arrowing_off_the_end_wraps_around() {
        let mut group = group();
        group.focus(Some(12));
        assert_eq!(group.select_in_direction(true), Some(10));
        assert_eq!(group.group_value, Some("small"));

        group.focus(Some(10));
        assert_eq!(
            group.select_in_direction(false),
            Some(12),
            "and the other way too"
        );
    }

    #[test]
    fn a_disabled_radio_is_not_a_stop_you_pass_it_is_not_there() {
        let mut group = RadioGroup::new(1, Some("small"));
        group.register_client(RadioClient::new(10, "small", 0));
        group.register_client(RadioClient::new(11, "medium", 1).disabled());
        group.register_client(RadioClient::new(12, "large", 2));
        group.focus(Some(10));

        assert_eq!(group.select_in_direction(true), Some(12));
        assert_eq!(group.group_value, Some("large"));
    }

    #[test]
    fn a_lone_radio_cannot_be_re_chosen_by_arrowing_at_it() {
        let mut group = RadioGroup::new(1, Some("only"));
        group.register_client(RadioClient::new(10, "only", 0));
        group.focus(Some(10));
        assert_eq!(group.select_in_direction(true), None);
        assert!(group.changes().is_empty());
    }

    #[test]
    fn a_key_with_no_radio_focused_is_ignored_rather_than_handled() {
        // Otherwise a text field inside the group would lose its arrow keys to
        // a manager that swallows everything in its subtree.
        let mut group = group();
        group.focus(None);
        assert_eq!(
            group.handle_key(RadioKey::ArrowDown),
            KeyEventResult::Ignored
        );
        assert!(group.changes().is_empty());

        group.focus(Some(99));
        assert_eq!(
            group.handle_key(RadioKey::ArrowDown),
            KeyEventResult::Ignored,
            "and a focused node that is not one of ours is not ours"
        );

        group.focus(Some(10));
        assert_eq!(
            group.handle_key(RadioKey::ArrowDown),
            KeyEventResult::Handled
        );
    }

    #[test]
    fn space_chooses_an_unchosen_radio() {
        let mut group = group();
        group.focus(Some(12));
        assert!(group.toggle_focused());
        assert_eq!(group.group_value, Some("large"));
    }

    #[test]
    fn space_on_the_chosen_radio_does_nothing_unless_it_is_toggleable() {
        // A group that could be emptied by fumbling the space bar would be a
        // worse control than one that cannot.
        let mut group = group();
        group.focus(Some(11));
        assert!(!group.toggle_focused());
        assert_eq!(group.group_value, Some("medium"), "still chosen");
        assert!(group.changes().is_empty());

        let mut toggleable = RadioGroup::new(1, Some("medium"));
        toggleable.register_client(RadioClient::new(11, "medium", 0).toggleable());
        toggleable.register_client(RadioClient::new(12, "large", 1).toggleable());
        toggleable.focus(Some(11));
        assert!(toggleable.toggle_focused());
        assert_eq!(toggleable.group_value, None, "and this one can be emptied");
    }

    #[test]
    fn space_with_nothing_focused_does_nothing() {
        let mut group = group();
        group.focus(None);
        assert!(!group.toggle_focused());
    }

    // -- One tab stop ------------------------------------------------------------

    #[test]
    fn a_radio_group_is_one_tab_stop_rather_than_one_per_option() {
        // Without this a five-option group would cost five tabs to walk past.
        let group = group();
        let descendants = [5, 10, 11, 12, 20];
        assert_eq!(
            group.sort_descendants(&descendants, None),
            [5, 11, 20],
            "the chosen radio, and the things around the group"
        );
    }

    #[test]
    fn an_unanswered_group_is_still_reachable_through_its_first_option() {
        // Otherwise tabbing into a group nobody has answered would land on
        // nothing at all.
        let mut group = RadioGroup::new(1, None);
        group.register_client(RadioClient::new(10, "small", 0));
        group.register_client(RadioClient::new(11, "medium", 1));
        group.register_client(RadioClient::new(12, "large", 2));
        assert_eq!(group.sort_descendants(&[5, 10, 11, 12], None), [5, 10]);
    }

    #[test]
    fn the_focused_radio_is_never_removed_from_the_order() {
        // Upstream notes it cannot be taken out of the sorted result it is the
        // current node of.
        let group = group();
        assert_eq!(
            group.sort_descendants(&[10, 11, 12], Some(12)),
            [11, 12],
            "the chosen one and the focused one"
        );
    }

    #[test]
    fn a_container_with_no_radios_in_it_is_left_in_reading_order() {
        let group = RadioGroup::<&'static str>::new(1, None);
        assert_eq!(group.sort_descendants(&[5, 6, 7], None), [5, 6, 7]);
    }

    // -- The radio itself ---------------------------------------------------------

    #[test]
    fn a_radio_holds_no_selected_state_and_asks_the_group() {
        let radio = RawRadio::new("medium", Some(1));
        assert!(radio.is_selected(Some(&"medium")));
        assert!(!radio.is_selected(Some(&"large")));
        assert!(!radio.is_selected(None));
    }

    #[test]
    fn a_radio_with_no_group_is_not_merely_unselected_but_inert() {
        // With no registry there is nothing to compare against and nothing to
        // tell, which is what upstream's null onChanged means.
        let orphan = RawRadio::new("medium", None);
        assert!(!orphan.is_interactive());
        assert!(!orphan.is_valid(), "and an enabled one is an error");

        let deliberately_disabled = RawRadio::new("medium", None).with_enabled(false);
        assert!(deliberately_disabled.is_valid());
        assert!(!deliberately_disabled.is_interactive());
    }

    #[test]
    fn a_radio_cannot_un_check_itself_by_being_pressed() {
        // Only the group, or a toggleable radio's second press, clears it.
        let radio = RawRadio::new("medium", Some(1));
        assert_eq!(radio.handle_changed(Some(false)), None);
        assert_eq!(
            radio.handle_changed(Some(true)),
            Some(RadioRequest::Select("medium"))
        );
        assert_eq!(radio.handle_changed(None), Some(RadioRequest::Clear));
    }

    #[test]
    fn a_radios_client_carries_its_toggleability_across() {
        let radio = RawRadio::new("medium", Some(1)).with_toggleable(true);
        let client = radio.client(11, 1);
        assert!(client.tristate);
        assert!(client.enabled);
        assert_eq!(client.registry(), Some(1));
        assert_eq!(client.value, "medium");
    }

    // -- What a screen reader hears -------------------------------------------------

    #[test]
    fn an_unselected_radio_gets_a_hint_only_where_the_platform_needs_one() {
        // iOS announces the selected state from its own `selected` property, so
        // a hint on the chosen radio would say it twice.
        let radio = RawRadio::new("large", Some(1));

        let ios = radio.semantics(ScrollPlatform::IOS, Some(&"medium"));
        assert_eq!(ios.selected, Some(false));
        assert_eq!(ios.hint, Some("radio button unselected"));

        let ios_chosen = radio.semantics(ScrollPlatform::IOS, Some(&"large"));
        assert_eq!(ios_chosen.selected, Some(true));
        assert_eq!(ios_chosen.hint, None, "said once, not twice");
    }

    #[test]
    fn other_platforms_are_told_only_that_it_is_checked() {
        let radio = RawRadio::new("large", Some(1));
        for platform in [
            ScrollPlatform::Android,
            ScrollPlatform::Fuchsia,
            ScrollPlatform::Linux,
            ScrollPlatform::Windows,
        ] {
            let semantics = radio.semantics(platform, Some(&"large"));
            assert!(semantics.checked, "{platform:?}");
            assert_eq!(semantics.selected, None, "{platform:?}");
            assert_eq!(semantics.hint, None, "{platform:?}");
        }
    }

    #[test]
    fn every_radio_says_it_is_one_of_a_set() {
        // Which is the whole meaning of the control: choosing it unchooses the
        // others.
        let radio = RawRadio::new("large", Some(1));
        assert!(
            radio
                .semantics(ScrollPlatform::Android, None)
                .in_mutually_exclusive_group
        );
        assert!(
            radio
                .semantics(ScrollPlatform::IOS, None)
                .in_mutually_exclusive_group
        );
    }
}
