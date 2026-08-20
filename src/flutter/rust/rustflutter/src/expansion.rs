//! Ports of `material/expansion_tile.dart` and `material/expand_icon.dart`.
//!
//! Something that opens, and the arrow that says so. The Material dressing over
//! [`crate::presence::Expansible`], with two decisions of its own worth having
//! written down.

use crate::scroll_plumbing::ScrollPlatform;

/// Upstream's `_kExpand`.
pub const EXPAND_DURATION_MS: u32 = 200;

/// Upstream's iOS announcement delay.
///
/// A full second, and it is a workaround with its issue number attached:
/// *"This is a workaround for VoiceOver interrupting semantic announcements on
/// iOS."*
///
/// **The announcement waits a second because the alternative is the reader
/// hearing nothing.** VoiceOver is still speaking about the tap when the tile
/// finishes opening, and an announcement sent into that gets cut off. Waiting
/// is worse than not waiting everywhere except where it is the difference
/// between being heard and not.
pub const IOS_ANNOUNCEMENT_DELAY_MS: u32 = 1000;

/// Upstream `ExpandIcon`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpandIcon {
    /// Whether the arrow points at the open state.
    pub is_expanded: bool,
    pub size: f32,
    /// Upstream disables the button when `onPressed` is null.
    pub has_on_pressed: bool,
}

impl ExpandIcon {
    pub const DEFAULT_SIZE: f32 = 24.0;

    pub fn new(is_expanded: bool) -> ExpandIcon {
        ExpandIcon {
            is_expanded,
            size: ExpandIcon::DEFAULT_SIZE,
            has_on_pressed: true,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.has_on_pressed
    }

    /// Upstream states the controlled-widget contract in one sentence:
    /// *"Rebuilding the widget with a different `isExpanded` value will trigger
    /// the animation, but will not trigger the `onPressed` callback."*
    ///
    /// **The animation follows the value; the callback reports the press.**
    /// Changing the value from outside turns the arrow without pretending
    /// somebody pressed it -- which is the difference between a control that
    /// can be driven and one that argues with whoever drives it.
    pub fn animates_on_value_change() -> bool {
        true
    }

    pub fn calls_back_on_value_change() -> bool {
        false
    }

    /// How far round the arrow is, as a half turn.
    pub fn turns(&self) -> f32 {
        if self.is_expanded { 0.5 } else { 0.0 }
    }
}

/// What the tile does with its children while shut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollapsedChildren {
    /// Upstream's default: **removed from the tree** and rebuilt on the way
    /// back. A list of fifty collapsed tiles then costs fifty headers rather
    /// than fifty pages.
    Discarded,
    /// Kept, for children whose state is expensive to rebuild or impossible to
    /// recover -- a half-filled form, a video partway through.
    Maintained,
}

/// Upstream `ExpansionTile`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpansionTile {
    pub is_expanded: bool,
    /// Upstream's `maintainState`, defaulting to **false**.
    pub maintain_state: bool,
    /// Whether a controller was supplied. Without one the tile makes its own
    /// and disposes it; with one it does neither.
    pub has_controller: bool,
}

impl ExpansionTile {
    pub fn new() -> ExpansionTile {
        ExpansionTile {
            is_expanded: false,
            maintain_state: false,
            has_controller: false,
        }
    }

    pub fn collapsed_children(&self) -> CollapsedChildren {
        if self.maintain_state {
            CollapsedChildren::Maintained
        } else {
            CollapsedChildren::Discarded
        }
    }

    /// Whether the tile disposes the controller when it goes. Only the one it
    /// built itself -- the same rule as the two-dimensional scrollable and the
    /// search anchor.
    pub fn disposes_controller(&self) -> bool {
        !self.has_controller
    }

    /// The announcement made when the tile opens or shuts.
    ///
    /// Upstream picks `collapsedHint` when the tile has just **expanded** and
    /// `expandedHint` when it has just shut. Read from the naming that is the
    /// action now available rather than the state just reached -- the tile
    /// tells the reader what another tap would do.
    pub fn state_hint(now_expanded: bool) -> &'static str {
        if now_expanded {
            "collapsedHint"
        } else {
            "expandedHint"
        }
    }

    /// How long to wait before announcing. Everywhere but iOS, not at all.
    pub fn announcement_delay_ms(platform: ScrollPlatform) -> u32 {
        match platform {
            ScrollPlatform::IOS => IOS_ANNOUNCEMENT_DELAY_MS,
            _ => 0,
        }
    }

    /// A pending announcement is cancelled before a new one is scheduled, so a
    /// tile opened and shut quickly says the second thing rather than both.
    pub fn cancels_pending_announcement() -> bool {
        true
    }
}

impl Default for ExpansionTile {
    fn default() -> Self {
        ExpansionTile::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The arrow ------------------------------------------------------------

    #[test]
    fn the_animation_follows_the_value_and_the_callback_reports_the_press() {
        // Which is the difference between a control that can be driven and one
        // that argues with whoever drives it.
        assert!(ExpandIcon::animates_on_value_change());
        assert!(!ExpandIcon::calls_back_on_value_change());
    }

    #[test]
    fn the_arrow_turns_half_a_circle() {
        assert_eq!(ExpandIcon::new(false).turns(), 0.0);
        assert_eq!(ExpandIcon::new(true).turns(), 0.5);
    }

    #[test]
    fn no_callback_disables_the_button() {
        let mut icon = ExpandIcon::new(false);
        assert!(icon.is_enabled());
        icon.has_on_pressed = false;
        assert!(!icon.is_enabled());
    }

    // -- What a shut tile keeps ------------------------------------------------

    #[test]
    fn a_collapsed_tile_throws_its_children_away_by_default() {
        // A list of fifty collapsed tiles then costs fifty headers rather than
        // fifty pages.
        assert_eq!(
            ExpansionTile::new().collapsed_children(),
            CollapsedChildren::Discarded
        );

        let mut kept = ExpansionTile::new();
        kept.maintain_state = true;
        assert_eq!(kept.collapsed_children(), CollapsedChildren::Maintained);
    }

    #[test]
    fn it_disposes_only_the_controller_it_built_itself() {
        // The same rule as the two-dimensional scrollable and the search
        // anchor.
        assert!(ExpansionTile::new().disposes_controller());

        let mut given = ExpansionTile::new();
        given.has_controller = true;
        assert!(!given.disposes_controller());
    }

    // -- The announcement ---------------------------------------------------------

    #[test]
    fn the_announcement_waits_a_second_on_ios_and_nowhere_else() {
        // VoiceOver is still speaking about the tap when the tile finishes
        // opening, and an announcement sent into that gets cut off. Waiting is
        // worse than not waiting everywhere except where it is the difference
        // between being heard and not.
        assert_eq!(
            ExpansionTile::announcement_delay_ms(ScrollPlatform::IOS),
            1000
        );
        for platform in [
            ScrollPlatform::Android,
            ScrollPlatform::MacOS,
            ScrollPlatform::Windows,
            ScrollPlatform::Linux,
            ScrollPlatform::Fuchsia,
        ] {
            assert_eq!(
                ExpansionTile::announcement_delay_ms(platform),
                0,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_delay_is_five_times_the_animation_it_is_waiting_out() {
        // Which is how you can tell it is not waiting for the animation.
        assert!(IOS_ANNOUNCEMENT_DELAY_MS > EXPAND_DURATION_MS * 4);
    }

    #[test]
    fn a_tile_opened_and_shut_quickly_says_the_second_thing_and_not_both() {
        assert!(ExpansionTile::cancels_pending_announcement());
    }

    #[test]
    fn the_hint_names_what_another_tap_would_do() {
        // Upstream picks collapsedHint when the tile has just expanded, and the
        // other way round.
        assert_eq!(ExpansionTile::state_hint(true), "collapsedHint");
        assert_eq!(ExpansionTile::state_hint(false), "expandedHint");
    }
}
