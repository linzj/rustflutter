//! Conditional presence -- ports of upstream's `widgets/indexed_stack.dart`
//! (the `Visibility` half), `widgets/expansible.dart`,
//! `widgets/orientation_builder.dart`, `widgets/title.dart` and
//! `widgets/status_transitions.dart`.
//!
//! What ties them together is the question of **how much of a widget survives
//! being hidden**. "Invisible" is not one state: a hidden thing may or may not
//! keep its `State`, keep animating, keep taking up room, be announced to a
//! screen reader, accept taps, or accept focus. [`Visibility`] makes all six
//! separately choosable, and then constrains them into a ladder -- because
//! most of the combinations are not coherent.

use crate::engine::Color;
use crate::services::system::ApplicationSwitcherDescription;

/// Upstream `Visibility`: hide a child, with six independent decisions about
/// what "hidden" means.
///
/// The flags are **not** independent in practice, and upstream's five
/// assertions are what say so. They form a ladder, each rung strictly more
/// expensive than the one below:
///
/// ```text
/// maintainState  <--  maintainAnimation  <--  maintainSize  <--  maintainSemantics
///       ^                                            ^
///       |                                            +-------  maintainInteractivity
///  maintainFocusability
/// ```
///
/// Read the arrows as "requires". You cannot keep the size of something that
/// is not animating, because keeping size means keeping it laid out and a
/// laid-out subtree whose tickers are off is a subtree frozen mid-animation.
/// You cannot announce something that takes no space, because a screen reader
/// navigates by geometry. And you cannot let taps reach something with no
/// area to be tapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    pub visible: bool,
    /// Keep the subtree's `State` alive.
    pub maintain_state: bool,
    /// Keep its tickers running.
    pub maintain_animation: bool,
    /// Keep it laid out, occupying its space.
    pub maintain_size: bool,
    /// Keep it in the semantics tree.
    pub maintain_semantics: bool,
    /// Let pointer events through to it.
    pub maintain_interactivity: bool,
    /// Let it stay in the focus tree.
    pub maintain_focusability: bool,
}

/// Which of upstream's five assertions a configuration breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityError {
    /// "Cannot maintain animations if the state is not also maintained."
    AnimationWithoutState,
    /// "Cannot maintain size if animations are not maintained."
    SizeWithoutAnimation,
    /// "Cannot maintain semantics if size is not maintained."
    SemanticsWithoutSize,
    /// "Cannot maintain interactivity if size is not maintained."
    InteractivityWithoutSize,
    /// "Cannot maintain focusability if the state is not also maintained."
    FocusabilityWithoutState,
}

impl Visibility {
    /// A plain `Visibility`, which keeps **nothing**: hiding it takes the
    /// subtree out of the tree entirely, `State` and all.
    ///
    /// That default is the one worth knowing, because it is the one that
    /// surprises: toggling `visible` on a default `Visibility` destroys and
    /// rebuilds the subtree, so a scroll position or a half-typed field inside
    /// it is gone.
    pub fn new(visible: bool) -> Visibility {
        Visibility {
            visible,
            maintain_state: false,
            maintain_animation: false,
            maintain_size: false,
            maintain_semantics: false,
            maintain_interactivity: false,
            maintain_focusability: false,
        }
    }

    /// Upstream's `Visibility.maintain` constructor: **everything on**.
    ///
    /// It exists because the full ladder is the common case for a caller who
    /// wants "still there, just not drawn" -- and spelling out six booleans
    /// correctly every time is how the assertions get tripped.
    pub fn maintain(visible: bool) -> Visibility {
        Visibility {
            visible,
            maintain_state: true,
            maintain_animation: true,
            maintain_size: true,
            maintain_semantics: true,
            maintain_interactivity: true,
            maintain_focusability: true,
        }
    }

    /// Upstream's five assertions, in the order it makes them.
    pub fn check(&self) -> Result<(), VisibilityError> {
        if self.maintain_animation && !self.maintain_state {
            return Err(VisibilityError::AnimationWithoutState);
        }
        if self.maintain_size && !self.maintain_animation {
            return Err(VisibilityError::SizeWithoutAnimation);
        }
        if self.maintain_semantics && !self.maintain_size {
            return Err(VisibilityError::SemanticsWithoutSize);
        }
        if self.maintain_interactivity && !self.maintain_size {
            return Err(VisibilityError::InteractivityWithoutSize);
        }
        if self.maintain_focusability && !self.maintain_state {
            return Err(VisibilityError::FocusabilityWithoutState);
        }
        Ok(())
    }

    /// Whether the child is excluded from the focus tree.
    ///
    /// Note this is applied **whether or not** the child is visible -- the
    /// wrapper is always there and its `excluding` argument does the work. A
    /// visible `Visibility` excludes nothing.
    pub fn excludes_focus(&self) -> bool {
        !self.visible && !self.maintain_focusability
    }

    /// Whether pointer events are blocked.
    pub fn ignores_pointer(&self) -> bool {
        !self.visible && !self.maintain_interactivity
    }

    /// Whether the subtree is offstage: laid out at zero size but still built.
    ///
    /// Reached only on the `maintainState` branch -- upstream asserts the
    /// three size-related flags are false there, which is the ladder holding.
    pub fn is_offstage(&self) -> bool {
        !self.visible && self.maintain_state && !self.maintain_size
    }

    /// Whether tickers are switched off.
    pub fn tickers_disabled(&self) -> bool {
        !self.visible && self.maintain_state && !self.maintain_animation
    }

    /// Whether the subtree is replaced by [`Visibility::replacement`] rather
    /// than kept at all.
    pub fn uses_replacement(&self) -> bool {
        !self.visible && !self.maintain_state && !self.maintain_size
    }

    /// Upstream's `Visibility.of`, which walks **every** ancestor scope rather
    /// than stopping at the nearest.
    ///
    /// A child is visible only if every `Visibility` above it says so, which
    /// is the only correct answer: one invisible ancestor hides everything
    /// under it however visible the rest claim to be. The walk stops early on
    /// the first `false`, since nothing further up can bring it back.
    pub fn of(ancestors: &[bool]) -> bool {
        ancestors.iter().all(|visible| *visible)
    }
}

/// Upstream `SliverVisibility`: the same six decisions, for a sliver.
///
/// It is a separate class rather than a flag because the replacement differs
/// in kind: a hidden box collapses to a zero-size box, a hidden sliver to a
/// zero-extent sliver, and there is no widget that is both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SliverVisibility {
    pub visibility: Visibility,
}

impl SliverVisibility {
    pub fn new(visible: bool) -> SliverVisibility {
        SliverVisibility {
            visibility: Visibility::new(visible),
        }
    }

    pub fn maintain(visible: bool) -> SliverVisibility {
        SliverVisibility {
            visibility: Visibility::maintain(visible),
        }
    }

    pub fn check(&self) -> Result<(), VisibilityError> {
        self.visibility.check()
    }
}

/// Upstream `ExpansibleController`: opens and closes an [`Expansible`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExpansibleController {
    expanded: bool,
    notifications: usize,
}

impl ExpansibleController {
    pub fn new(expanded: bool) -> ExpansibleController {
        ExpansibleController {
            expanded,
            notifications: 0,
        }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's `expand`, which is an **end state rather than a toggle**:
    /// calling it on something already expanded has no effect, and notifies
    /// nobody.
    ///
    /// Its doc also carries the warning these controllers all share: it may
    /// rebuild the widget, so it may not be called from a build method.
    pub fn expand(&mut self) {
        self.set_expansion_state(true);
    }

    pub fn collapse(&mut self) {
        self.set_expansion_state(false);
    }

    fn set_expansion_state(&mut self, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.notifications += 1;
    }
}

/// Upstream `Expansible`: a header that reveals a body.
///
/// It is deliberately **unstyled** -- upstream extracted it out of
/// `ExpansionTile` so that Material and Cupertino could share the mechanism
/// without sharing the look. What is left is the state, the controller, and
/// the animation's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Expansible {
    /// Whether the body is built while collapsed. Upstream's
    /// `maintainState`, false by default for the same reason
    /// [`Visibility`]'s is: a collapsed panel that keeps its subtree is paying
    /// for something nobody can see.
    pub maintain_state: bool,
}

impl Expansible {
    pub fn new() -> Expansible {
        Expansible::default()
    }

    /// Whether the body exists in the tree.
    pub fn builds_body(&self, expanded: bool) -> bool {
        expanded || self.maintain_state
    }
}

/// Upstream `Orientation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

/// Upstream `OrientationBuilder`: the orientation of **the space this widget
/// was given**.
///
/// It reads the incoming constraints: wider than tall is landscape. So a tall
/// column inside a landscape phone is *portrait* to this builder, which is
/// usually what a caller laying that column out actually wants to know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrientationBuilder;

impl OrientationBuilder {
    /// Upstream's `_buildWithConstraints`.
    ///
    /// The comparison is strict, so a **square** space is portrait. Something
    /// had to break the tie, and portrait is the safer default -- a layout
    /// that assumed landscape in a square would have nowhere to put a wide
    /// row.
    pub fn orientation_of(max_width: f32, max_height: f32) -> Orientation {
        if max_width > max_height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        }
    }
}

/// Upstream `DeviceOrientationBuilder`: the orientation of **the device**.
///
/// The pair exists because the two questions differ, and confusing them is a
/// real bug: a sidebar occupying a third of a landscape tablet has portrait
/// constraints and a landscape device, and a caller asking "should I show the
/// tablet layout" wants the second while one asking "is my box wide" wants the
/// first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceOrientationBuilder;

impl DeviceOrientationBuilder {
    /// Upstream reads `MediaQuery.orientationOf`, which is the view's, not the
    /// constraints'.
    pub fn orientation_of(media_query_orientation: Orientation) -> Orientation {
        media_query_orientation
    }
}

/// Upstream `Title`: the application's name and colour, as the task switcher
/// shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Title {
    pub title: String,
    pub color: Color,
}

impl Title {
    /// Upstream asserts the colour is **fully opaque**:
    ///
    /// ```dart
    /// assert((color.a * 255.0).round().clamp(0, 255) == 0xFF);
    /// ```
    ///
    /// It is not a colour Flutter paints -- it is handed to the operating
    /// system for the task-switcher card, which composites it against whatever
    /// it likes. A translucent one would come out a colour nobody chose.
    pub fn new(title: impl Into<String>, color: Color) -> Option<Title> {
        if color.alpha() != 0xFF {
            return None;
        }
        Some(Title::opaqued(title, color))
    }

    /// The same, with the colour **made** opaque rather than checked.
    ///
    /// This is what upstream's `WidgetsApp` does -- it hands `Title` its
    /// `color.withOpacity(1.0)`, never the application's colour as given. That
    /// forcing is the *only* reason the assert above never fires from there:
    /// `WidgetsApp.color` is a required `Color`, but nothing requires it to be
    /// opaque, and an application that passes a translucent one would crash
    /// its own root widget if the value went through unchanged.
    pub fn opaqued(title: impl Into<String>, color: Color) -> Title {
        Title {
            title: title.into(),
            color: color.with_alpha(0xFF),
        }
    }

    /// Upstream's default title is the **empty string**, not the application's
    /// name: the framework does not know what the application is called, and
    /// inventing something would put a wrong name in the switcher.
    pub fn untitled(color: Color) -> Option<Title> {
        Title::new("", color)
    }

    /// What crosses the platform channel for this title.
    ///
    /// Upstream sends `widget.color.value` -- the whole 0xAARRGGBB word, alpha
    /// included, not the three colour channels. The alpha is always 0xFF by
    /// then, so what the host reads is a fully opaque word rather than a bare
    /// RGB triple it would have to decide the top byte of itself.
    pub fn description(&self) -> ApplicationSwitcherDescription {
        ApplicationSwitcherDescription::new()
            .with_label(self.title.as_str())
            .with_primary_color(self.color.0)
    }
}

/// Upstream `_TitleState`: **when** the operating system gets told.
///
/// The widget above holds what to say; this holds the decision of whether to
/// say it again. Upstream's two lifecycle methods are the whole class:
///
/// ```dart
/// void initState() { super.initState(); _updateChrome(); }
///
/// void didUpdateWidget(covariant Title oldWidget) {
///   super.didUpdateWidget(oldWidget);
///   if (oldWidget.title != widget.title || oldWidget.color != widget.color) {
///     _updateChrome();
///   }
/// }
/// ```
///
/// Both halves matter and they say opposite things. The first is
/// unconditional: an application is told to the host **once, on the way up**,
/// before anything has changed and even when the title is the empty string.
/// The second is conditional: the root widget rebuilds on every frame that
/// touches anything, and a platform message per frame for a name that has not
/// moved would be a channel round trip sixty times a second for nothing.
///
/// The pairing is what makes an `onGenerateTitle` callback cheap. Upstream
/// documents that the callback "is called each time the [WidgetsApp]
/// rebuilds" -- so it *does* run every frame -- but the string it returns is
/// compared here, and a rebuild that regenerates the same title sends nothing.
/// The cost of a localized title is the callback, not the channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleState {
    title: Title,
    told: Vec<ApplicationSwitcherDescription>,
}

impl TitleState {
    /// Upstream's `initState`, which calls `_updateChrome` with nothing to
    /// compare against -- so the host is always told once.
    pub fn init_state(title: Title) -> TitleState {
        let told = vec![title.description()];
        TitleState { title, told }
    }

    /// Upstream's `didUpdateWidget`. Returns whether the host was told.
    ///
    /// The comparison is on **both** fields. A title that keeps its name and
    /// changes its colour still has to go: the colour is half of what the
    /// switcher card shows, and on the web the engine turns it into the page's
    /// theme colour.
    pub fn did_update_widget(&mut self, title: Title) -> bool {
        let changed = self.title != title;
        self.title = title;
        if changed {
            self.told.push(self.title.description());
        }
        changed
    }

    pub fn title(&self) -> &Title {
        &self.title
    }

    /// Every description handed to the platform, oldest first.
    pub fn told(&self) -> &[ApplicationSwitcherDescription] {
        &self.told
    }
}

/// Upstream `AnimationStatus`, as this module needs it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationStatus {
    #[default]
    Dismissed,
    Forward,
    Reverse,
    Completed,
}

/// Upstream `StatusTransitionWidget`: rebuilds when an animation's **status**
/// changes, not when its value does.
///
/// The distinction is the whole class. A value listener fires sixty times a
/// second; a status listener fires four times in the life of an animation --
/// dismissed, forward, completed, reverse. A widget that only needs to know
/// *whether* something is running, rather than how far along, pays the second
/// price instead of the first.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StatusTransitionWidget {
    animation: u64,
    status: AnimationStatus,
    listening_to: Option<u64>,
    rebuilds: usize,
}

impl StatusTransitionWidget {
    pub fn new(animation: u64) -> StatusTransitionWidget {
        StatusTransitionWidget {
            animation,
            status: AnimationStatus::Dismissed,
            listening_to: Some(animation),
            rebuilds: 0,
        }
    }

    pub fn status(&self) -> AnimationStatus {
        self.status
    }

    pub fn listening_to(&self) -> Option<u64> {
        self.listening_to
    }

    pub fn rebuilds(&self) -> usize {
        self.rebuilds
    }

    /// The animation's status changed.
    pub fn status_changed(&mut self, status: AnimationStatus) {
        self.status = status;
        self.rebuilds += 1;
    }

    /// Upstream's `didUpdateWidget`, which moves the listener **only when the
    /// animation object changed**. A rebuild that passes the same animation
    /// must not churn its listener list.
    pub fn did_update_widget(&mut self, animation: u64) {
        if self.animation == animation {
            return;
        }
        self.animation = animation;
        self.listening_to = Some(animation);
    }

    pub fn dispose(&mut self) {
        self.listening_to = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The visibility ladder ----------------------------------------------

    #[test]
    fn each_rung_of_the_ladder_requires_the_one_below_it() {
        let mut only_animation = Visibility::new(false);
        only_animation.maintain_animation = true;
        assert_eq!(
            only_animation.check(),
            Err(VisibilityError::AnimationWithoutState)
        );

        let mut only_size = Visibility::new(false);
        only_size.maintain_state = true;
        only_size.maintain_size = true;
        assert_eq!(
            only_size.check(),
            Err(VisibilityError::SizeWithoutAnimation)
        );
    }

    #[test]
    fn a_screen_reader_navigates_by_geometry_so_semantics_needs_size() {
        let mut semantics = Visibility::new(false);
        semantics.maintain_state = true;
        semantics.maintain_animation = true;
        semantics.maintain_semantics = true;
        assert_eq!(
            semantics.check(),
            Err(VisibilityError::SemanticsWithoutSize)
        );
    }

    #[test]
    fn a_tap_needs_somewhere_to_land_so_interactivity_needs_size() {
        let mut taps = Visibility::new(false);
        taps.maintain_state = true;
        taps.maintain_animation = true;
        taps.maintain_interactivity = true;
        assert_eq!(taps.check(), Err(VisibilityError::InteractivityWithoutSize));
    }

    #[test]
    fn focusability_hangs_off_state_rather_than_size() {
        // The one rung that does not go through the size chain: a focusable
        // thing needs to exist, not to occupy space.
        let mut focus = Visibility::new(false);
        focus.maintain_focusability = true;
        assert_eq!(
            focus.check(),
            Err(VisibilityError::FocusabilityWithoutState)
        );

        focus.maintain_state = true;
        assert!(focus.check().is_ok(), "state is all it needed");
    }

    #[test]
    fn the_whole_ladder_together_is_legal_and_is_what_maintain_gives_you() {
        assert!(Visibility::maintain(false).check().is_ok());
        let all = Visibility::maintain(false);
        assert!(all.maintain_state && all.maintain_animation && all.maintain_size);
        assert!(all.maintain_semantics && all.maintain_interactivity);
        assert!(all.maintain_focusability);
    }

    #[test]
    fn a_plain_visibility_keeps_nothing_at_all() {
        // The default that surprises: toggling it destroys and rebuilds the
        // subtree, so a scroll position inside it is gone.
        let plain = Visibility::new(false);
        assert!(plain.check().is_ok());
        assert!(!plain.maintain_state);
        assert!(plain.uses_replacement());
    }

    // -- What hiding actually does -----------------------------------------

    #[test]
    fn a_visible_widget_excludes_and_ignores_nothing() {
        let shown = Visibility::new(true);
        assert!(!shown.excludes_focus());
        assert!(!shown.ignores_pointer());
        assert!(!shown.is_offstage());
        assert!(!shown.tickers_disabled());
        assert!(!shown.uses_replacement());
    }

    #[test]
    fn hiding_with_state_kept_goes_offstage_with_its_tickers_off() {
        let mut kept = Visibility::new(false);
        kept.maintain_state = true;
        assert!(kept.check().is_ok());
        assert!(kept.is_offstage());
        assert!(kept.tickers_disabled(), "frozen where it stood");
        assert!(!kept.uses_replacement());
    }

    #[test]
    fn keeping_the_animation_leaves_the_tickers_running() {
        let mut animating = Visibility::new(false);
        animating.maintain_state = true;
        animating.maintain_animation = true;
        assert!(!animating.tickers_disabled());
        assert!(animating.is_offstage(), "still not laid out");
    }

    #[test]
    fn keeping_the_size_stops_it_being_offstage() {
        let mut sized = Visibility::maintain(false);
        sized.maintain_semantics = false;
        sized.maintain_interactivity = false;
        assert!(!sized.is_offstage(), "it occupies its space");
        assert!(sized.check().is_ok());
    }

    #[test]
    fn interactivity_and_focusability_each_lift_their_own_barrier() {
        let mut hidden = Visibility::maintain(false);
        assert!(!hidden.ignores_pointer());
        assert!(!hidden.excludes_focus());

        hidden.maintain_interactivity = false;
        assert!(hidden.ignores_pointer());

        hidden.maintain_focusability = false;
        assert!(hidden.excludes_focus());
    }

    #[test]
    fn one_invisible_ancestor_hides_everything_under_it() {
        // However visible the rest claim to be.
        assert!(Visibility::of(&[true, true, true]));
        assert!(!Visibility::of(&[true, false, true]));
        assert!(Visibility::of(&[]), "no ancestors means visible");
    }

    #[test]
    fn a_sliver_visibility_has_the_same_ladder() {
        assert!(SliverVisibility::maintain(false).check().is_ok());
        let mut broken = SliverVisibility::new(false);
        broken.visibility.maintain_animation = true;
        assert_eq!(broken.check(), Err(VisibilityError::AnimationWithoutState));
    }

    // -- Expansible ----------------------------------------------------------

    #[test]
    fn expanding_is_an_end_state_rather_than_a_toggle() {
        let mut controller = ExpansibleController::new(false);
        controller.expand();
        assert!(controller.is_expanded());
        assert_eq!(controller.notifications(), 1);

        controller.expand();
        assert_eq!(controller.notifications(), 1, "already there");

        controller.collapse();
        assert!(!controller.is_expanded());
        assert_eq!(controller.notifications(), 2);
    }

    #[test]
    fn a_collapsed_panel_does_not_build_its_body_unless_told_to() {
        // A collapsed panel keeping its subtree is paying for something nobody
        // can see.
        let plain = Expansible::new();
        assert!(!plain.builds_body(false));
        assert!(plain.builds_body(true));

        let mut kept = Expansible::new();
        kept.maintain_state = true;
        assert!(kept.builds_body(false));
    }

    // -- The two orientations ------------------------------------------------

    #[test]
    fn one_builder_asks_about_the_box_and_the_other_about_the_device() {
        // A sidebar taking a third of a landscape tablet has portrait
        // constraints and a landscape device, and the two callers want
        // different answers.
        assert_eq!(
            OrientationBuilder::orientation_of(300.0, 900.0),
            Orientation::Portrait,
            "a narrow column"
        );
        assert_eq!(
            DeviceOrientationBuilder::orientation_of(Orientation::Landscape),
            Orientation::Landscape,
            "on a device held sideways"
        );
    }

    #[test]
    fn a_square_space_is_portrait_because_the_comparison_is_strict() {
        // Something had to break the tie, and a layout that assumed landscape
        // in a square would have nowhere to put a wide row.
        assert_eq!(
            OrientationBuilder::orientation_of(400.0, 400.0),
            Orientation::Portrait
        );
        assert_eq!(
            OrientationBuilder::orientation_of(401.0, 400.0),
            Orientation::Landscape
        );
    }

    // -- Title ---------------------------------------------------------------

    #[test]
    fn a_translucent_title_colour_is_refused() {
        // It is handed to the operating system, which composites it against
        // whatever it likes -- a translucent one comes out a colour nobody
        // chose.
        assert!(Title::new("App", Color(0xFF00_7ACC)).is_some());
        assert!(Title::new("App", Color(0x8000_7ACC)).is_none());
        assert!(Title::new("App", Color(0x0000_0000)).is_none());
    }

    #[test]
    fn the_default_title_is_empty_rather_than_a_guess() {
        // The framework does not know what the application is called, and
        // inventing something would put a wrong name in the task switcher.
        let untitled = Title::untitled(Color(0xFF00_0000)).unwrap();
        assert_eq!(untitled.title, "");
    }

    #[test]
    fn the_host_is_told_once_on_the_way_up_even_with_nothing_to_say() {
        // `initState` calls `_updateChrome` with nothing to compare against.
        // An application whose title is the empty string still sends it.
        let state = TitleState::init_state(Title::untitled(Color(0xFF00_0000)).unwrap());
        assert_eq!(state.told().len(), 1);
        assert_eq!(state.told()[0].label.as_deref(), Some(""));
    }

    #[test]
    fn a_rebuild_that_changed_nothing_sends_no_platform_message() {
        // The root widget rebuilds on every frame that touches anything, and
        // an `onGenerateTitle` callback re-runs each time. Comparing here is
        // what keeps a localized title from costing a channel round trip
        // sixty times a second.
        let same = Title::new("Inbox", Color(0xFF00_7ACC)).unwrap();
        let mut state = TitleState::init_state(same.clone());
        assert!(!state.did_update_widget(same.clone()));
        assert!(!state.did_update_widget(same));
        assert_eq!(state.told().len(), 1);
    }

    #[test]
    fn either_half_changing_on_its_own_is_enough_to_send() {
        // Upstream compares both fields. The colour is half of what the
        // switcher card shows -- and on the web the engine turns it into the
        // page's theme colour -- so a rename-free recolour still has to go.
        let mut state = TitleState::init_state(Title::new("Inbox", Color(0xFF00_7ACC)).unwrap());

        assert!(state.did_update_widget(Title::new("Drafts", Color(0xFF00_7ACC)).unwrap()));
        assert_eq!(state.told().len(), 2);

        assert!(state.did_update_widget(Title::new("Drafts", Color(0xFFCC_2200)).unwrap()));
        assert_eq!(state.told().len(), 3);
        assert_eq!(state.told()[2].primary_color, Some(0xFFCC_2200));
    }

    #[test]
    fn what_crosses_is_the_whole_word_alpha_included() {
        // Upstream sends `color.value`, not the three colour channels. The
        // alpha is 0xFF by then, so the host reads a fully opaque word rather
        // than having to decide the top byte itself.
        let described = Title::new("Inbox", Color(0xFF00_7ACC))
            .unwrap()
            .description();
        assert_eq!(described.label.as_deref(), Some("Inbox"));
        assert_eq!(described.primary_color, Some(0xFF00_7ACC));
    }

    #[test]
    fn widgets_app_forces_the_colour_where_title_only_refuses_it() {
        // The two constructors differ on purpose: `Title` itself asserts, and
        // `WidgetsApp` hands it `color.withOpacity(1.0)` so the assert is
        // unreachable from there.
        assert!(Title::new("App", Color(0x2200_7ACC)).is_none());
        assert_eq!(
            Title::opaqued("App", Color(0x2200_7ACC)).color,
            Color(0xFF00_7ACC)
        );
    }

    // -- Status transitions --------------------------------------------------

    #[test]
    fn a_status_listener_fires_four_times_where_a_value_listener_fires_sixty() {
        // A widget that only needs to know *whether* something is running pays
        // the cheaper price.
        let mut widget = StatusTransitionWidget::new(1);
        assert_eq!(widget.status(), AnimationStatus::Dismissed);
        assert_eq!(widget.rebuilds(), 0);

        widget.status_changed(AnimationStatus::Forward);
        widget.status_changed(AnimationStatus::Completed);
        assert_eq!(widget.rebuilds(), 2);
        assert_eq!(widget.status(), AnimationStatus::Completed);
    }

    #[test]
    fn the_listener_moves_only_when_the_animation_object_changed() {
        // A rebuild passing the same animation must not churn its listener
        // list.
        let mut widget = StatusTransitionWidget::new(1);
        widget.did_update_widget(1);
        assert_eq!(widget.listening_to(), Some(1));

        widget.did_update_widget(2);
        assert_eq!(widget.listening_to(), Some(2));

        widget.dispose();
        assert_eq!(widget.listening_to(), None);
    }
}
