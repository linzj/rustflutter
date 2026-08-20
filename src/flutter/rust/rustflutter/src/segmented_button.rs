//! A port of `material/segmented_button.dart`.
//!
//! A row of options where the chosen ones stay lit. Where a
//! [`crate::radio_group::RadioGroup`] is one of many and a checkbox is many of
//! many, this is either, decided by a flag -- and the interesting part is what
//! that flag does to a press on something already chosen.

use std::collections::BTreeSet;

/// Upstream `ButtonSegment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonSegment {
    /// Must be unique across the button's segments -- it is the identity, not a
    /// position, so reordering the segments does not change what is selected.
    pub value: i32,
    pub has_icon: bool,
    pub has_label: bool,
    pub tooltip: Option<String>,
    pub enabled: bool,
}

impl ButtonSegment {
    pub fn new(value: i32) -> ButtonSegment {
        ButtonSegment {
            value,
            has_icon: false,
            has_label: true,
            tooltip: None,
            enabled: true,
        }
    }

    pub fn icon_only(value: i32) -> ButtonSegment {
        ButtonSegment {
            has_icon: true,
            has_label: false,
            ..ButtonSegment::new(value)
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Upstream asserts `icon != null || label != null`. A segment with
    /// neither is a button with nothing on it -- a target the reader can hit and
    /// cannot read.
    pub fn is_valid(&self) -> bool {
        self.has_icon || self.has_label
    }
}

/// Upstream `SegmentedButton`.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentedButton {
    pub segments: Vec<ButtonSegment>,
    pub selected: BTreeSet<i32>,
    pub multi_selection_enabled: bool,
    pub empty_selection_allowed: bool,
    pub enabled: bool,
}

impl SegmentedButton {
    pub fn new(segments: Vec<ButtonSegment>, selected: &[i32]) -> SegmentedButton {
        SegmentedButton {
            segments,
            selected: selected.iter().copied().collect(),
            multi_selection_enabled: false,
            empty_selection_allowed: false,
            enabled: true,
        }
    }

    pub fn multi_select(mut self) -> Self {
        self.multi_selection_enabled = true;
        self
    }

    pub fn allowing_empty(mut self) -> Self {
        self.empty_selection_allowed = true;
        self
    }

    /// Upstream's three constructor asserts, which between them say one thing:
    /// **the state it starts in has to be a state it could have reached.**
    ///
    /// A button that begins empty without allowing empty, or begins with two
    /// selected without allowing two, is in a position its own press handler
    /// would never have produced -- and every rule below would then be
    /// reasoning about something that cannot happen.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.segments.is_empty() {
            return Err("a segmented button with no segments is nothing");
        }
        if self.selected.is_empty() && !self.empty_selection_allowed {
            return Err("selected must not be empty unless emptySelectionAllowed");
        }
        if self.selected.len() >= 2 && !self.multi_selection_enabled {
            return Err("selected must have at most one unless multiSelectionEnabled");
        }
        Ok(())
    }
}

/// What a press produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PressOutcome {
    /// The button is disabled, or the press would have emptied a selection that
    /// may not be emptied.
    Ignored,
    /// The selection would not have changed, so no callback is made. Pressing
    /// the already-chosen segment of a single-select button lands here.
    Unchanged,
    Changed(BTreeSet<i32>),
}

/// Upstream `SegmentedButtonState`.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentedButtonState {
    pub widget: SegmentedButton,
}

impl SegmentedButtonState {
    pub fn new(widget: SegmentedButton) -> SegmentedButtonState {
        SegmentedButtonState { widget }
    }

    /// Upstream `_handleOnPressed`, whose four clauses each earn their place.
    ///
    /// The one worth keeping is `validChange`: **pressing the only selected
    /// segment does nothing unless the button allows an empty selection.** In
    /// single-select mode you cannot deselect by pressing again -- the same
    /// judgement as the radio group's space bar and the toggleable's tap
    /// cycle. A control that can be emptied by a stray press is worse than one
    /// that cannot.
    ///
    /// And `toggle` says that single-select *does* toggle, in exactly one case:
    /// the last selected segment, when emptying is allowed.
    pub fn press(&self, segment_value: i32) -> PressOutcome {
        if !self.widget.enabled {
            return PressOutcome::Ignored;
        }
        let only_selected_segment =
            self.widget.selected.len() == 1 && self.widget.selected.contains(&segment_value);
        let valid_change = self.widget.empty_selection_allowed || !only_selected_segment;
        if !valid_change {
            return PressOutcome::Ignored;
        }

        let toggle = self.widget.multi_selection_enabled
            || (self.widget.empty_selection_allowed && only_selected_segment);
        let updated: BTreeSet<i32> = if toggle {
            if self.widget.selected.contains(&segment_value) {
                self.widget
                    .selected
                    .iter()
                    .copied()
                    .filter(|value| *value != segment_value)
                    .collect()
            } else {
                let mut next = self.widget.selected.clone();
                next.insert(segment_value);
                next
            }
        } else {
            // Not toggling: the press replaces the selection outright.
            [segment_value].into_iter().collect()
        };

        // Upstream compares before calling: no callback when nothing moved.
        if updated == self.widget.selected {
            return PressOutcome::Unchanged;
        }
        PressOutcome::Changed(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments() -> Vec<ButtonSegment> {
        vec![
            ButtonSegment::new(0),
            ButtonSegment::new(1),
            ButtonSegment::new(2),
        ]
    }

    fn single(selected: &[i32]) -> SegmentedButtonState {
        SegmentedButtonState::new(SegmentedButton::new(segments(), selected))
    }

    fn multi(selected: &[i32]) -> SegmentedButtonState {
        SegmentedButtonState::new(SegmentedButton::new(segments(), selected).multi_select())
    }

    fn selection(values: &[i32]) -> BTreeSet<i32> {
        values.iter().copied().collect()
    }

    // -- What the constructor refuses -------------------------------------------

    #[test]
    fn the_state_it_starts_in_has_to_be_one_it_could_have_reached() {
        // A button beginning empty without allowing empty, or with two selected
        // without allowing two, is in a position its own press handler would
        // never have produced.
        assert_eq!(single(&[1]).widget.validate(), Ok(()));

        assert!(SegmentedButton::new(segments(), &[]).validate().is_err());
        assert!(
            SegmentedButton::new(segments(), &[0, 1])
                .validate()
                .is_err()
        );

        assert_eq!(
            SegmentedButton::new(segments(), &[])
                .allowing_empty()
                .validate(),
            Ok(())
        );
        assert_eq!(
            SegmentedButton::new(segments(), &[0, 1])
                .multi_select()
                .validate(),
            Ok(())
        );
    }

    #[test]
    fn a_button_with_no_segments_is_nothing() {
        assert!(
            SegmentedButton::new(vec![], &[])
                .allowing_empty()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn a_segment_with_neither_icon_nor_label_is_a_target_you_cannot_read() {
        assert!(ButtonSegment::new(0).is_valid());
        assert!(ButtonSegment::icon_only(0).is_valid());

        let blank = ButtonSegment {
            has_icon: false,
            has_label: false,
            ..ButtonSegment::new(0)
        };
        assert!(!blank.is_valid());
    }

    // -- Pressing ----------------------------------------------------------------

    #[test]
    fn in_single_select_you_cannot_deselect_by_pressing_again() {
        // The same judgement as the radio group's space bar and the
        // toggleable's tap cycle: a control that can be emptied by a stray
        // press is worse than one that cannot.
        assert_eq!(single(&[1]).press(1), PressOutcome::Ignored);
    }

    #[test]
    fn single_select_does_toggle_in_exactly_one_case() {
        // The last selected segment, when emptying is allowed.
        let emptiable =
            SegmentedButtonState::new(SegmentedButton::new(segments(), &[1]).allowing_empty());
        assert_eq!(emptiable.press(1), PressOutcome::Changed(selection(&[])));
    }

    #[test]
    fn pressing_another_segment_replaces_the_selection_outright() {
        assert_eq!(
            single(&[1]).press(2),
            PressOutcome::Changed(selection(&[2]))
        );
    }

    #[test]
    fn multi_select_adds_and_removes() {
        assert_eq!(
            multi(&[1]).press(2),
            PressOutcome::Changed(selection(&[1, 2]))
        );
        assert_eq!(
            multi(&[1, 2]).press(2),
            PressOutcome::Changed(selection(&[1]))
        );
    }

    #[test]
    fn multi_select_can_empty_itself_only_where_empty_is_allowed() {
        assert_eq!(multi(&[1]).press(1), PressOutcome::Ignored);

        let emptiable = SegmentedButtonState::new(
            SegmentedButton::new(segments(), &[1])
                .multi_select()
                .allowing_empty(),
        );
        assert_eq!(emptiable.press(1), PressOutcome::Changed(selection(&[])));
    }

    #[test]
    fn a_disabled_button_ignores_everything() {
        let mut widget = SegmentedButton::new(segments(), &[1]);
        widget.enabled = false;
        assert_eq!(
            SegmentedButtonState::new(widget).press(2),
            PressOutcome::Ignored
        );
    }

    #[test]
    fn the_set_equality_guard_is_unreachable_once_the_earlier_clauses_have_run() {
        // Worth stating rather than faking a case for. Follow the clauses:
        // without toggling, the press replaces the selection with exactly the
        // pressed segment, and that can only equal the old selection when the
        // pressed one was the only selected one -- which validChange already
        // refused unless emptying is allowed, and if it is allowed then toggle
        // is true. With toggling, adding or removing always changes the set.
        //
        // So `if (!setEquals(...))` never fires today. It is a guard against
        // the clauses above it changing, not against any input, and porting it
        // faithfully means porting an unreachable branch.
        let mut reached_unchanged = false;
        for selected in [vec![], vec![0], vec![1], vec![0, 1], vec![0, 1, 2]] {
            for empty in [false, true] {
                for multi in [false, true] {
                    if selected.is_empty() && !empty {
                        continue;
                    }
                    if selected.len() >= 2 && !multi {
                        continue;
                    }
                    let mut widget = SegmentedButton::new(segments(), &selected);
                    widget.empty_selection_allowed = empty;
                    widget.multi_selection_enabled = multi;
                    let state = SegmentedButtonState::new(widget);
                    for value in 0..3 {
                        if state.press(value) == PressOutcome::Unchanged {
                            reached_unchanged = true;
                        }
                    }
                }
            }
        }
        assert!(
            !reached_unchanged,
            "a reachable Unchanged would mean the reasoning above is wrong"
        );
    }

    #[test]
    fn the_value_is_the_identity_so_reordering_changes_nothing() {
        let forwards = SegmentedButtonState::new(SegmentedButton::new(segments(), &[1]));
        let mut backwards_segments = segments();
        backwards_segments.reverse();
        let backwards = SegmentedButtonState::new(SegmentedButton::new(backwards_segments, &[1]));
        assert_eq!(forwards.press(2), backwards.press(2));
    }
}
