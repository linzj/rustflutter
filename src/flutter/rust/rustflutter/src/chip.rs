//! Port of `material/chip.dart`'s `RawChip`.
//!
//! The chip every other chip is built out of. It implements all six of
//! upstream's chip attribute interfaces at once, which is why it is the one
//! with the flags that have to explain themselves.

/// Upstream `RawChip`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawChip {
    pub has_on_pressed: bool,
    pub has_on_selected: bool,
    pub has_on_deleted: bool,
    pub has_avatar: bool,
    pub selected: bool,
    pub is_enabled: bool,
    /// Not what the name suggests on its own. See [`RawChip::can_tap`].
    pub tap_enabled: bool,
    pub press_elevation: Option<f32>,
    pub elevation: Option<f32>,
}

/// Why a chip's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawChipError {
    NegativePressElevation,
    NegativeElevation,
}

impl RawChip {
    pub fn new() -> RawChip {
        RawChip {
            has_on_pressed: false,
            has_on_selected: false,
            has_on_deleted: false,
            has_avatar: false,
            selected: false,
            is_enabled: true,
            tap_enabled: true,
            press_elevation: None,
            elevation: None,
        }
    }

    /// Upstream's two constructor asserts, both nullable-shaped like the
    /// floating action button's.
    pub fn validate(&self) -> Result<(), RawChipError> {
        if self.press_elevation.is_some_and(|value| value < 0.0) {
            return Err(RawChipError::NegativePressElevation);
        }
        if self.elevation.is_some_and(|value| value < 0.0) {
            return Err(RawChipError::NegativeElevation);
        }
        Ok(())
    }

    /// Upstream's *"The `onPressed` and `onSelected` callbacks must not both be
    /// specified at the same time."*
    ///
    /// Worth noting where that rule lives. The two elevation asserts sit in the
    /// constructor; this one sits in **`initState`**:
    ///
    /// ```dart
    /// void initState() {
    ///   assert(widget.onSelected == null || widget.onPressed == null);
    /// ```
    ///
    /// Which is a later and rarer moment than it looks. A constructor assert
    /// fires when the widget is *made*; an `initState` assert fires only when
    /// the element is *mounted*. **A chip built into a list and never inserted
    /// into the tree never trips it.**
    ///
    /// And underneath, the tap handler does not choose between them:
    ///
    /// ```dart
    /// widget.onSelected?.call(!widget.selected);
    /// widget.onPressed?.call();
    /// ```
    ///
    /// **Two unconditional lines. If both are given, both fire** -- which is
    /// exactly what happens in a release build, where the assert is gone. The
    /// code underneath tolerates the state the assert forbids.
    pub fn callbacks_are_exclusive(&self) -> bool {
        !(self.has_on_pressed && self.has_on_selected)
    }

    /// What a tap actually invokes, as upstream's `_handleTap` does it: both
    /// lines, in order, each guarded only by its own presence.
    pub fn tap(&self) -> (Option<bool>, bool) {
        let selected = self.has_on_selected.then_some(!self.selected);
        (selected, self.has_on_pressed)
    }

    /// Upstream `canTap`, whose three terms are three separate ways for a chip
    /// to be untappable.
    ///
    /// The middle one is the interesting one. `tapEnabled` sounds like it means
    /// "taps work", and its doc explains something else:
    ///
    /// > If set, this indicates that the chip should be **disabled if all of the
    /// > tap callbacks are null**. For example, the `Chip` class sets this to
    /// > false because it **can't be disabled**, even if no callbacks are set on
    /// > it, since it is used for displaying information only.
    ///
    /// So the flag is named for its mechanism and documented for its purpose,
    /// and the purpose is about **appearance**: a plain `Chip` carries no
    /// callbacks and must not therefore look greyed out. Turning `tapEnabled`
    /// off is how it declines to have its enabledness inferred.
    ///
    /// Third of a family this week -- `indexIsChanging` named for its cause,
    /// `ListTileControlAffinity.platform` named for the wrong axis, and now a
    /// flag whose name describes the lever and whose doc describes the reason
    /// for pulling it.
    pub fn can_tap(&self) -> bool {
        self.is_enabled && self.tap_enabled && (self.has_on_pressed || self.has_on_selected)
    }

    /// Whether the chip should be *drawn* as disabled, which is the thing
    /// `tap_enabled` is really guarding.
    pub fn looks_disabled(&self) -> bool {
        if !self.is_enabled {
            return true;
        }
        // A chip that opted out of the inference never looks disabled, however
        // few callbacks it has.
        self.tap_enabled && !self.has_on_pressed && !self.has_on_selected
    }

    /// Upstream `hasDeleteButton`. The delete affordance is not a mode, it is
    /// the presence of a callback.
    pub fn has_delete_button(&self) -> bool {
        self.has_on_deleted
    }

    /// A plain `Chip`, which is where the flag's doc comes from.
    pub fn plain_chip() -> RawChip {
        RawChip {
            tap_enabled: false,
            ..RawChip::new()
        }
    }
}

impl Default for RawChip {
    fn default() -> Self {
        RawChip::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressable() -> RawChip {
        RawChip {
            has_on_pressed: true,
            ..RawChip::new()
        }
    }

    // -- The flag whose name and doc describe different things ------------------

    #[test]
    fn a_plain_chip_carries_no_callbacks_and_must_not_look_greyed_out() {
        let plain = RawChip::plain_chip();
        assert!(!plain.can_tap(), "nothing happens when you press it");
        assert!(
            !plain.looks_disabled(),
            "but it is a label, not a dead button"
        );
    }

    #[test]
    fn the_same_chip_with_the_inference_left_on_does_look_disabled() {
        // Which is the whole difference tapEnabled makes.
        let inferring = RawChip::new();
        assert!(inferring.tap_enabled);
        assert!(!inferring.can_tap());
        assert!(inferring.looks_disabled());
    }

    #[test]
    fn an_explicitly_disabled_chip_looks_disabled_however_the_flag_is_set() {
        let mut chip = RawChip::plain_chip();
        chip.is_enabled = false;
        assert!(chip.looks_disabled());
    }

    #[test]
    fn there_are_three_separate_ways_to_be_untappable() {
        let mut chip = pressable();
        assert!(chip.can_tap());

        chip.is_enabled = false;
        assert!(!chip.can_tap(), "disabled");
        chip.is_enabled = true;

        chip.tap_enabled = false;
        assert!(!chip.can_tap(), "opted out");
        chip.tap_enabled = true;

        chip.has_on_pressed = false;
        assert!(!chip.can_tap(), "nothing to do");
    }

    // -- The rule the code underneath does not need ------------------------------

    #[test]
    fn both_callbacks_at_once_is_forbidden_by_an_assert_and_by_nothing_else() {
        let mut chip = pressable();
        chip.has_on_selected = true;
        assert!(!chip.callbacks_are_exclusive(), "the assert would fire");

        // And in a release build, where it would not, the tap handler runs both
        // lines rather than choosing.
        let (selected, pressed) = chip.tap();
        assert_eq!(selected, Some(true));
        assert!(pressed);
    }

    #[test]
    fn a_selectable_chip_reports_the_value_it_is_moving_to() {
        let mut chip = RawChip::new();
        chip.has_on_selected = true;
        assert_eq!(chip.tap(), (Some(true), false));

        chip.selected = true;
        assert_eq!(chip.tap(), (Some(false), false), "and back again");
    }

    #[test]
    fn a_pressable_chip_reports_nothing_about_selection() {
        assert_eq!(pressable().tap(), (None, true));
    }

    #[test]
    fn one_callback_each_is_what_the_assert_wants() {
        assert!(pressable().callbacks_are_exclusive());
        let mut selectable = RawChip::new();
        selectable.has_on_selected = true;
        assert!(selectable.callbacks_are_exclusive());
        assert!(
            RawChip::new().callbacks_are_exclusive(),
            "and neither is allowed too -- it is an exclusion, not a requirement"
        );
    }

    // -- What the constructor refuses ---------------------------------------------

    #[test]
    fn an_elevation_may_be_unset_or_zero_but_not_negative() {
        let mut chip = RawChip::new();
        assert_eq!(chip.validate(), Ok(()));

        chip.elevation = Some(0.0);
        assert_eq!(chip.validate(), Ok(()));
        chip.elevation = Some(-0.5);
        assert_eq!(chip.validate(), Err(RawChipError::NegativeElevation));

        chip.elevation = None;
        chip.press_elevation = Some(-1.0);
        assert_eq!(chip.validate(), Err(RawChipError::NegativePressElevation));
    }

    #[test]
    fn the_delete_button_is_the_callback_rather_than_a_mode() {
        let mut chip = RawChip::new();
        assert!(!chip.has_delete_button());
        chip.has_on_deleted = true;
        assert!(chip.has_delete_button());
    }
}
