//! A port of `material/action_buttons.dart`.
//!
//! The four buttons an app bar puts there without being asked: back, close,
//! open the drawer, open the end drawer. Eight classes, because each is a
//! button and an icon, and the icon is separable so an application can replace
//! all of them at once through `ActionIconTheme`.
//!
//! They are worth reading together for one inversion and one distinction.

use crate::scroll_plumbing::ScrollPlatform;

/// Which of the four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionButtonKind {
    Back,
    Close,
    Drawer,
    EndDrawer,
}

/// What a button does when nobody gave it a callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAction {
    /// Upstream's `Navigator.maybePop` -- **maybe**, so a route that refuses
    /// (a `PopScope` with `can_pop` false) is honoured rather than overruled.
    MaybePop,
    OpenDrawer,
    OpenEndDrawer,
}

/// Upstream's `_ActionButton`, the base of all four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionButton {
    pub kind: ActionButtonKind,
    /// Whether the caller supplied one.
    pub has_on_pressed: bool,
}

impl ActionButton {
    pub fn new(kind: ActionButtonKind) -> ActionButton {
        ActionButton {
            kind,
            has_on_pressed: false,
        }
    }

    pub fn with_on_pressed(mut self) -> Self {
        self.has_on_pressed = true;
        self
    }

    /// The inversion worth writing down: **a null `onPressed` does not disable
    /// this button.**
    ///
    /// On an ordinary `IconButton` a null callback greys it out. Here the base
    /// class wraps the handler so a null one falls through to the obvious
    /// thing. A back button with no callback is still a back button; the
    /// caller supplying nothing is not saying "do nothing", they are saying
    /// "do what a back button does".
    pub fn is_enabled(&self) -> bool {
        true
    }

    /// What pressing it runs.
    pub fn on_pressed(&self) -> Option<DefaultAction> {
        if self.has_on_pressed {
            return None;
        }
        Some(self.default_action())
    }

    pub fn default_action(&self) -> DefaultAction {
        match self.kind {
            // Back and Close do exactly the same thing. They differ in icon and
            // in tooltip, which is to say they differ in what the reader is
            // being told: "return to where you were" against "dismiss this".
            // The same action, given two meanings.
            ActionButtonKind::Back | ActionButtonKind::Close => DefaultAction::MaybePop,
            ActionButtonKind::Drawer => DefaultAction::OpenDrawer,
            ActionButtonKind::EndDrawer => DefaultAction::OpenEndDrawer,
        }
    }

    /// The localisation key for the tooltip. Every one of the four has its own,
    /// because the tooltip is the only thing telling a mouse user which of two
    /// identical actions this is.
    pub fn tooltip_key(&self) -> &'static str {
        match self.kind {
            ActionButtonKind::Back => "backButtonTooltip",
            ActionButtonKind::Close => "closeButtonTooltip",
            ActionButtonKind::Drawer => "openAppDrawerTooltip",
            ActionButtonKind::EndDrawer => "openAppDrawerTooltip",
        }
    }

    /// Upstream tags these with a `StandardComponentType` key, so a test or a
    /// tool can find "the back button" without knowing what the app called it.
    pub fn has_standard_key(&self) -> bool {
        true
    }
}

/// Upstream's `_ActionIcon`, and the four icon widgets over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionIcon {
    pub kind: ActionButtonKind,
}

impl ActionIcon {
    pub fn new(kind: ActionButtonKind) -> ActionIcon {
        ActionIcon { kind }
    }

    /// The icon drawn, given the **theme's** platform and whether this is the
    /// web.
    ///
    /// Only the back button varies, and it varies by `Theme.of(context)
    /// .platform` rather than by the real one: the icon is **appearance**, so
    /// an application themed as iOS gets the iOS chevron wherever it runs.
    ///
    /// On the web it is always the plain arrow, whatever the platform says. A
    /// web page on a Mac is still a web page, and the browser has a back
    /// affordance of its own to not look like.
    pub fn icon(&self, platform: ScrollPlatform, is_web: bool) -> &'static str {
        match self.kind {
            ActionButtonKind::Back => {
                if is_web {
                    return "arrow_back";
                }
                match platform {
                    ScrollPlatform::IOS | ScrollPlatform::MacOS => "arrow_back_ios_new_rounded",
                    _ => "arrow_back",
                }
            }
            ActionButtonKind::Close => "close",
            ActionButtonKind::Drawer => "menu",
            ActionButtonKind::EndDrawer => "menu",
        }
    }

    /// The semantics label, given the **real** platform.
    ///
    /// This is the distinction the file is worth reading for. The icon follows
    /// the theme; the label follows `defaultTargetPlatform`, and upstream says
    /// why in a comment: *"This can't use the platform from Theme because it is
    /// the Android OS that expects the duplicated tooltip and label."*
    ///
    /// **A theme override changes how an application looks. It must not change
    /// what the operating system's accessibility service is told.** An app
    /// dressed as iOS running on Android is still being read by TalkBack.
    pub fn semantics_label(&self, real_platform: ScrollPlatform) -> Option<&'static str> {
        match real_platform {
            ScrollPlatform::Android => Some(match self.kind {
                ActionButtonKind::Back => "backButtonTooltip",
                ActionButtonKind::Close => "closeButtonTooltip",
                ActionButtonKind::Drawer | ActionButtonKind::EndDrawer => "openAppDrawerTooltip",
            }),
            // Everywhere else the tooltip alone is enough; repeating it would
            // have the reader hear the same words twice.
            _ => None,
        }
    }

    /// Whether an `ActionIconTheme` builder replaces this icon. All four can be
    /// replaced at once, which is the reason the icons are separate widgets at
    /// all.
    pub fn uses_theme_builder(theme_has_builder: bool) -> bool {
        theme_has_builder
    }
}

// -- The eight, which is four twice ------------------------------------------
//
// Upstream's eight classes differ only in which of the four they are: each
// button is `_ActionButton` with one kind, each icon is `_ActionIcon` with the
// same. They are separate classes because a caller writes `BackButton()` and
// not `ActionButton(kind: back)` -- the name at the call site is the point.

/// Returns to the previous route. The only one of the four whose icon changes by platform.
///
/// Upstream's class differs from its three siblings only in which of the four
/// it is; it is a separate class because a caller writes `BackButton()` and
/// not `ActionButton(kind: ...)`. **The name at the call site is the point.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackButton(pub ActionButton);

impl BackButton {
    pub fn new() -> BackButton {
        BackButton(ActionButton::new(ActionButtonKind::Back))
    }

    /// With a callback of the caller's own, which replaces the default rather
    /// than disabling the button.
    pub fn with_on_pressed() -> BackButton {
        BackButton(ActionButton::new(ActionButtonKind::Back).with_on_pressed())
    }
}

impl Default for BackButton {
    fn default() -> Self {
        BackButton::new()
    }
}

/// The icon [`BackButton`] draws, separable so `ActionIconTheme` can replace it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackButtonIcon;

impl BackButtonIcon {
    pub fn icon(platform: ScrollPlatform, is_web: bool) -> &'static str {
        ActionIcon::new(ActionButtonKind::Back).icon(platform, is_web)
    }

    pub fn semantics_label(real_platform: ScrollPlatform) -> Option<&'static str> {
        ActionIcon::new(ActionButtonKind::Back).semantics_label(real_platform)
    }
}

/// Pops the route, exactly as [`BackButton`] does, but says "dismiss this" rather than "go back".
///
/// Upstream's class differs from its three siblings only in which of the four
/// it is; it is a separate class because a caller writes `CloseButton()` and
/// not `ActionButton(kind: ...)`. **The name at the call site is the point.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseButton(pub ActionButton);

impl CloseButton {
    pub fn new() -> CloseButton {
        CloseButton(ActionButton::new(ActionButtonKind::Close))
    }

    /// With a callback of the caller's own, which replaces the default rather
    /// than disabling the button.
    pub fn with_on_pressed() -> CloseButton {
        CloseButton(ActionButton::new(ActionButtonKind::Close).with_on_pressed())
    }
}

impl Default for CloseButton {
    fn default() -> Self {
        CloseButton::new()
    }
}

/// The icon [`CloseButton`] draws, separable so `ActionIconTheme` can replace it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloseButtonIcon;

impl CloseButtonIcon {
    pub fn icon(platform: ScrollPlatform, is_web: bool) -> &'static str {
        ActionIcon::new(ActionButtonKind::Close).icon(platform, is_web)
    }

    pub fn semantics_label(real_platform: ScrollPlatform) -> Option<&'static str> {
        ActionIcon::new(ActionButtonKind::Close).semantics_label(real_platform)
    }
}

/// Opens the scaffold's drawer.
///
/// Upstream's class differs from its three siblings only in which of the four
/// it is; it is a separate class because a caller writes `DrawerButton()` and
/// not `ActionButton(kind: ...)`. **The name at the call site is the point.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawerButton(pub ActionButton);

impl DrawerButton {
    pub fn new() -> DrawerButton {
        DrawerButton(ActionButton::new(ActionButtonKind::Drawer))
    }

    /// With a callback of the caller's own, which replaces the default rather
    /// than disabling the button.
    pub fn with_on_pressed() -> DrawerButton {
        DrawerButton(ActionButton::new(ActionButtonKind::Drawer).with_on_pressed())
    }
}

impl Default for DrawerButton {
    fn default() -> Self {
        DrawerButton::new()
    }
}

/// The icon [`DrawerButton`] draws, separable so `ActionIconTheme` can replace it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawerButtonIcon;

impl DrawerButtonIcon {
    pub fn icon(platform: ScrollPlatform, is_web: bool) -> &'static str {
        ActionIcon::new(ActionButtonKind::Drawer).icon(platform, is_web)
    }

    pub fn semantics_label(real_platform: ScrollPlatform) -> Option<&'static str> {
        ActionIcon::new(ActionButtonKind::Drawer).semantics_label(real_platform)
    }
}

/// Opens the scaffold's end drawer, the one on the other side.
///
/// Upstream's class differs from its three siblings only in which of the four
/// it is; it is a separate class because a caller writes `EndDrawerButton()` and
/// not `ActionButton(kind: ...)`. **The name at the call site is the point.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndDrawerButton(pub ActionButton);

impl EndDrawerButton {
    pub fn new() -> EndDrawerButton {
        EndDrawerButton(ActionButton::new(ActionButtonKind::EndDrawer))
    }

    /// With a callback of the caller's own, which replaces the default rather
    /// than disabling the button.
    pub fn with_on_pressed() -> EndDrawerButton {
        EndDrawerButton(ActionButton::new(ActionButtonKind::EndDrawer).with_on_pressed())
    }
}

impl Default for EndDrawerButton {
    fn default() -> Self {
        EndDrawerButton::new()
    }
}

/// The icon [`EndDrawerButton`] draws, separable so `ActionIconTheme` can replace it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EndDrawerButtonIcon;

impl EndDrawerButtonIcon {
    pub fn icon(platform: ScrollPlatform, is_web: bool) -> &'static str {
        ActionIcon::new(ActionButtonKind::EndDrawer).icon(platform, is_web)
    }

    pub fn semantics_label(real_platform: ScrollPlatform) -> Option<&'static str> {
        ActionIcon::new(ActionButtonKind::EndDrawer).semantics_label(real_platform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_back_button_with_no_callback_is_still_a_back_button() {
        // On an ordinary IconButton a null onPressed greys it out. Here the
        // caller supplying nothing is not saying "do nothing".
        let plain = BackButton::new();
        assert!(plain.0.is_enabled());
        assert_eq!(plain.0.on_pressed(), Some(DefaultAction::MaybePop));

        let custom = BackButton::with_on_pressed();
        assert!(custom.0.is_enabled());
        assert_eq!(
            custom.0.on_pressed(),
            None,
            "the caller's callback replaces the default rather than adding to it"
        );
    }

    #[test]
    fn back_and_close_are_the_same_action_given_two_meanings() {
        assert_eq!(
            BackButton::new().0.default_action(),
            CloseButton::new().0.default_action()
        );
        assert_ne!(
            BackButton::new().0.tooltip_key(),
            CloseButton::new().0.tooltip_key(),
            "and the tooltip is the only thing telling them apart"
        );
    }

    #[test]
    fn popping_is_a_request_rather_than_an_order() {
        // maybePop, so a PopScope that refuses is honoured rather than
        // overruled.
        assert_eq!(
            BackButton::new().0.default_action(),
            DefaultAction::MaybePop
        );
    }

    #[test]
    fn the_two_drawer_buttons_open_opposite_sides() {
        assert_eq!(
            DrawerButton::new().0.default_action(),
            DefaultAction::OpenDrawer
        );
        assert_eq!(
            EndDrawerButton::new().0.default_action(),
            DefaultAction::OpenEndDrawer
        );
    }

    // -- Icons follow the theme -------------------------------------------------

    #[test]
    fn only_the_back_button_changes_its_icon_by_platform() {
        assert_eq!(
            BackButtonIcon::icon(ScrollPlatform::IOS, false),
            "arrow_back_ios_new_rounded"
        );
        assert_eq!(
            BackButtonIcon::icon(ScrollPlatform::Android, false),
            "arrow_back"
        );

        for platform in [ScrollPlatform::IOS, ScrollPlatform::Android] {
            assert_eq!(CloseButtonIcon::icon(platform, false), "close");
            assert_eq!(DrawerButtonIcon::icon(platform, false), "menu");
            assert_eq!(EndDrawerButtonIcon::icon(platform, false), "menu");
        }
    }

    #[test]
    fn on_the_web_it_is_always_the_plain_arrow() {
        // A web page on a Mac is still a web page, and the browser has a back
        // affordance of its own to not look like.
        assert_eq!(
            BackButtonIcon::icon(ScrollPlatform::MacOS, true),
            "arrow_back"
        );
        assert_eq!(
            BackButtonIcon::icon(ScrollPlatform::IOS, true),
            "arrow_back"
        );
    }

    // -- Labels follow the operating system --------------------------------------

    #[test]
    fn a_theme_override_must_not_change_what_the_screen_reader_is_told() {
        // The icon follows Theme.of(context).platform; the label follows
        // defaultTargetPlatform, because it is the Android OS that expects the
        // duplicated tooltip and label. An app dressed as iOS running on
        // Android is still being read by TalkBack.
        assert_eq!(
            BackButtonIcon::semantics_label(ScrollPlatform::Android),
            Some("backButtonTooltip")
        );
        assert_eq!(
            BackButtonIcon::semantics_label(ScrollPlatform::IOS),
            None,
            "elsewhere the tooltip alone is enough"
        );
    }

    #[test]
    fn the_icon_and_the_label_can_disagree_and_that_is_the_point() {
        // An iOS-themed app on Android: the iOS chevron, and the Android label.
        let icon = BackButtonIcon::icon(ScrollPlatform::IOS, false);
        let label = BackButtonIcon::semantics_label(ScrollPlatform::Android);
        assert_eq!(icon, "arrow_back_ios_new_rounded");
        assert_eq!(label, Some("backButtonTooltip"));
    }

    #[test]
    fn every_icon_has_a_label_where_the_platform_wants_one() {
        assert!(CloseButtonIcon::semantics_label(ScrollPlatform::Android).is_some());
        assert!(DrawerButtonIcon::semantics_label(ScrollPlatform::Android).is_some());
        assert!(EndDrawerButtonIcon::semantics_label(ScrollPlatform::Android).is_some());
    }

    #[test]
    fn a_theme_builder_replaces_all_four_icons_at_once() {
        // Which is the reason the icons are separate widgets at all.
        assert!(ActionIcon::uses_theme_builder(true));
        assert!(!ActionIcon::uses_theme_builder(false));
    }

    #[test]
    fn each_button_carries_a_key_a_test_can_find_it_by() {
        assert!(BackButton::new().0.has_standard_key());
    }
}
