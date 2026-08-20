//! Ports of `material/button.dart`, `material/material_button.dart`,
//! `material/icon_button.dart` and `material/floating_action_button.dart`.
//!
//! Things you press. What they share is a set of states that can all be true at
//! once, and the whole of `_effectiveElevation` is deciding which of them the
//! button should look like.

/// The minimum a touch target may be, upstream's `kMinInteractiveDimension`.
pub const MIN_INTERACTIVE_DIMENSION: f32 = 48.0;

/// Which of a button's states is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonStates {
    pub disabled: bool,
    pub pressed: bool,
    pub hovered: bool,
    pub focused: bool,
}

/// The five elevations a button carries, one per state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonElevations {
    pub resting: f32,
    pub focus: f32,
    pub hover: f32,
    pub highlight: f32,
    pub disabled: f32,
}

impl ButtonElevations {
    /// Upstream's defaults for `RawMaterialButton`.
    pub fn new() -> ButtonElevations {
        ButtonElevations {
            resting: 2.0,
            focus: 4.0,
            hover: 4.0,
            highlight: 8.0,
            disabled: 0.0,
        }
    }

    /// Upstream asserts each is non-negative. A negative elevation would be a
    /// shadow cast upwards.
    pub fn is_valid(&self) -> bool {
        self.resting >= 0.0
            && self.focus >= 0.0
            && self.hover >= 0.0
            && self.highlight >= 0.0
            && self.disabled >= 0.0
    }
}

impl Default for ButtonElevations {
    fn default() -> Self {
        ButtonElevations::new()
    }
}

/// Upstream `RawMaterialButton`: `Semantics`, `Material` and `InkWell`, and
/// nothing else. Everything Material calls a button is built on this.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawMaterialButton {
    pub elevations: ButtonElevations,
    pub has_on_pressed: bool,
}

impl RawMaterialButton {
    pub fn new() -> RawMaterialButton {
        RawMaterialButton {
            elevations: ButtonElevations::new(),
            has_on_pressed: true,
        }
    }

    /// Upstream `_effectiveElevation`, which carries a warning of its own:
    ///
    /// > These conditionals are in order of precedence, so be careful about
    /// > reorganizing them.
    ///
    /// **The order of the if-chain is the specification.** These states can all
    /// be true at once -- a disabled button can be hovered, and with a mouse a
    /// button cannot be pressed without also being hovered -- so something has
    /// to win, and the order says which.
    ///
    /// Disabled first, because a disabled button that is hovered is still
    /// disabled. Pressed above hovered, because otherwise the pressed elevation
    /// would be unreachable for anybody using a mouse. And focus last of the
    /// four, because focus is the state a reader is least likely to be looking
    /// at while doing something else.
    ///
    /// Compare the toggleable's three nested lerps in tick 61, where focus won
    /// instead: both are precedence written as structure, and each picked the
    /// order its own control needed.
    pub fn effective_elevation(&self, states: ButtonStates) -> f32 {
        if states.disabled {
            return self.elevations.disabled;
        }
        if states.pressed {
            return self.elevations.highlight;
        }
        if states.hovered {
            return self.elevations.hover;
        }
        if states.focused {
            return self.elevations.focus;
        }
        self.elevations.resting
    }

    /// Upstream reads disabled off a null `onPressed`.
    pub fn states(&self, pressed: bool, hovered: bool, focused: bool) -> ButtonStates {
        ButtonStates {
            disabled: !self.has_on_pressed,
            pressed,
            hovered,
            focused,
        }
    }
}

impl Default for RawMaterialButton {
    fn default() -> Self {
        RawMaterialButton::new()
    }
}

/// Upstream `MaterialButton`.
///
/// The Material 2 button, and its own documentation sends you elsewhere:
/// *"To create a custom Material button consider using `TextButton`,
/// `ElevatedButton`, or `OutlinedButton`."* It is kept because applications
/// were written against it, not because anybody should reach for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialButton {
    pub elevations: ButtonElevations,
    pub has_on_pressed: bool,
}

impl MaterialButton {
    pub fn new() -> MaterialButton {
        MaterialButton {
            elevations: ButtonElevations::new(),
            has_on_pressed: true,
        }
    }

    /// Whether upstream's own documentation points somewhere else for new work.
    pub fn is_superseded() -> bool {
        true
    }
}

impl Default for MaterialButton {
    fn default() -> Self {
        MaterialButton::new()
    }
}

/// Upstream `IconButton`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconButton {
    pub icon_size: f32,
    /// `None` uses the default. Upstream asserts it is positive when given: a
    /// radius of zero would be an ink reaction with no ink, which is not the
    /// same as asking for no reaction.
    pub splash_radius: Option<f32>,
    pub has_on_pressed: bool,
}

impl IconButton {
    pub const DEFAULT_ICON_SIZE: f32 = 24.0;

    pub fn new() -> IconButton {
        IconButton {
            icon_size: IconButton::DEFAULT_ICON_SIZE,
            splash_radius: None,
            has_on_pressed: true,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.splash_radius.is_none_or(|radius| radius > 0.0)
    }

    /// Upstream: *"The hit region of an icon button will, if possible, be at
    /// least `kMinInteractiveDimension` pixels in size, regardless of the
    /// actual `iconSize`"*.
    ///
    /// **The drawn thing and the pressable thing are two different sizes**, and
    /// the button grows the second without growing the first. A sixteen-pixel
    /// icon is still a forty-eight-pixel target, and `alignment` decides where
    /// in that target the icon sits.
    ///
    /// The same idea as the scrollbar's touch expansion in tick 59: a fingertip
    /// is not a cursor, and what you can hit is not what you can see.
    pub fn hit_region_size(&self) -> f32 {
        self.icon_size.max(MIN_INTERACTIVE_DIMENSION)
    }

    /// Whether the icon fills its target or floats inside it.
    pub fn icon_fills_hit_region(&self) -> bool {
        self.icon_size >= MIN_INTERACTIVE_DIMENSION
    }
}

impl Default for IconButton {
    fn default() -> Self {
        IconButton::new()
    }
}

/// Upstream `FloatingActionButton`.
///
/// The elevations are the same five, but every one is **nullable** here where
/// `RawMaterialButton`'s are not -- a FAB defers to the theme by default and
/// only overrides what it was told to. The asserts change shape to match:
/// `elevation == null || elevation >= 0.0`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatingActionButton {
    pub elevation: Option<f32>,
    pub focus_elevation: Option<f32>,
    pub hover_elevation: Option<f32>,
    pub highlight_elevation: Option<f32>,
    pub disabled_elevation: Option<f32>,
    /// Whether this is the extended form, which carries a label beside the
    /// icon.
    pub is_extended: bool,
}

impl FloatingActionButton {
    pub fn new() -> FloatingActionButton {
        FloatingActionButton::default()
    }

    pub fn extended() -> FloatingActionButton {
        FloatingActionButton {
            is_extended: true,
            ..FloatingActionButton::default()
        }
    }

    /// Upstream's asserts, in their nullable form.
    pub fn is_valid(&self) -> bool {
        [
            self.elevation,
            self.focus_elevation,
            self.hover_elevation,
            self.highlight_elevation,
            self.disabled_elevation,
        ]
        .iter()
        .all(|value| value.is_none_or(|elevation| elevation >= 0.0))
    }

    /// What the button uses, given the theme's own five.
    pub fn resolve(&self, theme: ButtonElevations) -> ButtonElevations {
        ButtonElevations {
            resting: self.elevation.unwrap_or(theme.resting),
            focus: self.focus_elevation.unwrap_or(theme.focus),
            hover: self.hover_elevation.unwrap_or(theme.hover),
            highlight: self.highlight_elevation.unwrap_or(theme.highlight),
            disabled: self.disabled_elevation.unwrap_or(theme.disabled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Five distinct values, so that every step of the chain is observable.
    ///
    /// The real defaults will not do for this: upstream's `focusElevation` and
    /// `hoverElevation` are **both 4.0**, so a test written against the defaults
    /// could not tell whether focus or hover won -- it would compare 4.0 to 4.0
    /// and pass whichever way the chain ran.
    fn distinct() -> RawMaterialButton {
        RawMaterialButton {
            elevations: ButtonElevations {
                resting: 1.0,
                focus: 2.0,
                hover: 3.0,
                highlight: 4.0,
                disabled: 5.0,
            },
            has_on_pressed: true,
        }
    }

    // -- The order of the chain is the specification -----------------------------

    #[test]
    fn a_disabled_button_that_is_hovered_is_still_disabled() {
        let button = distinct();
        let everything = ButtonStates {
            disabled: true,
            pressed: true,
            hovered: true,
            focused: true,
        };
        assert_eq!(button.effective_elevation(everything), 5.0);
    }

    #[test]
    fn pressing_beats_hovering_which_is_the_only_reason_the_pressed_elevation_is_reachable() {
        // With a mouse you cannot press a button without also hovering it, so
        // these two arrive together every single time. If hover were tested
        // first, the highlight elevation would never be seen by a mouse user at
        // all.
        let button = distinct();
        let mouse_press = ButtonStates {
            disabled: false,
            pressed: true,
            hovered: true,
            focused: false,
        };
        assert_eq!(button.effective_elevation(mouse_press), 4.0);
        assert_ne!(
            button.elevations.hover, button.elevations.highlight,
            "and the two differ, so the ordering is observable"
        );
    }

    #[test]
    fn hovering_beats_focus() {
        let button = distinct();
        let states = ButtonStates {
            disabled: false,
            pressed: false,
            hovered: true,
            focused: true,
        };
        assert_eq!(button.effective_elevation(states), 3.0);
    }

    #[test]
    fn each_step_of_the_chain_is_reachable_on_its_own() {
        let button = distinct();
        let only = |set: fn(&mut ButtonStates)| {
            let mut states = ButtonStates::default();
            set(&mut states);
            button.effective_elevation(states)
        };
        assert_eq!(only(|s| s.disabled = true), 5.0);
        assert_eq!(only(|s| s.pressed = true), 4.0);
        assert_eq!(only(|s| s.hovered = true), 3.0);
        assert_eq!(only(|s| s.focused = true), 2.0);
        assert_eq!(button.effective_elevation(ButtonStates::default()), 1.0);
    }

    #[test]
    fn the_defaults_cannot_distinguish_focus_from_hover() {
        // Which is worth stating rather than working around: upstream gives both
        // 4.0, so on a stock button the difference between the two states is not
        // visible at all. The precedence still matters, because a theme is free
        // to make them differ.
        let defaults = ButtonElevations::new();
        assert_eq!(defaults.focus, defaults.hover);
        assert_eq!(
            (defaults.resting, defaults.highlight, defaults.disabled),
            (2.0, 8.0, 0.0)
        );
    }

    // -- What the constructors refuse --------------------------------------------

    #[test]
    fn an_elevation_may_not_be_negative() {
        let mut elevations = ButtonElevations::new();
        assert!(elevations.is_valid());
        elevations.highlight = -1.0;
        assert!(!elevations.is_valid(), "a shadow cast upwards");
    }

    #[test]
    fn a_button_with_nothing_to_do_is_disabled() {
        let mut button = distinct();
        button.has_on_pressed = false;
        assert!(button.states(false, false, false).disabled);
        assert!(!distinct().states(false, false, false).disabled);
    }

    // -- The hit region is not the icon ------------------------------------------

    #[test]
    fn a_small_icon_still_gets_a_full_size_target() {
        let mut button = IconButton::new();
        button.icon_size = 16.0;
        assert_eq!(button.hit_region_size(), MIN_INTERACTIVE_DIMENSION);
        assert!(!button.icon_fills_hit_region(), "the icon floats inside it");
    }

    #[test]
    fn the_default_icon_is_smaller_than_the_target_it_sits_in() {
        let button = IconButton::new();
        assert_eq!(button.icon_size, 24.0);
        assert_eq!(button.hit_region_size(), 48.0);
        assert_eq!(
            button.hit_region_size(),
            button.icon_size * 2.0,
            "the ordinary case is a target twice the width of what is drawn in it"
        );
    }

    #[test]
    fn a_large_icon_grows_the_target_rather_than_being_clipped_to_it() {
        // The minimum is a floor, not a size. Upstream's own doc uses 72.
        let mut button = IconButton::new();
        button.icon_size = 72.0;
        assert_eq!(button.hit_region_size(), 72.0);
        assert!(button.icon_fills_hit_region());
    }

    #[test]
    fn a_splash_with_no_radius_is_refused_but_no_splash_radius_at_all_is_fine() {
        let mut button = IconButton::new();
        assert!(button.is_valid(), "None means the default, not none");
        button.splash_radius = Some(0.0);
        assert!(!button.is_valid());
        button.splash_radius = Some(20.0);
        assert!(button.is_valid());
    }

    // -- The FAB defers ------------------------------------------------------------

    #[test]
    fn a_floating_action_button_overrides_only_what_it_was_given() {
        let theme = distinct().elevations;
        let mut fab = FloatingActionButton::new();
        assert_eq!(
            fab.resolve(theme),
            theme,
            "and by default overrides nothing"
        );

        fab.highlight_elevation = Some(12.0);
        let resolved = fab.resolve(theme);
        assert_eq!(resolved.highlight, 12.0);
        assert_eq!(resolved.resting, theme.resting);
        assert_eq!(resolved.hover, theme.hover);
    }

    #[test]
    fn the_fabs_assert_is_the_same_one_in_nullable_clothing() {
        let mut fab = FloatingActionButton::new();
        assert!(fab.is_valid(), "unset is not negative");
        fab.focus_elevation = Some(-0.5);
        assert!(!fab.is_valid());
        fab.focus_elevation = Some(0.0);
        assert!(fab.is_valid(), "zero is allowed; the assert is >= 0");
    }

    #[test]
    fn an_extended_fab_is_the_same_button_carrying_a_label() {
        assert!(FloatingActionButton::extended().is_extended);
        assert!(!FloatingActionButton::new().is_extended);
    }

    // -- The one upstream tells you not to use ------------------------------------

    #[test]
    fn the_material_button_documents_its_own_replacement() {
        // Kept because applications were written against it.
        assert!(MaterialButton::is_superseded());
        assert_eq!(
            MaterialButton::new().elevations,
            RawMaterialButton::new().elevations,
            "it is the raw button's five, unchanged"
        );
    }
}
