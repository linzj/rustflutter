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

    /// This button's appearance, with the theme and the defaults folded in.
    ///
    /// The widget's own elevation for the state in hand comes **first**, ahead
    /// of the theme's -- which is why this is not simply
    /// [`ResolvedFloatingActionButton::of`](crate::component_themes::ResolvedFloatingActionButton::of).
    /// The resolver knows how to pick one of the five by state; the widget
    /// knows which five it was given.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        states: crate::widget_state::WidgetStates,
    ) -> crate::component_themes::ResolvedFloatingActionButton {
        use crate::widget_state::WidgetState;
        let mut resolved =
            crate::component_themes::ResolvedFloatingActionButton::of(context, states);
        // The same order the resolver uses, over the widget's own fields.
        let mine = if states.contains(WidgetState::Disabled) {
            self.disabled_elevation.or(self.elevation)
        } else if states.contains(WidgetState::Pressed) {
            self.highlight_elevation
        } else if states.contains(WidgetState::Hovered) {
            self.hover_elevation.or(self.elevation)
        } else if states.contains(WidgetState::Focused) {
            self.focus_elevation.or(self.elevation)
        } else {
            self.elevation
        };
        if let Some(mine) = mine {
            resolved.elevation = mine;
        }
        resolved
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

#[cfg(test)]
mod fab_theme_tests {
    use super::*;
    use crate::component_themes::{
        FloatingActionButtonTheme, FloatingActionButtonThemeData, ResolvedFloatingActionButton,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, provide};
    use crate::widget_state::{WidgetState, WidgetStates};

    struct Reader {
        button: FloatingActionButton,
        states: WidgetStates,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedFloatingActionButton>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.button.resolved(context, self.states));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(
        button: FloatingActionButton,
        data: FloatingActionButtonThemeData,
        states: WidgetStates,
    ) -> ResolvedFloatingActionButton {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            FloatingActionButtonTheme::new(
                data,
                component(Reader {
                    button,
                    states,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn none() -> WidgetStates {
        WidgetStates::NONE
    }

    #[test]
    fn the_state_picks_one_of_the_five_rather_than_blending_them() {
        // An elevation is a height, and a button that is both hovered and
        // focused is at one height, not at the average of two.
        let mut data = FloatingActionButtonThemeData::new();
        data.elevation = Some(1.0);
        data.hover_elevation = Some(2.0);
        data.focus_elevation = Some(3.0);
        data.highlight_elevation = Some(4.0);
        data.disabled_elevation = Some(5.0);
        let fab = || FloatingActionButton::new();

        assert_eq!(resolve(fab(), data.clone(), none()).elevation, 1.0);
        assert_eq!(
            resolve(fab(), data.clone(), none().with(WidgetState::Hovered)).elevation,
            2.0
        );
        assert_eq!(
            resolve(fab(), data.clone(), none().with(WidgetState::Focused)).elevation,
            3.0
        );
        assert_eq!(
            resolve(fab(), data.clone(), none().with(WidgetState::Pressed)).elevation,
            4.0
        );
        assert_eq!(
            resolve(fab(), data, none().with(WidgetState::Disabled)).elevation,
            5.0
        );
    }

    #[test]
    fn disabled_beats_held_beats_hovered_beats_focused() {
        // A button can be several of these at once and there is only one
        // height; the order is upstream's.
        let mut data = FloatingActionButtonThemeData::new();
        data.hover_elevation = Some(2.0);
        data.focus_elevation = Some(3.0);
        data.highlight_elevation = Some(4.0);
        data.disabled_elevation = Some(5.0);

        let every = none()
            .with(WidgetState::Hovered)
            .with(WidgetState::Focused)
            .with(WidgetState::Pressed)
            .with(WidgetState::Disabled);
        assert_eq!(
            resolve(FloatingActionButton::new(), data.clone(), every).elevation,
            5.0
        );

        let busy = none()
            .with(WidgetState::Hovered)
            .with(WidgetState::Focused)
            .with(WidgetState::Pressed);
        assert_eq!(
            resolve(FloatingActionButton::new(), data.clone(), busy).elevation,
            4.0
        );

        let both = none().with(WidgetState::Hovered).with(WidgetState::Focused);
        assert_eq!(
            resolve(FloatingActionButton::new(), data, both).elevation,
            2.0
        );
    }

    #[test]
    fn the_buttons_own_elevation_beats_the_themes() {
        let mut data = FloatingActionButtonThemeData::new();
        data.elevation = Some(1.0);
        data.hover_elevation = Some(2.0);

        let mut mine = FloatingActionButton::new();
        mine.elevation = Some(20.0);
        assert_eq!(resolve(mine, data.clone(), none()).elevation, 20.0);

        let mut hovering = FloatingActionButton::new();
        hovering.hover_elevation = Some(30.0);
        assert_eq!(
            resolve(hovering, data, none().with(WidgetState::Hovered)).elevation,
            30.0
        );
    }

    #[test]
    fn the_buttons_own_five_are_picked_in_the_same_order_as_the_themes() {
        // A button that is hovered *and* focused takes the hover height, on
        // its own fields as on the theme's. Setting one of them alone cannot
        // see the order.
        let mut mine = FloatingActionButton::new();
        mine.hover_elevation = Some(30.0);
        mine.focus_elevation = Some(40.0);
        let both = none().with(WidgetState::Hovered).with(WidgetState::Focused);
        assert_eq!(
            resolve(mine, FloatingActionButtonThemeData::new(), both).elevation,
            30.0,
            "hover before focus"
        );

        let mut disabled_too = FloatingActionButton::new();
        disabled_too.hover_elevation = Some(30.0);
        disabled_too.disabled_elevation = Some(50.0);
        assert_eq!(
            resolve(
                disabled_too,
                FloatingActionButtonThemeData::new(),
                both.with(WidgetState::Disabled)
            )
            .elevation,
            50.0,
            "and disabled before everything"
        );
    }

    #[test]
    fn a_buttons_resting_elevation_stands_in_for_the_states_it_did_not_set() {
        // Upstream's `hoverElevation ?? elevation`: a caller who raised the
        // button meant it raised, hovered or not.
        let mut mine = FloatingActionButton::new();
        mine.elevation = Some(20.0);
        assert_eq!(
            resolve(
                mine,
                FloatingActionButtonThemeData::new(),
                none().with(WidgetState::Hovered)
            )
            .elevation,
            20.0
        );
    }

    #[test]
    fn a_held_button_does_not_fall_back_to_the_resting_height() {
        // Upstream's highlight branch has no `?? elevation`: being pressed is
        // the one state whose height is *lower relative to the finger*, and
        // borrowing the resting value would flatten the press entirely.
        let mut mine = FloatingActionButton::new();
        mine.elevation = Some(20.0);
        assert_eq!(
            resolve(
                mine,
                FloatingActionButtonThemeData::new(),
                none().with(WidgetState::Pressed)
            )
            .elevation,
            ResolvedFloatingActionButton::HIGHLIGHT_ELEVATION,
            "the highlight default, not the button's own resting height"
        );
    }

    #[test]
    fn the_defaults_are_upstreams() {
        let resolved = resolve(
            FloatingActionButton::new(),
            FloatingActionButtonThemeData::new(),
            none(),
        );
        assert_eq!(resolved.elevation, 6.0);
        assert_eq!(resolved.size.min_width, 56.0);
        assert_eq!(resolved.size.max_width, 56.0, "a FAB is a fixed size");
        let scheme = crate::theme::ThemeData::fallback().color_scheme;
        assert_eq!(resolved.background, scheme.primary_container());
        assert_eq!(resolved.foreground, scheme.on_primary_container());
    }

    #[test]
    fn a_negative_elevation_is_refused() {
        let mut fab = FloatingActionButton::new();
        fab.elevation = Some(-1.0);
        assert!(!fab.is_valid());
        fab.elevation = Some(0.0);
        assert!(fab.is_valid(), "resting on the surface is allowed");
    }
}
