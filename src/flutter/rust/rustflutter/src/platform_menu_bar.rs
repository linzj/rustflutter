//! Menus the *platform* draws -- a port of upstream's
//! `widgets/platform_menu_bar.dart`.
//!
//! This is the one menu bar Flutter does not paint. On macOS the menu bar
//! belongs to the system, so the framework's job is to describe the menus over
//! a channel and then wait to be told which item the reader chose. Everything
//! here is about that description: what a menu looks like as plain data, and
//! how the answers come back.
//!
//! ## What is not here
//!
//! The channel round trip itself. Upstream's `DefaultPlatformMenuDelegate`
//! sends on `SystemChannels.menu` and installs a method-call handler; this
//! crate has the channel machinery in `services/`, but the *serialisation* is
//! what carries the decisions, and it is here in full.

use crate::keyboard::LogicalKey;
use crate::shortcuts::{LockState, ShortcutActivator};

/// Upstream's channel keys, which are the wire format and therefore not free
/// to rename.
pub mod keys {
    pub const ID: &str = "id";
    pub const LABEL: &str = "label";
    pub const TOOLTIP: &str = "tooltip";
    pub const ENABLED: &str = "enabled";
    pub const CHILDREN: &str = "children";
    pub const IS_DIVIDER: &str = "isDivider";
    pub const PLATFORM_PROVIDED_MENU: &str = "platformProvidedMenu";
    pub const SHORTCUT_CHARACTER: &str = "shortcutCharacter";
    pub const SHORTCUT_TRIGGER: &str = "shortcutTrigger";
    pub const SHORTCUT_MODIFIERS: &str = "shortcutModifiers";
}

/// Upstream `ShortcutSerialization`: a keyboard shortcut as the platform wants
/// to hear about it.
///
/// The **bit values are the platform's, not ours**, and they are not in any
/// order a reader would guess: meta is bit 0, shift bit 1, alt bit 2 and
/// control bit 3. Nothing about the framework's own ordering shows through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutSerialization {
    /// Upstream's `ShortcutSerialization.character`.
    ///
    /// **No shift flag at all.** A character already says whether shift was
    /// held -- `$` and `4` are different characters -- so a shift bit beside
    /// it would be a second, possibly disagreeing, answer.
    Character {
        character: char,
        alt: bool,
        control: bool,
        meta: bool,
    },
    /// Upstream's `ShortcutSerialization.modifier`.
    Modifier {
        trigger: LogicalKey,
        alt: bool,
        control: bool,
        meta: bool,
        shift: bool,
    },
}

impl ShortcutSerialization {
    /// Upstream's `_shortcutModifierMeta`.
    pub const MODIFIER_META: i32 = 1 << 0;
    /// Upstream's `_shortcutModifierShift`.
    pub const MODIFIER_SHIFT: i32 = 1 << 1;
    /// Upstream's `_shortcutModifierAlt`.
    pub const MODIFIER_ALT: i32 = 1 << 2;
    /// Upstream's `_shortcutModifierControl`.
    pub const MODIFIER_CONTROL: i32 = 1 << 3;

    pub fn character(character: char) -> ShortcutSerialization {
        ShortcutSerialization::Character {
            character,
            alt: false,
            control: false,
            meta: false,
        }
    }

    pub fn modifier(trigger: LogicalKey) -> ShortcutSerialization {
        ShortcutSerialization::Modifier {
            trigger,
            alt: false,
            control: false,
            meta: false,
            shift: false,
        }
    }

    pub fn with_alt(mut self, value: bool) -> Self {
        match &mut self {
            ShortcutSerialization::Character { alt, .. } => *alt = value,
            ShortcutSerialization::Modifier { alt, .. } => *alt = value,
        }
        self
    }

    pub fn with_control(mut self, value: bool) -> Self {
        match &mut self {
            ShortcutSerialization::Character { control, .. } => *control = value,
            ShortcutSerialization::Modifier { control, .. } => *control = value,
        }
        self
    }

    pub fn with_meta(mut self, value: bool) -> Self {
        match &mut self {
            ShortcutSerialization::Character { meta, .. } => *meta = value,
            ShortcutSerialization::Modifier { meta, .. } => *meta = value,
        }
        self
    }

    /// Setting shift on a character shortcut does nothing, because upstream's
    /// character constructor has no shift parameter to set.
    pub fn with_shift(mut self, value: bool) -> Self {
        if let ShortcutSerialization::Modifier { shift, .. } = &mut self {
            *shift = value;
        }
        self
    }

    /// Upstream's `shift` getter, which is `null` for a character shortcut --
    /// not `false`. "The question does not apply here" is a different answer
    /// from "shift was not held".
    pub fn shift(&self) -> Option<bool> {
        match self {
            ShortcutSerialization::Character { .. } => None,
            ShortcutSerialization::Modifier { shift, .. } => Some(*shift),
        }
    }

    /// The modifier bitmask upstream builds into `_internal`.
    pub fn modifiers(&self) -> i32 {
        let (alt, control, meta, shift) = match self {
            ShortcutSerialization::Character {
                alt, control, meta, ..
            } => (*alt, *control, *meta, false),
            ShortcutSerialization::Modifier {
                alt,
                control,
                meta,
                shift,
                ..
            } => (*alt, *control, *meta, *shift),
        };
        (if control { Self::MODIFIER_CONTROL } else { 0 })
            | (if alt { Self::MODIFIER_ALT } else { 0 })
            | (if meta { Self::MODIFIER_META } else { 0 })
            | (if shift { Self::MODIFIER_SHIFT } else { 0 })
    }

    /// Upstream's `toChannelRepresentation`, as key/value pairs.
    ///
    /// A character shortcut sends the character; a modifier shortcut sends the
    /// trigger's key id. Never both -- the platform picks its own way of
    /// matching, and giving it two would let the two disagree.
    pub fn to_channel_representation(&self) -> Vec<(&'static str, ChannelValue)> {
        match self {
            ShortcutSerialization::Character { character, .. } => vec![
                (
                    keys::SHORTCUT_CHARACTER,
                    ChannelValue::Text(character.to_string()),
                ),
                (
                    keys::SHORTCUT_MODIFIERS,
                    ChannelValue::Int(self.modifiers()),
                ),
            ],
            ShortcutSerialization::Modifier { trigger, .. } => vec![
                (keys::SHORTCUT_TRIGGER, ChannelValue::Int(trigger.0 as i32)),
                (
                    keys::SHORTCUT_MODIFIERS,
                    ChannelValue::Int(self.modifiers()),
                ),
            ],
        }
    }

    /// Upstream's assertion on the modifier constructor: **a modifier key may
    /// not be the trigger**.
    ///
    /// Its message says what to do instead -- use the boolean parameters --
    /// and the reason is that a shortcut of "control" would be ambiguous
    /// between "control is held" and "control was pressed".
    pub fn trigger_is_allowed(trigger: LogicalKey) -> bool {
        ![
            LogicalKey::ALT,
            LogicalKey::ALT_LEFT,
            LogicalKey::ALT_RIGHT,
            LogicalKey::CONTROL,
            LogicalKey::CONTROL_LEFT,
            LogicalKey::CONTROL_RIGHT,
            LogicalKey::META,
            LogicalKey::META_LEFT,
            LogicalKey::META_RIGHT,
            LogicalKey::SHIFT,
            LogicalKey::SHIFT_LEFT,
            LogicalKey::SHIFT_RIGHT,
        ]
        .contains(&trigger)
    }
}

/// A value as it goes over the channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelValue {
    Text(String),
    Int(i32),
    Bool(bool),
    List(Vec<Vec<(&'static str, ChannelValue)>>),
}

/// Upstream `PlatformProvidedMenuItemType`: menus the platform supplies
/// itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformProvidedMenuItemType {
    About,
    Quit,
    ServicesSubmenu,
    Hide,
    HideOtherApplications,
    ShowAllApplications,
    StartSpeaking,
    StopSpeaking,
    ToggleFullScreen,
    MinimizeWindow,
    ZoomWindow,
    ArrangeWindowsInFront,
}

/// Upstream `PlatformProvidedMenuItem`: an item the platform draws and
/// handles.
///
/// The application does not say what these *do* -- there is no callback --
/// because the platform already knows. What the framework contributes is
/// where in the menu they go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformProvidedMenuItem {
    pub menu_type: PlatformProvidedMenuItemType,
    pub enabled: bool,
}

impl PlatformProvidedMenuItem {
    pub fn new(menu_type: PlatformProvidedMenuItemType) -> PlatformProvidedMenuItem {
        PlatformProvidedMenuItem {
            menu_type,
            enabled: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's static `hasMenu`.
    ///
    /// **Only macOS has any of them**, and it has all twelve. Every other
    /// platform returns false for every type, which is not a gap waiting to be
    /// filled -- these are the items of the macOS application menu, and there
    /// is no such menu elsewhere.
    pub fn has_menu(is_macos: bool, _menu_type: PlatformProvidedMenuItemType) -> bool {
        is_macos
    }
}

/// Upstream `PlatformMenuItem`: one line of a platform menu.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMenuItem {
    pub label: String,
    pub tooltip: Option<String>,
    pub shortcut: Option<ShortcutSerialization>,
    /// Whether the application gave this item something to do. Upstream's
    /// serialisation derives `enabled` from exactly this.
    pub has_action: bool,
}

impl PlatformMenuItem {
    pub fn new(label: impl Into<String>) -> PlatformMenuItem {
        PlatformMenuItem {
            label: label.into(),
            tooltip: None,
            shortcut: None,
            has_action: false,
        }
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_shortcut(mut self, shortcut: ShortcutSerialization) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn with_action(mut self) -> Self {
        self.has_action = true;
        self
    }

    /// Upstream's static `serialize`.
    ///
    /// **`enabled` is computed, not stored**: an item is enabled exactly when
    /// the application gave it something to do. There is no way to send a
    /// menu item that looks pressable and is not, which is the right
    /// restriction for a menu somebody else draws.
    ///
    /// The tooltip is omitted rather than sent as null when absent, because
    /// the platform's own default tooltip is not the same as no tooltip.
    pub fn serialize(&self, id: i32) -> Vec<(&'static str, ChannelValue)> {
        let mut entry = vec![
            (keys::ID, ChannelValue::Int(id)),
            (keys::LABEL, ChannelValue::Text(self.label.clone())),
        ];
        if let Some(tooltip) = &self.tooltip {
            entry.push((keys::TOOLTIP, ChannelValue::Text(tooltip.clone())));
        }
        entry.push((keys::ENABLED, ChannelValue::Bool(self.has_action)));
        if let Some(shortcut) = &self.shortcut {
            entry.extend(shortcut.to_channel_representation());
        }
        entry
    }
}

/// What a node of a platform menu tree is.
#[derive(Clone, Debug, PartialEq)]
pub enum PlatformMenuNode {
    Item(PlatformMenuItem),
    Group(PlatformMenuItemGroup),
    Menu(PlatformMenu),
    Provided(PlatformProvidedMenuItem),
}

/// Upstream `PlatformMenuItemGroup`: items with a divider on each side.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMenuItemGroup {
    pub members: Vec<PlatformMenuNode>,
}

impl PlatformMenuItemGroup {
    pub fn new(members: Vec<PlatformMenuNode>) -> PlatformMenuItemGroup {
        debug_assert!(
            !members.is_empty(),
            "there must be at least one member in a PlatformMenuItemGroup"
        );
        PlatformMenuItemGroup { members }
    }
}

/// Upstream `PlatformMenu`: a submenu.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMenu {
    pub label: String,
    pub menus: Vec<PlatformMenuNode>,
}

impl PlatformMenu {
    pub fn new(label: impl Into<String>, menus: Vec<PlatformMenuNode>) -> PlatformMenu {
        PlatformMenu {
            label: label.into(),
            menus,
        }
    }
}

/// Upstream `PlatformMenuBar`: the widget that owns the platform's menus.
///
/// It draws nothing. Upstream's build returns its child untouched; the menus
/// go out over the channel as a side effect of being described.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PlatformMenuBar {
    pub menus: Vec<PlatformMenuNode>,
}

impl PlatformMenuBar {
    pub fn new(menus: Vec<PlatformMenuNode>) -> PlatformMenuBar {
        PlatformMenuBar { menus }
    }
}

/// Upstream `PlatformMenuDelegate`: what sends the menus to the platform.
pub trait PlatformMenuDelegate {
    /// Upstream's `setMenus`.
    fn set_menus(&mut self, menus: &[PlatformMenuNode]);

    /// Upstream's `clearMenus`, which upstream implements as `setMenus([])` --
    /// the platform is told about an empty menu bar rather than told nothing.
    fn clear_menus(&mut self) {
        self.set_menus(&[]);
    }

    /// Upstream's `debugLockDelegate`, which enforces one menu bar per
    /// delegate.
    fn lock(&mut self, context: u64) -> bool;

    /// Upstream's `debugUnlockDelegate`.
    fn unlock(&mut self, context: u64) -> bool;
}

/// Upstream `DefaultPlatformMenuDelegate`: serialises the tree and remembers
/// which id was which item.
#[derive(Debug, Default)]
pub struct DefaultPlatformMenuDelegate {
    serial: i32,
    /// Upstream's `_idMap`, from the id handed to the platform back to the
    /// item, so a selection can be routed to the right callback.
    id_map: Vec<(i32, String)>,
    locked_context: Option<u64>,
    /// The last thing sent, which is what upstream hands the channel under the
    /// window key `"0"`.
    last_sent: Option<Vec<Vec<(&'static str, ChannelValue)>>>,
}

impl DefaultPlatformMenuDelegate {
    pub fn new() -> DefaultPlatformMenuDelegate {
        DefaultPlatformMenuDelegate::default()
    }

    /// Upstream's `_getId`: an ever-increasing serial.
    ///
    /// **Every call takes a new one, even for the same item.** A group takes
    /// two -- one per divider -- and both map back to the group, which is what
    /// lets the platform tell the framework which divider it touched even
    /// though a reader never touches one.
    fn next_id(&mut self, label: &str) -> i32 {
        self.serial += 1;
        self.id_map.push((self.serial, label.to_string()));
        self.serial
    }

    pub fn id_map(&self) -> &[(i32, String)] {
        &self.id_map
    }

    pub fn last_sent(&self) -> Option<&Vec<Vec<(&'static str, ChannelValue)>>> {
        self.last_sent.as_ref()
    }

    /// The item an id belongs to, which is how a selection is routed back.
    pub fn item_for(&self, id: i32) -> Option<&str> {
        self.id_map
            .iter()
            .find(|(held, _)| *held == id)
            .map(|(_, label)| label.as_str())
    }

    /// Serialise one node, which may produce several entries.
    fn serialize_node(
        &mut self,
        node: &PlatformMenuNode,
    ) -> Vec<Vec<(&'static str, ChannelValue)>> {
        match node {
            PlatformMenuNode::Item(item) => {
                let id = self.next_id(&item.label);
                vec![item.serialize(id)]
            }
            PlatformMenuNode::Provided(provided) => {
                let id = self.next_id("");
                vec![vec![
                    (keys::ID, ChannelValue::Int(id)),
                    (
                        keys::PLATFORM_PROVIDED_MENU,
                        ChannelValue::Int(provided.menu_type as i32),
                    ),
                    (keys::ENABLED, ChannelValue::Bool(provided.enabled)),
                ]]
            }
            PlatformMenuNode::Group(group) => {
                // Upstream's `PlatformMenuItemGroup.serialize`: a divider on
                // each side, and a fresh id for each.
                let mut entries = vec![vec![
                    (keys::ID, ChannelValue::Int(self.next_id(""))),
                    (keys::IS_DIVIDER, ChannelValue::Bool(true)),
                ]];
                for member in &group.members {
                    entries.extend(self.serialize_node(member));
                }
                entries.push(vec![
                    (keys::ID, ChannelValue::Int(self.next_id(""))),
                    (keys::IS_DIVIDER, ChannelValue::Bool(true)),
                ]);
                entries
            }
            PlatformMenuNode::Menu(menu) => {
                let mut children = Vec::new();
                for child in &menu.menus {
                    children.extend(self.serialize_node(child));
                }
                let children = Self::tidy_dividers(children);
                let id = self.next_id(&menu.label);
                vec![vec![
                    (keys::ID, ChannelValue::Int(id)),
                    (keys::LABEL, ChannelValue::Text(menu.label.clone())),
                    (keys::ENABLED, ChannelValue::Bool(!menu.menus.is_empty())),
                    (keys::CHILDREN, ChannelValue::List(children)),
                ]]
            }
        }
    }

    /// Upstream's divider tidying inside `PlatformMenu.serialize`.
    ///
    /// Upstream's comment says why it is done this way: rather than type-check
    /// for groups, it filters the *result* -- because groups may be
    /// interleaved with non-groups, and a non-group may add a divider too. So
    /// a leading divider goes, a divider straight after another goes, and a
    /// trailing one goes. A menu never opens or closes with a rule across it.
    pub fn tidy_dividers(
        entries: Vec<Vec<(&'static str, ChannelValue)>>,
    ) -> Vec<Vec<(&'static str, ChannelValue)>> {
        let is_divider = |entry: &Vec<(&'static str, ChannelValue)>| {
            entry
                .iter()
                .any(|(key, value)| *key == keys::IS_DIVIDER && *value == ChannelValue::Bool(true))
        };
        let mut tidied: Vec<Vec<(&'static str, ChannelValue)>> = Vec::new();
        for entry in entries {
            if is_divider(&entry) {
                match tidied.last() {
                    None => continue,
                    Some(previous) if is_divider(previous) => continue,
                    _ => {}
                }
            }
            tidied.push(entry);
        }
        if tidied.last().is_some_and(is_divider) {
            tidied.pop();
        }
        tidied
    }
}

impl PlatformMenuDelegate for DefaultPlatformMenuDelegate {
    /// Upstream's `setMenus`, which clears the id map first: the ids are only
    /// meaningful for the menus currently on screen.
    fn set_menus(&mut self, menus: &[PlatformMenuNode]) {
        self.id_map.clear();
        let mut representation = Vec::new();
        for menu in menus {
            representation.extend(self.serialize_node(menu));
        }
        self.last_sent = Some(representation);
    }

    /// Upstream's `debugLockDelegate`: **one menu bar per delegate**.
    ///
    /// Two menu bars sharing one would each overwrite the other's menus, and
    /// the reader would see whichever built last with no way to tell why.
    fn lock(&mut self, context: u64) -> bool {
        if self.locked_context.is_some_and(|held| held != context) {
            return false;
        }
        self.locked_context = Some(context);
        true
    }

    /// Upstream's `debugUnlockDelegate`, whose comment says it is fine to
    /// unlock a delegate that is not locked, but not for a different context
    /// to do the unlocking.
    fn unlock(&mut self, context: u64) -> bool {
        if self.locked_context.is_some_and(|held| held != context) {
            return false;
        }
        self.locked_context = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str) -> PlatformMenuNode {
        PlatformMenuNode::Item(PlatformMenuItem::new(label).with_action())
    }

    fn value<'a>(entry: &'a [(&'static str, ChannelValue)], key: &str) -> Option<&'a ChannelValue> {
        entry
            .iter()
            .find(|(held, _)| *held == key)
            .map(|(_, value)| value)
    }

    #[test]
    fn the_modifier_bits_are_the_platforms_and_not_in_any_guessable_order() {
        // Meta is bit 0, shift bit 1, alt bit 2 and control bit 3. Nothing
        // about the framework's own ordering shows through, and a port that
        // renumbered them would send shortcuts the platform reads as different
        // ones.
        assert_eq!(ShortcutSerialization::MODIFIER_META, 1);
        assert_eq!(ShortcutSerialization::MODIFIER_SHIFT, 2);
        assert_eq!(ShortcutSerialization::MODIFIER_ALT, 4);
        assert_eq!(ShortcutSerialization::MODIFIER_CONTROL, 8);

        let all = ShortcutSerialization::modifier(LogicalKey::KEY_S)
            .with_meta(true)
            .with_shift(true)
            .with_alt(true)
            .with_control(true);
        assert_eq!(all.modifiers(), 15);
    }

    #[test]
    fn a_character_shortcut_has_no_shift_flag_because_the_character_says_it() {
        // $ and 4 are different characters, so a shift bit beside one would be
        // a second and possibly disagreeing answer.
        let dollar = ShortcutSerialization::character('$');
        assert_eq!(dollar.shift(), None, "the question does not apply");
        assert_eq!(
            dollar.clone().with_shift(true).shift(),
            None,
            "and setting it does nothing"
        );
        assert_eq!(dollar.modifiers(), 0);

        // Where a modifier shortcut answers it either way.
        let key = ShortcutSerialization::modifier(LogicalKey::KEY_S);
        assert_eq!(key.shift(), Some(false));
        assert_eq!(key.with_shift(true).shift(), Some(true));
    }

    #[test]
    fn a_shortcut_sends_a_character_or_a_trigger_and_never_both() {
        // The platform picks its own way of matching, and giving it two would
        // let the two disagree.
        let character = ShortcutSerialization::character('n').with_meta(true);
        let sent = character.to_channel_representation();
        assert_eq!(
            value(&sent, keys::SHORTCUT_CHARACTER),
            Some(&ChannelValue::Text("n".to_string()))
        );
        assert_eq!(value(&sent, keys::SHORTCUT_TRIGGER), None);
        assert_eq!(
            value(&sent, keys::SHORTCUT_MODIFIERS),
            Some(&ChannelValue::Int(ShortcutSerialization::MODIFIER_META))
        );

        let trigger = ShortcutSerialization::modifier(LogicalKey::ARROW_UP).with_control(true);
        let sent = trigger.to_channel_representation();
        assert_eq!(value(&sent, keys::SHORTCUT_CHARACTER), None);
        assert_eq!(
            value(&sent, keys::SHORTCUT_TRIGGER),
            Some(&ChannelValue::Int(LogicalKey::ARROW_UP.0 as i32))
        );
    }

    #[test]
    fn a_modifier_key_may_not_be_the_trigger() {
        // A shortcut of "control" would be ambiguous between "control is held"
        // and "control was pressed", and upstream's message says to use the
        // boolean parameters instead.
        for refused in [
            LogicalKey::ALT,
            LogicalKey::ALT_LEFT,
            LogicalKey::ALT_RIGHT,
            LogicalKey::CONTROL,
            LogicalKey::CONTROL_LEFT,
            LogicalKey::CONTROL_RIGHT,
            LogicalKey::META,
            LogicalKey::META_LEFT,
            LogicalKey::META_RIGHT,
            LogicalKey::SHIFT,
            LogicalKey::SHIFT_LEFT,
            LogicalKey::SHIFT_RIGHT,
        ] {
            assert!(
                !ShortcutSerialization::trigger_is_allowed(refused),
                "{refused:?}"
            );
        }
        assert!(ShortcutSerialization::trigger_is_allowed(LogicalKey::KEY_S));
        assert!(ShortcutSerialization::trigger_is_allowed(
            LogicalKey::ARROW_UP
        ));
    }

    #[test]
    fn an_item_is_enabled_exactly_when_the_application_gave_it_something_to_do() {
        // Computed rather than stored: there is no way to send a menu item
        // that looks pressable and is not, which is the right restriction for
        // a menu somebody else draws.
        let doing = PlatformMenuItem::new("Save").with_action().serialize(1);
        assert_eq!(
            value(&doing, keys::ENABLED),
            Some(&ChannelValue::Bool(true))
        );

        let inert = PlatformMenuItem::new("Save").serialize(1);
        assert_eq!(
            value(&inert, keys::ENABLED),
            Some(&ChannelValue::Bool(false))
        );
    }

    #[test]
    fn a_missing_tooltip_is_left_out_rather_than_sent_as_nothing() {
        // The platform's own default tooltip is not the same as no tooltip.
        let plain = PlatformMenuItem::new("Save").serialize(1);
        assert_eq!(value(&plain, keys::TOOLTIP), None);

        let described = PlatformMenuItem::new("Save")
            .with_tooltip("Save the document")
            .serialize(1);
        assert_eq!(
            value(&described, keys::TOOLTIP),
            Some(&ChannelValue::Text("Save the document".to_string()))
        );
    }

    #[test]
    fn a_group_gets_a_divider_on_each_side() {
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[PlatformMenuNode::Menu(PlatformMenu::new(
            "File",
            vec![
                item("New"),
                PlatformMenuNode::Group(PlatformMenuItemGroup::new(vec![
                    item("Open"),
                    item("Open Recent"),
                ])),
                item("Quit"),
            ],
        ))]);

        let sent = delegate.last_sent().expect("something was sent");
        let ChannelValue::List(children) = value(&sent[0], keys::CHILDREN).expect("children")
        else {
            panic!("children is a list");
        };
        let labels: Vec<String> = children
            .iter()
            .map(|entry| match value(entry, keys::LABEL) {
                Some(ChannelValue::Text(label)) => label.clone(),
                _ => "<divider>".to_string(),
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "New",
                "<divider>",
                "Open",
                "Open Recent",
                "<divider>",
                "Quit"
            ]
        );
    }

    #[test]
    fn a_menu_never_opens_or_closes_with_a_rule_across_it() {
        // Upstream filters the result rather than type-checking for groups,
        // because groups may be interleaved with non-groups and a non-group
        // may add a divider too.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[PlatformMenuNode::Menu(PlatformMenu::new(
            "File",
            vec![
                PlatformMenuNode::Group(PlatformMenuItemGroup::new(vec![item("Open")])),
                PlatformMenuNode::Group(PlatformMenuItemGroup::new(vec![item("Quit")])),
            ],
        ))]);

        let sent = delegate.last_sent().expect("sent");
        let ChannelValue::List(children) = value(&sent[0], keys::CHILDREN).expect("children")
        else {
            panic!("children is a list");
        };
        let labels: Vec<String> = children
            .iter()
            .map(|entry| match value(entry, keys::LABEL) {
                Some(ChannelValue::Text(label)) => label.clone(),
                _ => "<divider>".to_string(),
            })
            .collect();
        assert_eq!(
            labels,
            vec!["Open", "<divider>", "Quit"],
            "leading, doubled and trailing dividers all gone"
        );
    }

    #[test]
    fn tidying_leaves_a_menu_of_nothing_but_dividers_empty() {
        let divider = || {
            vec![
                (keys::ID, ChannelValue::Int(1)),
                (keys::IS_DIVIDER, ChannelValue::Bool(true)),
            ]
        };
        assert!(
            DefaultPlatformMenuDelegate::tidy_dividers(vec![divider(), divider(), divider()])
                .is_empty()
        );
    }

    #[test]
    fn every_serialised_thing_takes_a_fresh_id_including_both_of_a_groups_dividers() {
        // Both map back to the group, which is what lets the platform tell the
        // framework which one it touched even though a reader never touches a
        // divider.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[PlatformMenuNode::Group(PlatformMenuItemGroup::new(vec![
            item("Open"),
        ]))]);
        let ids: Vec<i32> = delegate.id_map().iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![1, 2, 3], "divider, item, divider");
        assert_eq!(delegate.item_for(2), Some("Open"));
        assert_eq!(delegate.item_for(99), None);
    }

    #[test]
    fn setting_the_menus_forgets_the_old_ids() {
        // They are only meaningful for the menus currently on screen, and a
        // stale id routed to a torn-down callback is the bug this prevents.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[item("First")]);
        assert_eq!(delegate.item_for(1), Some("First"));

        delegate.set_menus(&[item("Second")]);
        assert_eq!(delegate.item_for(1), None, "the old id is gone, not reused");
        assert_eq!(delegate.item_for(2), Some("Second"));
    }

    #[test]
    fn clearing_sends_an_empty_menu_bar_rather_than_saying_nothing() {
        // Upstream implements clearMenus as setMenus([]) -- the platform has
        // to be told, or it keeps drawing the last menus it was given.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[item("Save")]);
        delegate.clear_menus();
        assert_eq!(
            delegate.last_sent().map(|sent| sent.len()),
            Some(0),
            "something was sent, and it was empty"
        );
        assert!(delegate.id_map().is_empty());
    }

    #[test]
    fn one_menu_bar_per_delegate() {
        // Two sharing one would each overwrite the other's menus, and the
        // reader would see whichever built last with no way to tell why.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        assert!(delegate.lock(1));
        assert!(delegate.lock(1), "the same one again is fine");
        assert!(!delegate.lock(2), "a second is not");

        // Upstream's comment: unlocking one that is not locked is fine,
        // a different context doing the unlocking is not.
        assert!(!delegate.unlock(2));
        assert!(delegate.unlock(1));
        assert!(delegate.unlock(1), "and again is harmless");
        assert!(delegate.lock(2), "now it is free");
    }

    #[test]
    fn a_platform_provided_item_has_no_callback_because_the_platform_knows() {
        // What the framework contributes is where in the menu they go.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[PlatformMenuNode::Provided(PlatformProvidedMenuItem::new(
            PlatformProvidedMenuItemType::Quit,
        ))]);
        let sent = delegate.last_sent().expect("sent");
        assert!(value(&sent[0], keys::PLATFORM_PROVIDED_MENU).is_some());
        assert_eq!(value(&sent[0], keys::LABEL), None, "no label of ours");
        assert_eq!(
            value(&sent[0], keys::ENABLED),
            Some(&ChannelValue::Bool(true))
        );
    }

    #[test]
    fn only_macos_provides_any_of_them() {
        // Not a gap waiting to be filled: these are the items of the macOS
        // application menu, and there is no such menu elsewhere.
        for menu_type in [
            PlatformProvidedMenuItemType::About,
            PlatformProvidedMenuItemType::Quit,
            PlatformProvidedMenuItemType::ZoomWindow,
        ] {
            assert!(PlatformProvidedMenuItem::has_menu(true, menu_type));
            assert!(!PlatformProvidedMenuItem::has_menu(false, menu_type));
        }
    }

    #[test]
    fn an_empty_submenu_is_sent_disabled() {
        // A menu with nothing in it that still opens is a menu that wastes a
        // reader's click.
        let mut delegate = DefaultPlatformMenuDelegate::new();
        delegate.set_menus(&[
            PlatformMenuNode::Menu(PlatformMenu::new("Empty", Vec::new())),
            PlatformMenuNode::Menu(PlatformMenu::new("File", vec![item("Save")])),
        ]);
        let sent = delegate.last_sent().expect("sent");
        assert_eq!(
            value(&sent[0], keys::ENABLED),
            Some(&ChannelValue::Bool(false))
        );
        assert_eq!(
            value(&sent[1], keys::ENABLED),
            Some(&ChannelValue::Bool(true))
        );
    }

    #[test]
    fn a_menu_bar_holds_the_tree_it_was_given() {
        let bar = PlatformMenuBar::new(vec![PlatformMenuNode::Menu(PlatformMenu::new(
            "File",
            vec![item("Save")],
        ))]);
        assert_eq!(bar.menus.len(), 1);
        assert_eq!(PlatformMenuBar::default().menus.len(), 0);
    }
}

/// Upstream `MenuSerializableShortcut`: a shortcut activator that can also say
/// itself to the platform.
///
/// A mixin upstream, which is a trait here. It exists because the two things a
/// shortcut has to do are answered by different machinery: *matching a key
/// event* is the framework's job and belongs to the activator, while *appearing
/// next to a menu item in the platform's own menu bar* means handing the
/// platform a description it understands. Not every activator can do the second
/// -- an activator that matches on something the platform has no way to draw
/// has nothing to serialize -- which is why upstream makes it a separate mixin
/// rather than a member of `ShortcutActivator`.
pub trait MenuSerializableShortcut {
    /// Upstream's `serializeForMenu`.
    fn serialize_for_menu(&self) -> ShortcutSerialization;
}

impl MenuSerializableShortcut for ShortcutActivator {
    /// The two activators that can be drawn in a platform menu.
    ///
    /// A `LogicalKeySet` cannot: the platform's menus take one trigger key plus
    /// modifier flags, and a set of arbitrary keys held together has no such
    /// shape. Upstream's `LogicalKeySet` does not mix in
    /// `MenuSerializableShortcut` at all; here the closed enum has to answer
    /// something, so it answers with its lowest key as the trigger and says so.
    fn serialize_for_menu(&self) -> ShortcutSerialization {
        match self {
            ShortcutActivator::Character {
                character,
                control,
                single_modifier,
            } => ShortcutSerialization::Character {
                character: *character,
                alt: false,
                control: *control,
                meta: *single_modifier,
            },
            ShortcutActivator::Single {
                key,
                control,
                shift,
                alt,
                meta,
                // The menu serialization has no field for a lock demand: the
                // platform draws "Ctrl+C" and has nowhere to say "with num
                // lock off". Upstream's `serializeForMenu` drops it the same
                // way, by never reading it.
                ..
            } => ShortcutSerialization::Modifier {
                trigger: LogicalKey(*key),
                control: *control,
                shift: *shift,
                alt: *alt,
                meta: *meta,
            },
            ShortcutActivator::KeySet(set) => ShortcutSerialization::Modifier {
                trigger: LogicalKey(set.keys.first().copied().unwrap_or_default()),
                control: false,
                shift: false,
                alt: false,
                meta: false,
            },
        }
    }
}

#[cfg(test)]
mod menu_serializable_tests {
    use super::*;
    use crate::shortcuts::LogicalKeySet;

    #[test]
    fn a_single_activator_serializes_as_a_trigger_and_flags() {
        let save = ShortcutActivator::Single {
            key: LogicalKey::from_char('s').0,
            control: true,
            shift: false,
            alt: false,
            meta: true,
            num_lock: LockState::Ignored,
        };
        assert_eq!(
            save.serialize_for_menu(),
            ShortcutSerialization::Modifier {
                trigger: LogicalKey::from_char('s'),
                control: true,
                shift: false,
                alt: false,
                meta: true,
            }
        );
    }

    #[test]
    fn a_character_activator_carries_no_shift() {
        // A character already says whether shift was held -- see
        // [`ShortcutSerialization::Character`].
        let dollar = ShortcutActivator::Character {
            character: '$',
            control: false,
            single_modifier: true,
        };
        let ShortcutSerialization::Character {
            character, meta, ..
        } = dollar.serialize_for_menu()
        else {
            panic!("a character");
        };
        assert_eq!(character, '$');
        assert!(meta);
    }

    #[test]
    fn a_key_set_has_no_shape_the_platform_can_draw() {
        // The platform's menus take one trigger plus modifier flags, and a set
        // of arbitrary keys held together is not that. It answers with its
        // lowest key and no modifiers rather than inventing a combination.
        let set = ShortcutActivator::KeySet(LogicalKeySet::new(&[
            LogicalKey::from_char('b').0,
            LogicalKey::from_char('a').0,
        ]));
        assert_eq!(
            set.serialize_for_menu(),
            ShortcutSerialization::Modifier {
                trigger: LogicalKey::from_char('a'),
                control: false,
                shift: false,
                alt: false,
                meta: false,
            },
            "the lowest key, which is the sorted set's first"
        );
    }

    #[test]
    fn the_trait_is_what_a_menu_item_asks_through() {
        fn ask(shortcut: &dyn MenuSerializableShortcut) -> ShortcutSerialization {
            shortcut.serialize_for_menu()
        }
        let escape = ShortcutActivator::Single {
            key: LogicalKey::ESCAPE.0,
            control: false,
            shift: false,
            alt: false,
            meta: false,
            num_lock: LockState::Ignored,
        };
        assert!(matches!(
            ask(&escape),
            ShortcutSerialization::Modifier { .. }
        ));
    }
}
