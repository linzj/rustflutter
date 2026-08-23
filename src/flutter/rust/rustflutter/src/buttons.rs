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

    /// This button's style, from `IconButtonTheme` merged over the ambient
    /// icon theme -- see [`crate::component_themes::ResolvedIconButton`] for
    /// why it is a merge here and a `copy_with` inside a list tile or an app
    /// bar.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
        icon_theme: &crate::component_themes::IconThemeData,
    ) -> crate::component_themes::ResolvedIconButton {
        crate::component_themes::ResolvedIconButton::of(context, icon_theme)
    }

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
    fn a_state_specific_height_beats_the_resting_one_it_falls_back_to() {
        // `self.hover_elevation.or(self.elevation)` -- setting only one of the
        // two cannot show which wins.
        let mut mine = FloatingActionButton::new();
        mine.elevation = Some(20.0);
        mine.hover_elevation = Some(30.0);
        mine.focus_elevation = Some(40.0);
        mine.disabled_elevation = Some(50.0);
        let data = FloatingActionButtonThemeData::new();

        assert_eq!(
            resolve(mine, data.clone(), none().with(WidgetState::Hovered)).elevation,
            30.0
        );
        let mut mine = FloatingActionButton::new();
        mine.elevation = Some(20.0);
        mine.focus_elevation = Some(40.0);
        assert_eq!(
            resolve(mine, data.clone(), none().with(WidgetState::Focused)).elevation,
            40.0
        );
        let mut mine = FloatingActionButton::new();
        mine.elevation = Some(20.0);
        mine.disabled_elevation = Some(50.0);
        assert_eq!(
            resolve(mine, data, none().with(WidgetState::Disabled)).elevation,
            50.0
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

#[cfg(test)]
mod icon_button_theme_tests {
    use super::*;
    use crate::component_themes::{
        ButtonStyle, IconButtonTheme, IconButtonThemeData, IconThemeData, ResolvedIconButton,
    };
    use crate::engine::Color;
    use crate::framework::{
        AnyWidget, BuildContext, Component, ElementTree, component, leaf, provide,
    };
    use crate::theme::ThemeData;
    use crate::widget_state::{StateProperty, WidgetStates};
    use crate::widgets::SizedBox;

    struct Reader {
        read: std::rc::Rc<dyn Fn(&mut BuildContext) -> ResolvedIconButton>,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedIconButton>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some((self.read)(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn read_under(
        data: IconButtonThemeData,
        read: impl Fn(&mut BuildContext) -> ResolvedIconButton + 'static,
    ) -> ResolvedIconButton {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            ThemeData::light(),
            IconButtonTheme::new(
                data,
                component(Reader {
                    read: std::rc::Rc::new(read),
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn themed(style: Option<ButtonStyle>) -> IconButtonThemeData {
        IconButtonThemeData { style }
    }

    fn colour_and_size(color: Option<Color>, size: Option<f32>) -> ButtonStyle {
        let mut style = ButtonStyle::new();
        if let Some(color) = color {
            style.foreground_color = Some(StateProperty::all(Some(color)));
        }
        if let Some(size) = size {
            style.icon_size = Some(StateProperty::all(Some(size)));
        }
        style
    }

    fn icons(color: Option<Color>, size: Option<f32>) -> IconThemeData {
        let mut data = IconThemeData::new();
        data.color = color;
        data.size = size;
        data
    }

    const NONE: WidgetStates = WidgetStates::NONE;

    #[test]
    fn the_button_theme_wins_and_the_icon_theme_fills_its_gaps() {
        // Upstream's own doc: "if any of the properties exist in both
        // [IconButtonTheme] and [IconTheme], [IconTheme] will be overridden."
        let from_theme = Color(0xFFAA0000);
        let from_icons = Color(0xFF00AA00);
        let resolved = read_under(
            themed(Some(colour_and_size(Some(from_theme), None))),
            move |context| ResolvedIconButton::of(context, &icons(Some(from_icons), Some(30.0))),
        );
        assert_eq!(resolved.foreground(NONE), Some(from_theme));
        assert_eq!(
            resolved.icon_size(NONE),
            Some(30.0),
            "and the size the theme said nothing about still arrives"
        );
    }

    #[test]
    fn which_a_null_coalescing_ladder_would_not_have_done() {
        // The point of a merge being per-field. Written as
        // `themeStyle ?? iconThemeStyle` the first non-null style would take
        // everything, and setting one field would silently drop the other.
        let resolved = read_under(
            themed(Some(colour_and_size(Some(Color(0xFFAA0000)), None))),
            move |context| ResolvedIconButton::of(context, &icons(None, Some(30.0))),
        );
        assert!(
            resolved.foreground(NONE).is_some() && resolved.icon_size(NONE).is_some(),
            "both sources landed in one style"
        );
    }

    #[test]
    fn a_list_tile_overrides_the_theme_where_the_button_deferred_to_it() {
        // Same theme, opposite answer. A bare icon button has no opinion about
        // what is behind it; a list tile painted its own background and has to
        // impose a colour that reads against it.
        let from_theme = Color(0xFFAA0000);
        let imposed = Color(0xFF0000AA);
        let data = themed(Some(colour_and_size(Some(from_theme), None)));

        let deferring = read_under(data.clone(), move |context| {
            ResolvedIconButton::of(context, &icons(None, None))
        });
        let forcing = read_under(data, move |context| {
            ResolvedIconButton::forced_foreground(context, imposed)
        });

        assert_eq!(deferring.foreground(NONE), Some(from_theme));
        assert_eq!(forcing.foreground(NONE), Some(imposed));
        assert_ne!(deferring.foreground(NONE), forcing.foreground(NONE));
    }

    #[test]
    fn but_the_forcing_reader_keeps_everything_it_did_not_name() {
        // `copyWith(foregroundColor:)` replaces one field, not the style. A
        // list tile imposes a colour and leaves the theme's sizing alone.
        let resolved = read_under(
            themed(Some(colour_and_size(Some(Color(0xFFAA0000)), Some(18.0)))),
            move |context| ResolvedIconButton::forced_foreground(context, Color(0xFF0000AA)),
        );
        assert_eq!(resolved.foreground(NONE), Some(Color(0xFF0000AA)));
        assert_eq!(resolved.icon_size(NONE), Some(18.0));
    }

    #[test]
    fn the_ambient_icon_theme_contributes_only_what_was_chosen() {
        // `iconThemeStyle` is built from `isDefaultColor ? null : color`, so a
        // default-valued icon theme enters as nothing at all. That is what
        // makes merging it under the theme safe: it cannot re-assert a
        // fallback the theme was replacing.
        let untouched = ResolvedIconButton::from_icon_theme(
            &icons(Some(ResolvedIconButton::DEFAULT_DARK), Some(24.0)),
            false,
        );
        assert_eq!(untouched.foreground_color, None);
        assert_eq!(untouched.icon_size, None);

        let chosen =
            ResolvedIconButton::from_icon_theme(&icons(Some(Color(0xFF123456)), Some(31.0)), false);
        assert!(chosen.foreground_color.is_some());
        assert!(chosen.icon_size.is_some());
    }

    #[test]
    fn the_default_it_is_filtered_against_follows_the_brightness() {
        // Light mode's default is the dark ink and dark mode's is the light,
        // so the same icon theme is "untouched" in one and "chosen" in the
        // other.
        let light_default = icons(Some(ResolvedIconButton::DEFAULT_DARK), None);
        assert_eq!(
            ResolvedIconButton::from_icon_theme(&light_default, false).foreground_color,
            None
        );
        assert!(
            ResolvedIconButton::from_icon_theme(&light_default, true)
                .foreground_color
                .is_some(),
            "in the dark, that same colour was deliberate"
        );
    }

    #[test]
    fn with_no_theme_the_icon_theme_is_the_whole_answer() {
        let mine = Color(0xFF00AA00);
        let resolved = read_under(themed(None), move |context| {
            ResolvedIconButton::of(context, &icons(Some(mine), Some(30.0)))
        });
        assert_eq!(resolved.foreground(NONE), Some(mine));
        assert_eq!(resolved.icon_size(NONE), Some(30.0));
    }

    #[test]
    fn a_forcing_reader_needs_no_theme_to_work_from() {
        // Upstream's `?? IconButton.styleFrom(foregroundColor:)`: with nothing
        // to copy, it builds a style around the one field it cares about.
        let imposed = Color(0xFF0000AA);
        let resolved = read_under(themed(None), move |context| {
            ResolvedIconButton::forced_foreground(context, imposed)
        });
        assert_eq!(resolved.foreground(NONE), Some(imposed));
        assert_eq!(resolved.icon_size(NONE), None);
    }
}

#[cfg(test)]
mod material_button_color_tests {
    use super::*;
    use crate::component_themes::{
        BRIGHTNESS_THRESHOLD, ButtonTextTheme, MaterialButtonColors, estimate_brightness_for_color,
    };
    use crate::engine::Color;
    use crate::platform::Brightness;
    use crate::theme::ThemeData;

    fn scheme() -> crate::color_scheme::ColorScheme {
        ThemeData::fallback().color_scheme
    }

    // -- The estimate ----------------------------------------------------------

    #[test]
    fn luminance_weights_green_far_above_blue() {
        // 0.2126 / 0.7152 / 0.0722: not a colour-space convenience but how
        // much the eye gets from each channel.
        let red = Color::rgb(255, 0, 0).compute_luminance();
        let green = Color::rgb(0, 255, 0).compute_luminance();
        let blue = Color::rgb(0, 0, 255).compute_luminance();
        assert!(green > red && red > blue);
        assert!((green - 0.7152).abs() < 0.001);
        assert!((blue - 0.0722).abs() < 0.001);
        assert!((Color::WHITE.compute_luminance() - 1.0).abs() < 0.001);
        assert!(Color::BLACK.compute_luminance() < 0.001);
    }

    #[test]
    fn the_gamma_curve_is_what_makes_mid_grey_dark() {
        // A mutation deleting the un-gamma'ing survived every test above,
        // because all of them use colours at the ends of the curve -- 0 and
        // 255 -- where the curve and a straight line agree. Half way along is
        // where they do not: 50 per cent grey has about 21 per cent of the
        // light, not 50.
        let mid = Color::rgb(128, 128, 128).compute_luminance();
        assert!(
            (mid - 0.2158).abs() < 0.002,
            "mid grey should be about 0.216, was {mid}"
        );
        assert!(mid < 0.3, "and nowhere near the 0.5 a linear reading gives");

        // The other side of the knee, below 0.03928, where the curve *is* a
        // straight line -- divided by 12.92 rather than raised to a power.
        let very_dark = Color::rgb(5, 5, 5).compute_luminance();
        assert!((very_dark - (5.0 / 255.0) / 12.92).abs() < 0.0001);
    }

    #[test]
    fn alpha_takes_no_part_in_it() {
        // Luminance is a property of the colour, not of what compositing it
        // would produce.
        assert_eq!(
            Color::argb(255, 40, 90, 200).compute_luminance(),
            Color::argb(7, 40, 90, 200).compute_luminance()
        );
    }

    #[test]
    fn the_threshold_is_materials_and_not_the_specs() {
        // WCAG says 0.0525; upstream uses 0.15 because "Material Design
        // appears to bias more towards using light text". The higher
        // threshold makes *more* colours count as light, so more get dark
        // text.
        assert_eq!(BRIGHTNESS_THRESHOLD, 0.15);
        assert!(BRIGHTNESS_THRESHOLD > 0.0525);

        // A colour that the spec would call light and Material calls dark.
        let between = (0.15f32).sqrt() - 0.05;
        let spec = (0.0525f32).sqrt() - 0.05;
        assert!(between > spec, "the band the two disagree over exists");
    }

    #[test]
    fn white_is_light_and_black_is_dark() {
        assert_eq!(
            estimate_brightness_for_color(Color::WHITE),
            Brightness::Light
        );
        assert_eq!(
            estimate_brightness_for_color(Color::BLACK),
            Brightness::Dark
        );
    }

    // -- The label ladder -------------------------------------------------------

    #[test]
    fn a_primary_label_is_chosen_against_its_fill_and_not_against_the_page() {
        // The finding: a dark button on a light page gets white text, which
        // asking the page would have got wrong.
        let on_dark_fill = MaterialButtonColors::text_color(
            true,
            None,
            None,
            ButtonTextTheme::Primary,
            Some(Color::BLACK),
            Brightness::Light,
            &scheme(),
        );
        assert_eq!(on_dark_fill, Color::WHITE);

        let on_light_fill = MaterialButtonColors::text_color(
            true,
            None,
            None,
            ButtonTextTheme::Primary,
            Some(Color::WHITE),
            Brightness::Dark,
            &scheme(),
        );
        assert_eq!(on_light_fill, Color::BLACK);
    }

    #[test]
    fn and_falls_back_to_the_page_only_when_there_is_no_fill() {
        assert_eq!(
            MaterialButtonColors::text_color(
                true,
                None,
                None,
                ButtonTextTheme::Primary,
                None,
                Brightness::Dark,
                &scheme()
            ),
            Color::WHITE
        );
        assert_eq!(
            MaterialButtonColors::text_color(
                true,
                None,
                None,
                ButtonTextTheme::Primary,
                None,
                Brightness::Light,
                &scheme()
            ),
            Color::BLACK
        );
    }

    #[test]
    fn the_two_darks_are_different_darks() {
        // `normal` is body text on the page and takes the Material 2 body
        // black; `primary` sits on a fill and needs the whole of it.
        let normal = MaterialButtonColors::text_color(
            true,
            None,
            None,
            ButtonTextTheme::Normal,
            None,
            Brightness::Light,
            &scheme(),
        );
        let primary = MaterialButtonColors::text_color(
            true,
            None,
            None,
            ButtonTextTheme::Primary,
            Some(Color::WHITE),
            Brightness::Light,
            &scheme(),
        );
        assert_eq!(normal, MaterialButtonColors::BLACK87);
        assert_eq!(primary, Color::BLACK);
        assert_ne!(normal, primary);
    }

    #[test]
    fn accent_asks_neither_the_page_nor_the_fill() {
        for (fill, brightness) in [
            (Some(Color::BLACK), Brightness::Light),
            (Some(Color::WHITE), Brightness::Dark),
            (None, Brightness::Light),
        ] {
            assert_eq!(
                MaterialButtonColors::text_color(
                    true,
                    None,
                    None,
                    ButtonTextTheme::Accent,
                    fill,
                    brightness,
                    &scheme()
                ),
                scheme().secondary
            );
        }
    }

    #[test]
    fn a_disabled_button_keeps_a_text_colour_it_was_given() {
        // `getTextColor` checks disabled first, which reads as disabled
        // winning -- but `getDisabledTextColor` asks for `textColor` first
        // too, so it wins either way.
        let mine = Color(0xFFAA0000);
        assert_eq!(
            MaterialButtonColors::text_color(
                false,
                Some(mine),
                Some(Color(0xFF00FF00)),
                ButtonTextTheme::Normal,
                None,
                Brightness::Light,
                &scheme()
            ),
            mine
        );
        assert_eq!(
            MaterialButtonColors::text_color(
                true,
                Some(mine),
                None,
                ButtonTextTheme::Normal,
                None,
                Brightness::Light,
                &scheme()
            ),
            mine,
            "and enabled reaches it by the other route"
        );
    }

    #[test]
    fn all_the_disabled_branch_decides_is_what_happens_with_no_text_colour() {
        // Which is where `disabledTextColor` gets its only chance to be read.
        let disabled_only = Color(0xFF00FF00);
        assert_eq!(
            MaterialButtonColors::text_color(
                false,
                None,
                Some(disabled_only),
                ButtonTextTheme::Normal,
                None,
                Brightness::Light,
                &scheme()
            ),
            disabled_only
        );
        assert_eq!(
            MaterialButtonColors::text_color(
                true,
                None,
                Some(disabled_only),
                ButtonTextTheme::Normal,
                None,
                Brightness::Light,
                &scheme()
            ),
            MaterialButtonColors::BLACK87,
            "an enabled button never looks at it"
        );

        assert_eq!(
            MaterialButtonColors::text_color(
                false,
                None,
                None,
                ButtonTextTheme::Normal,
                None,
                Brightness::Light,
                &scheme()
            ),
            crate::elevation_overlay::with_opacity(scheme().on_surface, 0.38)
        );
    }

    // -- The fill ---------------------------------------------------------------

    #[test]
    fn a_plain_material_button_gets_no_fill_from_the_theme() {
        // Upstream's `if (button.runtimeType == MaterialButton) return null` --
        // an exact-type test, so being a `MaterialButton` and being *exactly*
        // one are different answers.
        assert_eq!(
            MaterialButtonColors::fill_color(
                true,
                None,
                None,
                true,
                Some(Color(0xFF123456)),
                ButtonTextTheme::Primary,
                &scheme()
            ),
            None
        );
        assert!(
            MaterialButtonColors::fill_color(
                true,
                None,
                None,
                false,
                None,
                ButtonTextTheme::Primary,
                &scheme()
            )
            .is_some(),
            "a subclass does get one"
        );
    }

    #[test]
    fn but_a_colour_it_was_given_beats_even_that() {
        // The exact-type gate is the *second* clause, so a button told what
        // colour to be is that colour whatever its type.
        let mine = Color(0xFF123456);
        assert_eq!(
            MaterialButtonColors::fill_color(
                true,
                Some(mine),
                None,
                true,
                None,
                ButtonTextTheme::Primary,
                &scheme()
            ),
            Some(mine)
        );
    }

    #[test]
    fn and_which_colour_it_reads_depends_on_whether_it_is_enabled() {
        let on = Color(0xFF111111);
        let off = Color(0xFF222222);
        assert_eq!(
            MaterialButtonColors::fill_color(
                true,
                Some(on),
                Some(off),
                true,
                None,
                ButtonTextTheme::Primary,
                &scheme()
            ),
            Some(on)
        );
        assert_eq!(
            MaterialButtonColors::fill_color(
                false,
                Some(on),
                Some(off),
                true,
                None,
                ButtonTextTheme::Primary,
                &scheme()
            ),
            Some(off)
        );
    }

    #[test]
    fn a_disabled_subclass_fill_is_the_same_faint_grey_either_text_theme() {
        let faint = crate::elevation_overlay::with_opacity(scheme().on_surface, 0.12);
        for text_theme in [
            ButtonTextTheme::Normal,
            ButtonTextTheme::Accent,
            ButtonTextTheme::Primary,
        ] {
            assert_eq!(
                MaterialButtonColors::fill_color(
                    false,
                    None,
                    None,
                    false,
                    None,
                    text_theme,
                    &scheme()
                ),
                Some(faint),
                "{text_theme:?}"
            );
        }
    }

    #[test]
    fn the_themes_button_colour_only_applies_while_enabled() {
        let themed = Color(0xFF654321);
        assert_eq!(
            MaterialButtonColors::fill_color(
                true,
                None,
                None,
                false,
                Some(themed),
                ButtonTextTheme::Normal,
                &scheme()
            ),
            Some(themed)
        );
        assert_ne!(
            MaterialButtonColors::fill_color(
                false,
                None,
                None,
                false,
                Some(themed),
                ButtonTextTheme::Normal,
                &scheme()
            ),
            Some(themed)
        );
    }
}
