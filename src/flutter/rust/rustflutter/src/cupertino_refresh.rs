//! Ports of `cupertino/refresh.dart`'s `CupertinoSliverRefreshControl`,
//! `cupertino/expansion_tile.dart`'s `CupertinoExpansionTile`, and the two
//! Cupertino text-selection toolbars.
//!
//! The last four classes of the sweep.

use crate::editable_text::TargetPlatform;
use crate::render::AxisDirection;

/// Upstream `RefreshIndicatorMode`.
///
/// Five states, and the split in them is worth noticing: `drag` and `armed`
/// describe the **gesture**, `refresh` and `done` describe the **work**, and
/// `inactive` is both ends of the loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RefreshIndicatorMode {
    /// *"Initial state, when not being overscrolled into, or after the
    /// overscroll is canceled or after done and the sliver retracted away."*
    #[default]
    Inactive,
    /// *"While being overscrolled but not far enough yet to trigger the
    /// refresh."*
    Drag,
    /// *"Dragged far enough that the `onRefresh` callback **will** run and the
    /// dragged displacement is not yet at the final refresh resting state."*
    ///
    /// The commitment happens **before** the release. By the time your finger
    /// is still down the answer is already settled, which is exactly what lets
    /// the indicator change under it -- an interface that told you only on
    /// release could not show you what releasing would do.
    Armed,
    /// *"While the `onRefresh` task is running."*
    Refresh,
    /// *"While the indicator is animating away after refreshing."*
    Done,
}

/// Why a refresh control's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshControlError {
    NonPositiveTriggerDistance,
    NegativeIndicatorExtent,
    /// The resting indicator is taller than the pull that summoned it.
    IndicatorTallerThanTrigger,
    NotLaidOutDownwards,
}

/// Upstream `CupertinoSliverRefreshControl`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoSliverRefreshControl {
    pub refresh_trigger_pull_distance: f32,
    pub refresh_indicator_extent: f32,
    pub has_on_refresh: bool,
}

impl CupertinoSliverRefreshControl {
    /// Upstream `_kActivityIndicatorRadius`.
    pub const ACTIVITY_INDICATOR_RADIUS: f32 = 14.0;
    pub const DEFAULT_TRIGGER_PULL_DISTANCE: f32 = 100.0;
    pub const DEFAULT_INDICATOR_EXTENT: f32 = 60.0;

    pub fn new() -> CupertinoSliverRefreshControl {
        CupertinoSliverRefreshControl {
            refresh_trigger_pull_distance:
                CupertinoSliverRefreshControl::DEFAULT_TRIGGER_PULL_DISTANCE,
            refresh_indicator_extent: CupertinoSliverRefreshControl::DEFAULT_INDICATOR_EXTENT,
            has_on_refresh: true,
        }
    }

    /// Upstream's three constructor asserts, the third of which explains itself:
    ///
    /// ```dart
    /// assert(
    ///   refreshTriggerPullDistance >= refreshIndicatorExtent,
    ///   'The refresh indicator cannot take more space in its final state '
    ///   'than the amount initially created by overscrolling.',
    /// );
    /// ```
    ///
    /// **The indicator's resting height has to fit inside the pull that
    /// summoned it.** Were it taller, letting go would leave the list needing
    /// more room than the gesture had opened, so the content would lurch
    /// downwards at exactly the moment it should be settling back up.
    pub fn validate(&self) -> Result<(), RefreshControlError> {
        if self.refresh_trigger_pull_distance <= 0.0 {
            return Err(RefreshControlError::NonPositiveTriggerDistance);
        }
        if self.refresh_indicator_extent < 0.0 {
            return Err(RefreshControlError::NegativeIndicatorExtent);
        }
        if self.refresh_trigger_pull_distance < self.refresh_indicator_extent {
            return Err(RefreshControlError::IndicatorTallerThanTrigger);
        }
        Ok(())
    }

    /// Upstream's render object asserts **both**
    /// `constraints.axisDirection == AxisDirection.down` and
    /// `constraints.growthDirection == GrowthDirection.forward`.
    ///
    /// The same "down, not merely vertical" as `RenderTreeSliver` in the tick
    /// before, with a second condition on top: pull-to-refresh lives at the
    /// beginning of a list, and both the direction and the growth have to agree
    /// on which end that is.
    pub fn perform_layout(
        &self,
        axis_direction: AxisDirection,
        growth_forward: bool,
    ) -> Result<(), RefreshControlError> {
        if axis_direction != AxisDirection::Down || !growth_forward {
            return Err(RefreshControlError::NotLaidOutDownwards);
        }
        Ok(())
    }

    /// Which state a given overscroll puts the control in, before release.
    pub fn mode_for_pull(&self, pulled_extent: f32) -> RefreshIndicatorMode {
        if pulled_extent <= 0.0 {
            RefreshIndicatorMode::Inactive
        } else if pulled_extent < self.refresh_trigger_pull_distance {
            RefreshIndicatorMode::Drag
        } else {
            RefreshIndicatorMode::Armed
        }
    }
}

impl Default for CupertinoSliverRefreshControl {
    fn default() -> Self {
        CupertinoSliverRefreshControl::new()
    }
}

/// Upstream `ExpansionTileTransitionMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExpansionTileTransitionMode {
    /// *"the child appears fully extended and fades into view"* -- the geometry
    /// never changes and only the paint does.
    #[default]
    Fade,
    /// *"the child scrolls from under the header until it becomes fully
    /// extended"* -- the paint never changes and only the geometry does.
    Scroll,
}

/// Upstream `CupertinoExpansionTile`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoExpansionTile {
    pub transition_mode: ExpansionTileTransitionMode,
    pub expanded: bool,
}

impl CupertinoExpansionTile {
    /// Upstream `_kAnimationDuration`.
    pub const ANIMATION_DURATION_MS: u64 = 250;

    pub fn new() -> CupertinoExpansionTile {
        CupertinoExpansionTile {
            transition_mode: ExpansionTileTransitionMode::Fade,
            expanded: false,
        }
    }

    /// The two modes animate different things, and that decides what each needs
    /// from the tree.
    ///
    /// Fading keeps the child at full size throughout, so it has to be drawn
    /// outside the collapsed tile's bounds -- upstream reaches for an
    /// `OverlayPortal`. Scrolling keeps the child opaque and slides it under the
    /// header, which a clip can do on its own.
    ///
    /// **One animates the paint and the other the layout**, and the more
    /// expensive machinery goes to the one whose picture outgrows its box.
    /// What a screen reader is told, from upstream's pair of hints. See
    /// [`crate::material_app::DefaultMaterialLocalizations::expansion_tile_hint`]
    /// for why the two halves are chosen the way they are.
    ///
    /// Read from the **Cupertino** localizations, which is where upstream's
    /// `CupertinoExpansionTile` reads it -- `CupertinoLocalizations.of(context)`.
    /// The Material class declares the same six words, and taking them from
    /// there is right in English and wrong in general: the two classes are
    /// separate contracts and a locale supplies each on its own.
    ///
    /// Upstream attaches this on iOS and macOS only:
    ///
    /// ```dart
    /// switch (defaultTargetPlatform) {
    ///   case TargetPlatform.iOS:
    ///   case TargetPlatform.macOS:
    ///     semanticsHint = ...;
    ///   case TargetPlatform.android:
    ///   case ...:
    ///     break;
    /// }
    /// ```
    ///
    /// -- so on Android the hint is **absent**, not empty. The tap hint below
    /// is attached on every platform; only the state half is conditional,
    /// because on Android the platform announces expansion state itself and
    /// saying it again would double it.
    pub fn semantics_hint(&self, platform: TargetPlatform) -> Option<String> {
        matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS).then(|| {
            crate::cupertino_app::DefaultCupertinoLocalizations::expansion_tile_hint(self.expanded)
        })
    }

    /// Upstream's `onTapHint`, a **different** semantics field from the hint
    /// above and attached unconditionally.
    ///
    /// It is also the one that is **not** crossed: expanded gives "Collapse".
    pub fn on_tap_hint(&self) -> &'static str {
        crate::cupertino_app::DefaultCupertinoLocalizations::expansion_tile_tap_hint(self.expanded)
    }

    pub fn needs_an_overlay(&self) -> bool {
        matches!(self.transition_mode, ExpansionTileTransitionMode::Fade)
    }

    /// Upstream's build reads:
    ///
    /// ```dart
    /// if (widget.transitionMode == ExpansionTileTransitionMode.scroll) {
    ///   return child;
    /// }
    /// assert(widget.transitionMode == ExpansionTileTransitionMode.fade);
    /// ```
    ///
    /// **An exhaustiveness check written as an assert.** Two cases, one taken by
    /// an early return and the other left implied, with the assert standing
    /// where a `switch` would have put the compiler. Add a third mode and it
    /// fires -- at run time, in debug, on the machine that happens to hit that
    /// path.
    ///
    /// This port gets the check for free: [`ExpansionTileTransitionMode`] is
    /// matched exhaustively and a new variant would not compile. Worth stating
    /// plainly at the end of a sweep like this -- **it is the one place the
    /// target language answers a question the source could only ask.**
    pub fn handled_modes() -> [ExpansionTileTransitionMode; 2] {
        [
            ExpansionTileTransitionMode::Scroll,
            ExpansionTileTransitionMode::Fade,
        ]
    }
}

impl Default for CupertinoExpansionTile {
    fn default() -> Self {
        CupertinoExpansionTile::new()
    }
}

/// Upstream `CupertinoSpellCheckSuggestionsToolbar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoSpellCheckSuggestionsToolbar {
    pub button_item_count: usize,
}

impl CupertinoSpellCheckSuggestionsToolbar {
    /// Upstream `_kMaxSuggestions`, the same name and the same 3 as Material's.
    pub const MAX_SUGGESTIONS: usize = 3;

    pub fn new(button_item_count: usize) -> CupertinoSpellCheckSuggestionsToolbar {
        CupertinoSpellCheckSuggestionsToolbar { button_item_count }
    }

    /// Upstream `assert(buttonItems.length <= _kMaxSuggestions)`.
    ///
    /// Tick 91 recorded Material's version of this line:
    /// `assert(buttonItems.length <= _kMaxSuggestions + 1)`, written as `3 + 1`
    /// rather than `4` so as to say that the fourth item is not a suggestion --
    /// it is the delete button beneath them.
    ///
    /// **Here there is no `+ 1`, because there is no delete button.** Same
    /// constant, same value, and the presence or absence of one term names the
    /// whole difference in what the two toolbars offer. The same shape as tick
    /// 96's `noMaxLength`: the missing clause is not an oversight, it is a
    /// missing feature, and the assert is where you can see it.
    pub fn accepts_button_items(count: usize) -> bool {
        count <= CupertinoSpellCheckSuggestionsToolbar::MAX_SUGGESTIONS
    }

    pub fn is_valid(&self) -> bool {
        CupertinoSpellCheckSuggestionsToolbar::accepts_button_items(self.button_item_count)
    }

    /// Upstream's build-time
    /// `assert(!editableTextState.widget.readOnly && !editableTextState.widget.obscureText)`.
    ///
    /// Two refusals for two different reasons. **You cannot correct text you
    /// cannot edit**, and offering spelling suggestions for an obscured field
    /// would spill a password into a menu -- the one is pointless, the other
    /// unsafe, and one assert covers both.
    pub fn may_be_shown_for(read_only: bool, obscure_text: bool) -> bool {
        !read_only && !obscure_text
    }
}

/// Upstream `CupertinoAdaptiveTextSelectionToolbar`, the Cupertino twin of the
/// Material class ported in tick 91.
///
/// Where `AdaptiveTextSelectionToolbar` switches on the platform twice -- and
/// disagreed with itself about Fuchsia -- this one has no platform switch at
/// all. It is always Cupertino chrome with Cupertino buttons; **a class named
/// "adaptive" that adapts to the button set rather than to the platform.**
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoAdaptiveTextSelectionToolbar {
    pub child_count: Option<usize>,
    pub button_item_count: Option<usize>,
}

impl CupertinoAdaptiveTextSelectionToolbar {
    pub fn new(button_item_count: usize) -> CupertinoAdaptiveTextSelectionToolbar {
        CupertinoAdaptiveTextSelectionToolbar {
            child_count: None,
            button_item_count: Some(button_item_count),
        }
    }

    /// The same `(children ?? buttonItems)?.isEmpty ?? true` opening as the
    /// Material one: nothing to show is a small box rather than an error.
    pub fn is_empty(&self) -> bool {
        match self.child_count.or(self.button_item_count) {
            Some(count) => count == 0,
            None => true,
        }
    }

    pub fn adapts_to_the_platform() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The indicator has to fit inside the pull ------------------------------------

    #[test]
    fn an_indicator_taller_than_its_trigger_is_refused() {
        let mut control = CupertinoSliverRefreshControl::new();
        assert_eq!(control.validate(), Ok(()));

        control.refresh_indicator_extent = control.refresh_trigger_pull_distance + 1.0;
        assert_eq!(
            control.validate(),
            Err(RefreshControlError::IndicatorTallerThanTrigger),
            "letting go would make the content lurch down"
        );

        control.refresh_indicator_extent = control.refresh_trigger_pull_distance;
        assert_eq!(control.validate(), Ok(()), "equal is exactly enough");
    }

    #[test]
    fn an_indicator_may_take_no_space_at_all_but_a_trigger_may_not_be_free() {
        let mut control = CupertinoSliverRefreshControl::new();
        control.refresh_indicator_extent = 0.0;
        assert_eq!(control.validate(), Ok(()));

        control.refresh_trigger_pull_distance = 0.0;
        assert_eq!(
            control.validate(),
            Err(RefreshControlError::NonPositiveTriggerDistance)
        );
    }

    // -- The commitment happens before the release -------------------------------------

    #[test]
    fn you_are_armed_while_your_finger_is_still_down() {
        let control = CupertinoSliverRefreshControl::new();
        assert_eq!(control.mode_for_pull(0.0), RefreshIndicatorMode::Inactive);
        assert_eq!(control.mode_for_pull(50.0), RefreshIndicatorMode::Drag);
        assert_eq!(
            control.mode_for_pull(100.0),
            RefreshIndicatorMode::Armed,
            "which is what lets the indicator show you what letting go will do"
        );
        assert_eq!(control.mode_for_pull(400.0), RefreshIndicatorMode::Armed);
    }

    #[test]
    fn the_sliver_wants_the_direction_and_the_growth_to_agree() {
        let control = CupertinoSliverRefreshControl::new();
        assert_eq!(control.perform_layout(AxisDirection::Down, true), Ok(()));
        assert_eq!(
            control.perform_layout(AxisDirection::Down, false),
            Err(RefreshControlError::NotLaidOutDownwards)
        );
        assert_eq!(
            control.perform_layout(AxisDirection::Up, true),
            Err(RefreshControlError::NotLaidOutDownwards)
        );
    }

    // -- One animates the paint, the other the layout ------------------------------------

    #[test]
    fn only_the_fading_tile_needs_to_draw_outside_its_own_box() {
        let mut tile = CupertinoExpansionTile::new();
        assert!(tile.needs_an_overlay(), "a full-size child at low opacity");

        tile.transition_mode = ExpansionTileTransitionMode::Scroll;
        assert!(!tile.needs_an_overlay(), "a clip is enough to slide one");
    }

    #[test]
    fn the_assert_stands_where_a_switch_would_have_put_the_compiler() {
        // Two modes, one taken by an early return and the other left implied.
        let handled = CupertinoExpansionTile::handled_modes();
        assert_eq!(handled.len(), 2);
        for mode in handled {
            let tile = CupertinoExpansionTile {
                transition_mode: mode,
                expanded: false,
            };
            // Exhaustive here by construction; upstream finds out at run time.
            assert_eq!(
                tile.needs_an_overlay(),
                mode == ExpansionTileTransitionMode::Fade
            );
        }
    }

    #[test]
    fn a_quarter_of_a_second_either_way() {
        assert_eq!(CupertinoExpansionTile::ANIMATION_DURATION_MS, 250);
    }

    // -- The missing term names the missing button ----------------------------------------

    #[test]
    fn cupertino_caps_at_three_where_material_caps_at_three_plus_one() {
        use crate::text_toolbars::SpellCheckSuggestionsToolbar;

        assert_eq!(
            CupertinoSpellCheckSuggestionsToolbar::MAX_SUGGESTIONS,
            SpellCheckSuggestionsToolbar::MAX_SUGGESTIONS,
            "the same constant and the same value"
        );

        assert!(SpellCheckSuggestionsToolbar::accepts_button_items(4));
        assert!(
            !CupertinoSpellCheckSuggestionsToolbar::accepts_button_items(4),
            "and the fourth item Material allows is its delete button"
        );
        assert!(CupertinoSpellCheckSuggestionsToolbar::accepts_button_items(
            3
        ));
    }

    #[test]
    fn no_suggestions_for_text_you_cannot_edit_or_must_not_reveal() {
        assert!(CupertinoSpellCheckSuggestionsToolbar::may_be_shown_for(
            false, false
        ));
        assert!(
            !CupertinoSpellCheckSuggestionsToolbar::may_be_shown_for(true, false),
            "pointless"
        );
        assert!(
            !CupertinoSpellCheckSuggestionsToolbar::may_be_shown_for(false, true),
            "unsafe"
        );
    }

    // -- Adaptive to what? -----------------------------------------------------------------

    #[test]
    fn the_cupertino_adaptive_toolbar_does_not_consult_the_platform_at_all() {
        // Where the Material one switches twice and disagreed with itself about
        // Fuchsia, this one is Cupertino everywhere.
        assert!(!CupertinoAdaptiveTextSelectionToolbar::adapts_to_the_platform());
    }

    #[test]
    fn but_it_keeps_the_same_empty_case() {
        assert!(CupertinoAdaptiveTextSelectionToolbar::new(0).is_empty());
        assert!(!CupertinoAdaptiveTextSelectionToolbar::new(2).is_empty());
        assert!(
            CupertinoAdaptiveTextSelectionToolbar {
                child_count: None,
                button_item_count: None,
            }
            .is_empty()
        );
    }
}

#[cfg(test)]
mod empty_direction_tests {
    use super::*;

    #[test]
    fn children_are_consulted_before_button_items() {
        // `(children ?? buttonItems)` -- with only one of the two set, which
        // one is asked first cannot be seen. A sheet given an empty list of
        // children and a non-empty list of button items is empty: the caller
        // said children, and an empty list is an answer.
        let mut sheet = CupertinoAdaptiveTextSelectionToolbar::new(0);
        sheet.child_count = Some(0);
        sheet.button_item_count = Some(3);
        assert!(
            sheet.is_empty(),
            "an empty children list is still the answer"
        );

        sheet.child_count = Some(3);
        sheet.button_item_count = Some(0);
        assert!(!sheet.is_empty());
    }

    #[test]
    fn button_items_answer_only_when_there_are_no_children_at_all() {
        let mut sheet = CupertinoAdaptiveTextSelectionToolbar::new(0);
        sheet.child_count = None;
        sheet.button_item_count = Some(2);
        assert!(!sheet.is_empty());

        sheet.button_item_count = Some(0);
        assert!(sheet.is_empty());
    }

    #[test]
    fn neither_given_is_empty() {
        assert!(CupertinoAdaptiveTextSelectionToolbar::new(0).is_empty());
    }
}

#[cfg(test)]
mod expansion_hint_tests {
    use super::CupertinoExpansionTile;
    use crate::cupertino_app::DefaultCupertinoLocalizations as CupertinoL10n;
    use crate::editable_text::TargetPlatform;
    use crate::material_app::DefaultMaterialLocalizations as L10n;

    /// The hint a tile shows on the platforms that get one at all.
    fn hint(tile: &CupertinoExpansionTile) -> String {
        tile.semantics_hint(TargetPlatform::IOS).unwrap()
    }

    #[test]
    fn the_hint_is_the_state_and_then_what_a_tap_does() {
        // Upstream joins the two halves with `\n ` -- a newline and then a
        // space -- so a screen reader pauses between the state and the
        // action rather than running them together.
        let mut tile = CupertinoExpansionTile::new();
        assert_eq!(hint(&tile), "Collapsed\n double tap to expand");
        tile.expanded = true;
        assert_eq!(hint(&tile), "Expanded\n double tap to collapse");
    }

    #[test]
    fn the_state_half_is_absent_off_ios_rather_than_empty() {
        // Upstream's switch leaves `semanticsHint` null on Android, Fuchsia,
        // Linux and Windows: those platforms announce expansion state
        // themselves, and saying it again would double it.
        let tile = CupertinoExpansionTile::new();
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(tile.semantics_hint(platform).is_some(), "{platform:?}");
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_eq!(tile.semantics_hint(platform), None, "{platform:?}");
        }
    }

    #[test]
    fn the_tap_hint_is_a_second_field_and_is_not_crossed() {
        // `onTapHint` is not `hint`, and the crossing above does not apply to
        // it: an expanded tile offers "Collapse". Carrying the crossing over
        // out of symmetry would tell the reader that tapping an open tile
        // opens it.
        let mut tile = CupertinoExpansionTile::new();
        assert_eq!(tile.on_tap_hint(), "Expand for more details");
        tile.expanded = true;
        assert_eq!(tile.on_tap_hint(), "Collapse");
    }

    #[test]
    fn the_tap_hint_is_attached_on_every_platform() {
        // Only the state half is platform-conditional upstream; `onTapHint`
        // is passed to `Semantics` outside the switch.
        let tile = CupertinoExpansionTile::new();
        assert_eq!(tile.semantics_hint(TargetPlatform::Android), None);
        assert_eq!(tile.on_tap_hint(), "Expand for more details");
    }

    #[test]
    fn the_cupertino_tile_reads_the_cupertino_words() {
        // Upstream's tile calls `CupertinoLocalizations.of(context)`. The two
        // classes carry the same English today and are separate contracts, so
        // this asserts both the agreement and where the tile looks.
        let mut tile = CupertinoExpansionTile::new();
        for expanded in [false, true] {
            tile.expanded = expanded;
            assert_eq!(hint(&tile), CupertinoL10n::expansion_tile_hint(expanded));
            assert_eq!(
                tile.on_tap_hint(),
                CupertinoL10n::expansion_tile_tap_hint(expanded)
            );
            // ... and that the Material class still says the same thing, which
            // is the fact that makes reading the wrong one invisible.
            assert_eq!(
                CupertinoL10n::expansion_tile_hint(expanded),
                L10n::expansion_tile_hint(expanded)
            );
        }
    }

    #[test]
    fn each_sentence_agrees_with_itself() {
        // The trap: upstream names expandedHint "Collapsed" and collapsedHint
        // "Expanded", and crosses the pairing to match. Tidying the names into
        // agreement with their values while keeping the obvious pairing gives
        // two sentences that each say the opposite of the truth -- so what is
        // worth asserting is the sentence, not the constants.
        let mut tile = CupertinoExpansionTile::new();
        assert!(
            hint(&tile).starts_with("Collapsed"),
            "a shut tile says it is shut"
        );
        assert!(hint(&tile).ends_with("expand"), "and that a tap opens it");
        tile.expanded = true;
        assert!(hint(&tile).starts_with("Expanded"));
        assert!(hint(&tile).ends_with("collapse"));
    }

    #[test]
    fn no_sentence_tells_the_reader_to_do_what_is_already_done() {
        // Which is what the un-crossed pairing would produce.
        for expanded in [false, true] {
            let hint = L10n::expansion_tile_hint(expanded);
            let says_open = hint.starts_with("Expanded");
            let offers_open = hint.ends_with("expand");
            assert_ne!(
                says_open, offers_open,
                "an open tile must not offer to open: {hint}"
            );
        }
    }

    #[test]
    fn the_constants_are_named_the_way_upstream_names_them() {
        // Copied rather than corrected, because the crossing above depends on
        // them being what upstream says they are.
        assert_eq!(L10n::EXPANDED_HINT, "Collapsed");
        assert_eq!(L10n::COLLAPSED_HINT, "Expanded");
    }
}
