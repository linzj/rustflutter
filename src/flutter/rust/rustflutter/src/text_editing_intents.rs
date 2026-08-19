//! Every keystroke a text field understands, as a value -- a port of
//! upstream's `widgets/text_editing_intents.dart`.
//!
//! A text field does not handle keys. It declares *intents* -- "delete a
//! character backwards", "extend the selection to the next word" -- and a
//! shortcut table maps keystrokes onto them. That indirection is what lets one
//! field behave like a macOS field on macOS and a Windows field on Windows
//! with the same code: the intents are identical and only the table differs.
//!
//! Two shapes carry almost all of it:
//!
//! * [`DirectionalTextEditingIntent`], which is any intent with a **forward**
//!   or backward sense -- and there are far more of those than of anything
//!   else, because a keyboard is mostly a pair of directions;
//! * [`DirectionalCaretMovementIntent`], which adds the three flags that say
//!   what happens to an existing selection when the caret moves.

use crate::services::text_boundary::TextRange;
use crate::services::text_input::{SelectionChangedCause, TextEditingValue};

/// Upstream `DoNothingAndStopPropagationTextIntent`.
///
/// Not the same as having no binding: this one **consumes** the keystroke, so
/// nothing above the field sees it either. A field that wants a key to do
/// nothing *here* while still reaching a shortcut above would leave it
/// unbound instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DoNothingAndStopPropagationTextIntent;

/// Upstream `DirectionalTextEditingIntent`: an intent with a direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalTextEditingIntent {
    pub forward: bool,
}

impl DirectionalTextEditingIntent {
    pub fn new(forward: bool) -> DirectionalTextEditingIntent {
        DirectionalTextEditingIntent { forward }
    }
}

/// Upstream `DirectionalCaretMovementIntent`: a direction, and what becomes of
/// the selection.
///
/// The three flags are the whole of the difference between a plain arrow key,
/// a shifted one, and the ones macOS and Windows disagree about:
///
/// * `collapse_selection` -- the arrow key with no shift, which throws the
///   selection away and leaves a caret;
/// * `collapse_at_reversal` -- shift-arrow that reverses direction. Upstream
///   asserts it is never set together with `collapse_selection`, and the two
///   really are contradictory: one says "no selection at all" and the other
///   says "collapse this selection when the reader turns back";
/// * `continues_at_wrap` -- whether moving past the end of a wrapped line goes
///   to the next visual line or stops. The two conventions differ here and it
///   is not an accident either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalCaretMovementIntent {
    pub forward: bool,
    pub collapse_selection: bool,
    pub collapse_at_reversal: bool,
    pub continues_at_wrap: bool,
}

impl DirectionalCaretMovementIntent {
    pub fn new(forward: bool, collapse_selection: bool) -> DirectionalCaretMovementIntent {
        DirectionalCaretMovementIntent {
            forward,
            collapse_selection,
            collapse_at_reversal: false,
            continues_at_wrap: false,
        }
    }

    pub fn with_collapse_at_reversal(mut self, collapse: bool) -> Self {
        self.collapse_at_reversal = collapse;
        self
    }

    pub fn with_continues_at_wrap(mut self, continues: bool) -> Self {
        self.continues_at_wrap = continues;
        self
    }

    /// Upstream's assertion, `!collapseSelection || !collapseAtReversal`.
    pub fn is_valid(&self) -> bool {
        !self.collapse_selection || !self.collapse_at_reversal
    }
}

/// Upstream `DeleteCharacterIntent`: backspace, or forward delete.
///
/// One *character* rather than one code unit, which is the difference between
/// deleting an emoji and leaving half of it behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteCharacterIntent {
    pub base: DirectionalTextEditingIntent,
}

/// Upstream `DeleteToNextWordBoundaryIntent`: control-backspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteToNextWordBoundaryIntent {
    pub base: DirectionalTextEditingIntent,
}

/// Upstream `DeleteToLineBreakIntent`: delete to the end of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteToLineBreakIntent {
    pub base: DirectionalTextEditingIntent,
}

/// Upstream `ScrollToDocumentBoundaryIntent`: control-home and control-end on
/// the platforms where those *scroll* rather than move the caret.
///
/// It is a plain directional intent rather than a caret movement one for
/// exactly that reason -- nothing about the selection changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollToDocumentBoundaryIntent {
    pub base: DirectionalTextEditingIntent,
}

// These are spelled out rather than generated, even though only their methods
// differ: a type name that exists only as a macro argument is invisible to
// `tools/coverage.py`, which reads declarations and is right not to expand
// macro calls. The macros below generate the methods.
macro_rules! directional_intent {
    ($name:ident) => {
        impl $name {
            pub fn new(forward: bool) -> $name {
                $name {
                    base: DirectionalTextEditingIntent::new(forward),
                }
            }

            pub fn forward(&self) -> bool {
                self.base.forward
            }
        }
    };
}

directional_intent!(DeleteCharacterIntent);
directional_intent!(DeleteToNextWordBoundaryIntent);
directional_intent!(DeleteToLineBreakIntent);
directional_intent!(ScrollToDocumentBoundaryIntent);

macro_rules! caret_movement_intent {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name {
            pub base: DirectionalCaretMovementIntent,
        }

        impl $name {
            pub fn forward(&self) -> bool {
                self.base.forward
            }

            pub fn collapse_selection(&self) -> bool {
                self.base.collapse_selection
            }
        }
    };
}

caret_movement_intent!(
    ExtendSelectionByCharacterIntent,
    "Upstream `ExtendSelectionByCharacterIntent`: the left and right arrows."
);
caret_movement_intent!(
    ExtendSelectionToNextWordBoundaryIntent,
    "Upstream `ExtendSelectionToNextWordBoundaryIntent`: control-arrow."
);
caret_movement_intent!(
    ExtendSelectionToNextWordBoundaryOrCaretLocationIntent,
    "Upstream `ExtendSelectionToNextWordBoundaryOrCaretLocationIntent`.\n\n\
     The macOS behaviour: option-shift-arrow reversing direction collapses to \
     where the caret was rather than carrying on past it. That is what \
     `collapseAtReversal` is for, and this intent is the one that sets it."
);
caret_movement_intent!(
    ExpandSelectionToDocumentBoundaryIntent,
    "Upstream `ExpandSelectionToDocumentBoundaryIntent`.\n\n\
     *Expand* rather than extend: the selection grows to the boundary and the \
     other end stays put, whichever end the reader is dragging."
);
caret_movement_intent!(
    ExpandSelectionToLineBreakIntent,
    "Upstream `ExpandSelectionToLineBreakIntent`."
);
caret_movement_intent!(
    ExtendSelectionToLineBreakIntent,
    "Upstream `ExtendSelectionToLineBreakIntent`: the only one that takes all \
     four parameters, because home and end differ between platforms in every \
     one of them."
);
caret_movement_intent!(
    ExtendSelectionVerticallyToAdjacentLineIntent,
    "Upstream `ExtendSelectionVerticallyToAdjacentLineIntent`: the up and down \
     arrows."
);
caret_movement_intent!(
    ExtendSelectionVerticallyToAdjacentPageIntent,
    "Upstream `ExtendSelectionVerticallyToAdjacentPageIntent`: page up and \
     page down."
);
caret_movement_intent!(
    ExtendSelectionToNextParagraphBoundaryIntent,
    "Upstream `ExtendSelectionToNextParagraphBoundaryIntent`."
);
caret_movement_intent!(
    ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent,
    "Upstream `ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent`, \
     the paragraph-sized twin of the word one above."
);
caret_movement_intent!(
    ExtendSelectionToDocumentBoundaryIntent,
    "Upstream `ExtendSelectionToDocumentBoundaryIntent`: control-shift-home \
     and its partner."
);

impl ExtendSelectionByCharacterIntent {
    pub fn new(forward: bool, collapse_selection: bool) -> ExtendSelectionByCharacterIntent {
        ExtendSelectionByCharacterIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }
}

impl ExtendSelectionToNextWordBoundaryIntent {
    pub fn new(forward: bool, collapse_selection: bool) -> ExtendSelectionToNextWordBoundaryIntent {
        ExtendSelectionToNextWordBoundaryIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }
}

impl ExtendSelectionToNextWordBoundaryOrCaretLocationIntent {
    /// Upstream passes `false, true` -- never collapsing the selection, always
    /// collapsing at a reversal.
    pub fn new(forward: bool) -> ExtendSelectionToNextWordBoundaryOrCaretLocationIntent {
        ExtendSelectionToNextWordBoundaryOrCaretLocationIntent {
            base: DirectionalCaretMovementIntent::new(forward, false)
                .with_collapse_at_reversal(true),
        }
    }
}

impl ExpandSelectionToDocumentBoundaryIntent {
    pub fn new(forward: bool) -> ExpandSelectionToDocumentBoundaryIntent {
        ExpandSelectionToDocumentBoundaryIntent {
            base: DirectionalCaretMovementIntent::new(forward, false),
        }
    }
}

impl ExpandSelectionToLineBreakIntent {
    pub fn new(forward: bool) -> ExpandSelectionToLineBreakIntent {
        ExpandSelectionToLineBreakIntent {
            base: DirectionalCaretMovementIntent::new(forward, false),
        }
    }
}

impl ExtendSelectionToLineBreakIntent {
    pub fn new(forward: bool, collapse_selection: bool) -> ExtendSelectionToLineBreakIntent {
        ExtendSelectionToLineBreakIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }

    pub fn with_collapse_at_reversal(mut self, collapse: bool) -> Self {
        self.base = self.base.with_collapse_at_reversal(collapse);
        self
    }

    pub fn with_continues_at_wrap(mut self, continues: bool) -> Self {
        self.base = self.base.with_continues_at_wrap(continues);
        self
    }

    pub fn is_valid(&self) -> bool {
        self.base.is_valid()
    }
}

impl ExtendSelectionVerticallyToAdjacentLineIntent {
    pub fn new(
        forward: bool,
        collapse_selection: bool,
    ) -> ExtendSelectionVerticallyToAdjacentLineIntent {
        ExtendSelectionVerticallyToAdjacentLineIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }
}

impl ExtendSelectionVerticallyToAdjacentPageIntent {
    pub fn new(
        forward: bool,
        collapse_selection: bool,
    ) -> ExtendSelectionVerticallyToAdjacentPageIntent {
        ExtendSelectionVerticallyToAdjacentPageIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }
}

impl ExtendSelectionToNextParagraphBoundaryIntent {
    pub fn new(
        forward: bool,
        collapse_selection: bool,
    ) -> ExtendSelectionToNextParagraphBoundaryIntent {
        ExtendSelectionToNextParagraphBoundaryIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }
}

impl ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent {
    pub fn new(forward: bool) -> ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent {
        ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent {
            base: DirectionalCaretMovementIntent::new(forward, false)
                .with_collapse_at_reversal(true),
        }
    }
}

impl ExtendSelectionToDocumentBoundaryIntent {
    pub fn new(forward: bool, collapse_selection: bool) -> ExtendSelectionToDocumentBoundaryIntent {
        ExtendSelectionToDocumentBoundaryIntent {
            base: DirectionalCaretMovementIntent::new(forward, collapse_selection),
        }
    }
}

/// Upstream `SelectAllTextIntent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectAllTextIntent {
    /// Every one of the undirected intents carries a cause, and it is not
    /// bookkeeping: a field tells its listeners *why* the selection moved, and
    /// a toolbar that appears on a long press but not on a keystroke needs the
    /// answer.
    pub cause: SelectionChangedCause,
}

impl SelectAllTextIntent {
    pub fn new(cause: SelectionChangedCause) -> SelectAllTextIntent {
        SelectAllTextIntent { cause }
    }
}

/// Upstream `CopySelectionTextIntent`.
///
/// **Cutting is copying with the selection collapsed afterwards**, and
/// upstream says so by giving the two one class with a private constructor.
/// The alternative -- a separate `CutSelectionTextIntent` -- would have had a
/// handler duplicating the copy before deleting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopySelectionTextIntent {
    pub cause: SelectionChangedCause,
    pub collapse_selection: bool,
}

impl CopySelectionTextIntent {
    /// Upstream's `CopySelectionTextIntent.copy`, a **constant** whose cause is
    /// `keyboard`: a copy changes nothing, so there is one of it rather than
    /// one per cause.
    pub const COPY: CopySelectionTextIntent = CopySelectionTextIntent {
        cause: SelectionChangedCause::Keyboard,
        collapse_selection: false,
    };

    /// Upstream's `CopySelectionTextIntent.cut`, which does take a cause --
    /// because a cut *does* change the text, and a listener is entitled to
    /// know whether the reader used the toolbar or the keyboard.
    pub fn cut(cause: SelectionChangedCause) -> CopySelectionTextIntent {
        CopySelectionTextIntent {
            cause,
            collapse_selection: true,
        }
    }

    pub fn is_cut(&self) -> bool {
        self.collapse_selection
    }
}

/// Upstream `PasteTextIntent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasteTextIntent {
    pub cause: SelectionChangedCause,
}

/// Upstream `RedoTextIntent`.
///
/// Distinct from the undo one rather than a directional pair, because redo is
/// not undo backwards -- it replays a recorded edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedoTextIntent {
    pub cause: SelectionChangedCause,
}

/// Upstream `UndoTextIntent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoTextIntent {
    pub cause: SelectionChangedCause,
}

macro_rules! caused_intent {
    ($name:ident) => {
        impl $name {
            pub fn new(cause: SelectionChangedCause) -> $name {
                $name { cause }
            }
        }
    };
}

caused_intent!(PasteTextIntent);
caused_intent!(RedoTextIntent);
caused_intent!(UndoTextIntent);

/// Upstream `ReplaceTextIntent`.
///
/// It carries the **current** value as well as the replacement, and that is
/// the interesting part: a handler that read the field's value itself could
/// act on a value the reader has since changed. Sending the value the intent
/// was built from makes the edit describe a state rather than assume one.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaceTextIntent {
    pub current_text_editing_value: TextEditingValue,
    pub replacement_text: String,
    pub replacement_range: TextRange,
    pub cause: SelectionChangedCause,
}

impl ReplaceTextIntent {
    pub fn new(
        current_text_editing_value: TextEditingValue,
        replacement_text: impl Into<String>,
        replacement_range: TextRange,
        cause: SelectionChangedCause,
    ) -> ReplaceTextIntent {
        ReplaceTextIntent {
            current_text_editing_value,
            replacement_text: replacement_text.into(),
            replacement_range,
            cause,
        }
    }
}

/// Upstream `UpdateSelectionIntent`, which carries the current value for the
/// same reason.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateSelectionIntent {
    pub current_text_editing_value: TextEditingValue,
    pub new_selection: TextRange,
    pub cause: SelectionChangedCause,
}

impl UpdateSelectionIntent {
    pub fn new(
        current_text_editing_value: TextEditingValue,
        new_selection: TextRange,
        cause: SelectionChangedCause,
    ) -> UpdateSelectionIntent {
        UpdateSelectionIntent {
            current_text_editing_value,
            new_selection,
            cause,
        }
    }
}

/// Upstream `TransposeCharactersIntent`: control-T on macOS, which swaps the
/// two characters either side of the caret.
///
/// It has no direction and no cause -- there is only one thing it can mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransposeCharactersIntent;

/// Upstream `EditableTextTapOutsideIntent`.
///
/// A tap outside a focused field is an *intent* rather than a callback so that
/// an application can change what it means. The default gives up focus, but a
/// form that wants a tap on its background to do nothing needs to be able to
/// say so without reimplementing the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditableTextTapOutsideIntent {
    /// The focus node of the field that was focused, so a handler knows which
    /// field it is being asked about.
    pub focus_node: u64,
}

impl EditableTextTapOutsideIntent {
    pub fn new(focus_node: u64) -> EditableTextTapOutsideIntent {
        EditableTextTapOutsideIntent { focus_node }
    }
}

/// Upstream `EditableTextTapUpOutsideIntent`.
///
/// The pair exists because the two are not interchangeable: some platforms
/// take focus away on the press and others on the release, and a field that
/// only heard about one could not follow the platform it is running on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditableTextTapUpOutsideIntent {
    pub focus_node: u64,
}

impl EditableTextTapUpOutsideIntent {
    pub fn new(focus_node: u64) -> EditableTextTapUpOutsideIntent {
        EditableTextTapUpOutsideIntent { focus_node }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doing_nothing_and_leaving_the_key_alone_are_different_things() {
        // DoNothingAndStopPropagation consumes the keystroke, so nothing above
        // the field sees it. A field wanting a key to do nothing here while
        // still reaching a shortcut above would leave it unbound instead.
        let stopping = DoNothingAndStopPropagationTextIntent;
        assert_eq!(stopping, DoNothingAndStopPropagationTextIntent);
    }

    #[test]
    fn a_plain_arrow_throws_the_selection_away_and_a_shifted_one_does_not() {
        // Which is the whole of collapseSelection.
        let plain = ExtendSelectionByCharacterIntent::new(true, true);
        assert!(plain.collapse_selection());
        assert!(plain.forward());

        let shifted = ExtendSelectionByCharacterIntent::new(true, false);
        assert!(!shifted.collapse_selection());
        assert_ne!(plain, shifted);
    }

    #[test]
    fn collapsing_the_selection_and_collapsing_at_a_reversal_contradict_each_other() {
        // One says "no selection at all", the other says "collapse this
        // selection when the reader turns back". Upstream asserts they are
        // never set together.
        let sound =
            DirectionalCaretMovementIntent::new(true, false).with_collapse_at_reversal(true);
        assert!(sound.is_valid());

        let contradictory =
            DirectionalCaretMovementIntent::new(true, true).with_collapse_at_reversal(true);
        assert!(!contradictory.is_valid());

        // And the plain combinations are fine.
        assert!(DirectionalCaretMovementIntent::new(true, true).is_valid());
        assert!(DirectionalCaretMovementIntent::new(false, false).is_valid());
    }

    #[test]
    fn the_macos_reversal_behaviour_is_what_collapse_at_reversal_exists_for() {
        // Option-shift-arrow reversing direction collapses to where the caret
        // was rather than carrying on past it, and this is the intent that
        // sets the flag.
        let word = ExtendSelectionToNextWordBoundaryOrCaretLocationIntent::new(true);
        assert!(!word.base.collapse_selection);
        assert!(word.base.collapse_at_reversal);
        assert!(word.base.is_valid());

        // Its paragraph-sized twin does the same.
        let paragraph = ExtendSelectionToNextParagraphBoundaryOrCaretLocationIntent::new(false);
        assert!(paragraph.base.collapse_at_reversal);
        assert!(!paragraph.forward());
    }

    #[test]
    fn expanding_never_collapses_because_the_other_end_stays_put() {
        // Expand rather than extend: the selection grows to the boundary and
        // the far end does not move, whichever end the reader is dragging.
        for expanding in [
            ExpandSelectionToDocumentBoundaryIntent::new(true).base,
            ExpandSelectionToLineBreakIntent::new(false).base,
        ] {
            assert!(!expanding.collapse_selection);
            assert!(!expanding.collapse_at_reversal);
        }
    }

    #[test]
    fn the_line_break_intent_is_the_one_that_takes_all_four_parameters() {
        // Home and end differ between platforms in every one of them.
        let full = ExtendSelectionToLineBreakIntent::new(true, false)
            .with_collapse_at_reversal(true)
            .with_continues_at_wrap(true);
        assert!(full.forward());
        assert!(!full.collapse_selection());
        assert!(full.base.collapse_at_reversal);
        assert!(full.base.continues_at_wrap);
        assert!(full.is_valid());

        // And the assertion still applies to it.
        let contradictory =
            ExtendSelectionToLineBreakIntent::new(true, true).with_collapse_at_reversal(true);
        assert!(!contradictory.is_valid());
    }

    #[test]
    fn scrolling_to_a_boundary_is_not_a_caret_movement_at_all() {
        // Control-home on the platforms where it scrolls rather than moves the
        // caret: it is a plain directional intent because nothing about the
        // selection changes.
        let scroll = ScrollToDocumentBoundaryIntent::new(true);
        assert!(scroll.forward());
        assert_eq!(
            scroll.base,
            DirectionalTextEditingIntent::new(true),
            "no selection flags to carry"
        );
    }

    #[test]
    fn the_three_deletions_differ_only_in_how_much_they_take() {
        let character = DeleteCharacterIntent::new(false);
        let word = DeleteToNextWordBoundaryIntent::new(false);
        let line = DeleteToLineBreakIntent::new(false);
        assert!(!character.forward() && !word.forward() && !line.forward());
        assert_eq!(character.base, word.base, "the same direction");
        assert_eq!(word.base, line.base);
    }

    #[test]
    fn cutting_is_copying_with_the_selection_collapsed_afterwards() {
        // Upstream says so by giving the two one class. A separate cut intent
        // would have had a handler duplicating the copy before deleting.
        let copy = CopySelectionTextIntent::COPY;
        assert!(!copy.is_cut());
        assert!(!copy.collapse_selection);

        let cut = CopySelectionTextIntent::cut(SelectionChangedCause::Toolbar);
        assert!(cut.is_cut());
        assert_eq!(cut.cause, SelectionChangedCause::Toolbar);
    }

    #[test]
    fn a_copy_is_a_constant_and_a_cut_is_not() {
        // A copy changes nothing, so there is one of it rather than one per
        // cause; a cut changes the text, and a listener is entitled to know
        // whether the reader used the toolbar or the keyboard.
        assert_eq!(
            CopySelectionTextIntent::COPY.cause,
            SelectionChangedCause::Keyboard
        );
        assert_ne!(
            CopySelectionTextIntent::cut(SelectionChangedCause::Keyboard),
            CopySelectionTextIntent::cut(SelectionChangedCause::Toolbar)
        );
    }

    #[test]
    fn every_undirected_intent_says_why_it_happened() {
        // A toolbar that appears on a long press but not on a keystroke needs
        // the answer.
        assert_eq!(
            SelectAllTextIntent::new(SelectionChangedCause::LongPress).cause,
            SelectionChangedCause::LongPress
        );
        assert_eq!(
            PasteTextIntent::new(SelectionChangedCause::Toolbar).cause,
            SelectionChangedCause::Toolbar
        );
        assert_eq!(
            UndoTextIntent::new(SelectionChangedCause::Keyboard).cause,
            SelectionChangedCause::Keyboard
        );
        assert_eq!(
            RedoTextIntent::new(SelectionChangedCause::Keyboard).cause,
            SelectionChangedCause::Keyboard
        );
    }

    #[test]
    fn an_edit_describes_a_state_rather_than_assuming_one() {
        // A handler reading the field's value itself could act on a value the
        // reader has since changed. Carrying the value the intent was built
        // from is what makes the edit unambiguous.
        let value = TextEditingValue {
            text: "hello".to_string(),
            selection_base: 5,
            selection_extent: 5,
            ..TextEditingValue::default()
        };
        let replace = ReplaceTextIntent::new(
            value.clone(),
            " world",
            TextRange::collapsed(5),
            SelectionChangedCause::Keyboard,
        );
        assert_eq!(replace.current_text_editing_value.text, "hello");
        assert_eq!(replace.replacement_text, " world");
        assert_eq!(replace.replacement_range, TextRange::collapsed(5));

        let update = UpdateSelectionIntent::new(
            value,
            TextRange::new(0, 5),
            SelectionChangedCause::DoubleTap,
        );
        assert_eq!(update.new_selection, TextRange::new(0, 5));
        assert_eq!(update.current_text_editing_value.text, "hello");
    }

    #[test]
    fn transposing_has_neither_a_direction_nor_a_cause() {
        // There is only one thing it can mean.
        assert_eq!(TransposeCharactersIntent, TransposeCharactersIntent);
    }

    #[test]
    fn the_two_tap_outside_intents_are_not_interchangeable() {
        // Some platforms take focus away on the press and others on the
        // release; a field that only heard about one could not follow the
        // platform it is running on.
        let down = EditableTextTapOutsideIntent::new(7);
        let up = EditableTextTapUpOutsideIntent::new(7);
        assert_eq!(down.focus_node, 7);
        assert_eq!(up.focus_node, 7);
        // Different types, so a handler binds one or the other deliberately.
        assert_eq!(down, EditableTextTapOutsideIntent::new(7));
        assert_ne!(up, EditableTextTapUpOutsideIntent::new(8));
    }

    #[test]
    fn a_direction_is_carried_all_the_way_down() {
        // Every directional intent is built from the same base, so a shortcut
        // table that flips a direction flips it everywhere.
        for forward in [true, false] {
            assert_eq!(DeleteCharacterIntent::new(forward).forward(), forward);
            assert_eq!(
                ExtendSelectionByCharacterIntent::new(forward, true).forward(),
                forward
            );
            assert_eq!(
                ExtendSelectionVerticallyToAdjacentLineIntent::new(forward, false).forward(),
                forward
            );
            assert_eq!(
                ExtendSelectionVerticallyToAdjacentPageIntent::new(forward, false).forward(),
                forward
            );
            assert_eq!(
                ExtendSelectionToDocumentBoundaryIntent::new(forward, true).forward(),
                forward
            );
            assert_eq!(
                ExtendSelectionToNextParagraphBoundaryIntent::new(forward, true).forward(),
                forward
            );
            assert_eq!(
                ExtendSelectionToNextWordBoundaryIntent::new(forward, true).forward(),
                forward
            );
        }
    }
}
