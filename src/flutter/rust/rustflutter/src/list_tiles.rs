//! Ports of `material/checkbox_list_tile.dart`, `material/radio_list_tile.dart`
//! and `material/switch_list_tile.dart`.
//!
//! A control with a label you can also tap, three times over. Reading the three
//! together turns up something none of them says on its own.

/// Upstream `ListTileControlAffinity`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListTileControlAffinity {
    /// Control on the leading edge, secondary widget on the trailing one.
    Leading,
    /// The other way round.
    Trailing,
    /// Documented as *"the fashion that is typical for the current platform"*.
    ///
    /// It is not that. See [`ListTileControlAffinity::resolve`].
    #[default]
    Platform,
}

/// Which control the tile is wrapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileControl {
    Checkbox,
    Radio,
    Switch,
}

/// Where the control and the secondary widget end up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSlots {
    pub control_is_leading: bool,
}

impl ListTileControlAffinity {
    /// Resolving `platform`, and this is the finding.
    ///
    /// The enum value is documented as platform-typical, and **no
    /// implementation looks at the platform.** What they look at is which
    /// control they are wrapping: `checkbox_list_tile.dart` and
    /// `switch_list_tile.dart` group `platform` with `trailing`, while
    /// `radio_list_tile.dart` groups it with `leading`.
    ///
    /// ```dart
    /// // checkbox_list_tile.dart and switch_list_tile.dart
    /// ListTileControlAffinity.trailing || ListTileControlAffinity.platform => (secondary, control),
    /// // radio_list_tile.dart
    /// ListTileControlAffinity.leading || ListTileControlAffinity.platform => (control, secondary),
    /// ```
    ///
    /// So the value is meaningful -- it means "wherever this kind of control
    /// conventionally goes", and a radio conventionally goes first while a
    /// checkbox or a switch goes last. **It is named after the wrong axis: it
    /// varies by control, not by platform.**
    ///
    /// Ported as it behaves, with the name upstream gave it.
    pub fn resolve(self, control: TileControl) -> TileSlots {
        let control_is_leading = match self {
            ListTileControlAffinity::Leading => true,
            ListTileControlAffinity::Trailing => false,
            ListTileControlAffinity::Platform => matches!(control, TileControl::Radio),
        };
        TileSlots { control_is_leading }
    }

    /// Whether resolving this value consults the platform. It never does.
    pub fn consults_the_platform(self) -> bool {
        false
    }
}

/// What the three tiles share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlListTile {
    pub control: TileControl,
    /// `None` is the indeterminate state, which only a tristate control may
    /// have -- the same rule as [`crate::toggleable::ToggleableStateMixin`].
    pub value: Option<bool>,
    pub tristate: bool,
    pub has_subtitle: bool,
    pub is_three_line: bool,
    /// `None` falls through to the list tile theme's, then to `Platform`.
    pub control_affinity: Option<ListTileControlAffinity>,
    pub has_secondary: bool,
}

impl ControlListTile {
    pub fn new(control: TileControl, value: Option<bool>) -> ControlListTile {
        ControlListTile {
            control,
            value,
            tristate: false,
            has_subtitle: false,
            is_three_line: false,
            control_affinity: None,
            has_secondary: false,
        }
    }

    /// Upstream's two constructor asserts.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.tristate && self.value.is_none() {
            return Err("only a tristate control may have a null value");
        }
        if self.is_three_line && !self.has_subtitle {
            // The third line has to come from somewhere.
            return Err("isThreeLine requires a subtitle");
        }
        Ok(())
    }

    /// Upstream's fallback chain: the widget's, then the list tile theme's,
    /// then `platform`.
    pub fn effective_affinity(
        &self,
        theme_affinity: Option<ListTileControlAffinity>,
    ) -> ListTileControlAffinity {
        self.control_affinity
            .or(theme_affinity)
            .unwrap_or(ListTileControlAffinity::Platform)
    }

    pub fn slots(&self, theme_affinity: Option<ListTileControlAffinity>) -> TileSlots {
        self.effective_affinity(theme_affinity)
            .resolve(self.control)
    }

    /// Upstream wraps the whole thing in `MergeSemantics`, which is what makes
    /// the tile **one** thing to a screen reader rather than a checkbox beside
    /// some unrelated text. It is also what makes tapping the label work: the
    /// label is not a second control, it is part of this one.
    pub fn merges_semantics() -> bool {
        true
    }
}

/// Upstream `CheckboxListTile`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckboxListTile(pub ControlListTile);

impl CheckboxListTile {
    pub fn new(value: bool) -> CheckboxListTile {
        CheckboxListTile(ControlListTile::new(TileControl::Checkbox, Some(value)))
    }

    /// Upstream's tristate constructor, the only way to a null value.
    pub fn tristate(value: Option<bool>) -> CheckboxListTile {
        let mut tile = ControlListTile::new(TileControl::Checkbox, value);
        tile.tristate = true;
        CheckboxListTile(tile)
    }
}

/// Upstream `RadioListTile`.
///
/// The one of the three whose control sits **first** by default, because a
/// column of radios reads as a list of choices and the marks want to line up
/// down the leading edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadioListTile(pub ControlListTile);

impl RadioListTile {
    pub fn new(selected: bool) -> RadioListTile {
        RadioListTile(ControlListTile::new(TileControl::Radio, Some(selected)))
    }
}

/// Upstream `SwitchListTile`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchListTile(pub ControlListTile);

impl SwitchListTile {
    pub fn new(value: bool) -> SwitchListTile {
        SwitchListTile(ControlListTile::new(TileControl::Switch, Some(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The finding ------------------------------------------------------------

    #[test]
    fn platform_affinity_varies_by_control_and_not_by_platform() {
        // The enum value is documented as platform-typical, and no
        // implementation looks at the platform. A radio goes first; a checkbox
        // and a switch go last.
        let platform = ListTileControlAffinity::Platform;
        assert!(platform.resolve(TileControl::Radio).control_is_leading);
        assert!(!platform.resolve(TileControl::Checkbox).control_is_leading);
        assert!(!platform.resolve(TileControl::Switch).control_is_leading);
        assert!(!platform.consults_the_platform());
    }

    #[test]
    fn the_two_explicit_values_do_not_vary_at_all() {
        for control in [
            TileControl::Checkbox,
            TileControl::Radio,
            TileControl::Switch,
        ] {
            assert!(
                ListTileControlAffinity::Leading
                    .resolve(control)
                    .control_is_leading,
                "{control:?}"
            );
            assert!(
                !ListTileControlAffinity::Trailing
                    .resolve(control)
                    .control_is_leading,
                "{control:?}"
            );
        }
    }

    #[test]
    fn a_column_of_radios_lines_its_marks_up_down_the_leading_edge() {
        // Which is why the radio tile is the odd one out.
        let radio = RadioListTile::new(true);
        let checkbox = CheckboxListTile::new(true);
        assert!(radio.0.slots(None).control_is_leading);
        assert!(!checkbox.0.slots(None).control_is_leading);
    }

    // -- The fallback chain --------------------------------------------------------

    #[test]
    fn the_widget_beats_the_theme_and_the_theme_beats_the_default() {
        let mut tile = ControlListTile::new(TileControl::Checkbox, Some(true));
        assert_eq!(
            tile.effective_affinity(None),
            ListTileControlAffinity::Platform
        );
        assert_eq!(
            tile.effective_affinity(Some(ListTileControlAffinity::Leading)),
            ListTileControlAffinity::Leading
        );

        tile.control_affinity = Some(ListTileControlAffinity::Trailing);
        assert_eq!(
            tile.effective_affinity(Some(ListTileControlAffinity::Leading)),
            ListTileControlAffinity::Trailing
        );
    }

    #[test]
    fn a_theme_can_move_every_control_in_a_list_at_once() {
        let checkbox = CheckboxListTile::new(true);
        assert!(
            checkbox
                .0
                .slots(Some(ListTileControlAffinity::Leading))
                .control_is_leading
        );
    }

    // -- What the constructors refuse -------------------------------------------------

    #[test]
    fn only_a_tristate_control_may_be_null() {
        // The same rule as the toggleable mixin it is built on.
        assert!(
            ControlListTile::new(TileControl::Checkbox, None)
                .validate()
                .is_err()
        );
        assert_eq!(
            CheckboxListTile::tristate(None).0.validate(),
            Ok(()),
            "and a tristate one may"
        );
        assert_eq!(CheckboxListTile::new(true).0.validate(), Ok(()));
    }

    #[test]
    fn the_third_line_has_to_come_from_somewhere() {
        let mut tile = ControlListTile::new(TileControl::Checkbox, Some(true));
        tile.is_three_line = true;
        assert!(tile.validate().is_err());

        tile.has_subtitle = true;
        assert_eq!(tile.validate(), Ok(()));
    }

    // -- One thing, not two ------------------------------------------------------------

    #[test]
    fn the_label_is_part_of_the_control_rather_than_next_to_it() {
        // MergeSemantics is what makes the tile one thing to a screen reader,
        // and it is also what makes tapping the label work.
        assert!(ControlListTile::merges_semantics());
    }

    #[test]
    fn a_tristate_checkbox_tile_keeps_its_indeterminate_value() {
        let tile = CheckboxListTile::tristate(None);
        assert!(tile.0.tristate);
        assert_eq!(tile.0.value, None);
    }
}
