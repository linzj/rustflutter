// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Who draws a text selection's handles, and who has none.
//!
//! Upstream's [`TextSelectionControls`] (`widgets/text_selection.dart`) is the
//! seam between an editable field and the platform's idea of what a selection
//! looks like: the two drag handles, where they anchor, and -- on the
//! deprecated path -- the toolbar and which of cut/copy/paste/select-all it
//! may offer.
//!
//! # Zero-size handles are the interesting case
//!
//! Three of the implementations here draw *nothing*, and each has its own
//! reason, which is why they are three classes and not one:
//!
//! * [`DesktopTextSelectionControls`] has no handles because a desktop has a
//!   mouse. Selection is dragged with the pointer, so a handle would be a
//!   touch affordance nobody needs and something extra to hit by accident.
//! * [`EmptyTextSelectionControls`] has none because the *field* wants none --
//!   a read-only label, say. It still answers every question, so a field can
//!   hold one without checking for null.
//! * [`TextSelectionHandleControls`] has none *of the toolbar*, and keeps the
//!   handles. It is upstream's migration seam: the toolbar moved to
//!   `contextMenuBuilder`, so this refuses every toolbar question while
//!   leaving the handles alone.
//!
//! # What is not ported
//!
//! `buildHandle` and `buildToolbar` return widgets built from a
//! `TextSelectionDelegate`, a `ClipboardStatusNotifier` and a list of
//! `TextSelectionPoint`s -- the overlay machinery of
//! `widgets/text_selection.dart`, which this crate does not have. What is here
//! is the part that decides *whether* and *where*: the handle sizes and
//! anchors, and the four `can*` rules. Those are the answers the rest is built
//! on, and they are the half that is testable without an overlay.

use crate::render::{Offset, Size};

/// Upstream `TextSelectionHandleType` (`rendering/selection.dart`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSelectionHandleType {
    /// To the left of the selection's end point.
    Left,
    /// To the right of it.
    Right,
    /// The two ends are at the same place -- an insertion point rather than a
    /// range, which is why it is a third kind and not one of the other two.
    Collapsed,
}

/// What a field can tell its controls about the state of its own text.
///
/// Upstream reads these off a `TextSelectionDelegate`, whose other members are
/// about editing rather than about the selection. Narrowed here to the five
/// facts the `can*` rules actually consult, so the rules can be tested without
/// an editable field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SelectionState {
    pub cut_enabled: bool,
    pub copy_enabled: bool,
    pub paste_enabled: bool,
    pub select_all_enabled: bool,
    /// Whether the selection has no extent -- a caret rather than a range.
    pub is_collapsed: bool,
    /// Whether there is any text at all.
    pub has_text: bool,
}

impl SelectionState {
    /// A field with everything permitted, a caret and some text -- the state
    /// a fresh editable is in.
    pub fn editable() -> SelectionState {
        SelectionState {
            cut_enabled: true,
            copy_enabled: true,
            paste_enabled: true,
            select_all_enabled: true,
            is_collapsed: true,
            has_text: true,
        }
    }

    pub fn with_selection(mut self) -> SelectionState {
        self.is_collapsed = false;
        self
    }
}

/// Upstream `TextSelectionControls`.
pub trait TextSelectionControls {
    /// Upstream's `getHandleSize`.
    fn handle_size(&self, text_line_height: f32) -> Size;

    /// Upstream's `getHandleAnchor`: where within the handle its point sits,
    /// so the handle can be placed by its tip rather than its corner.
    fn handle_anchor(&self, kind: TextSelectionHandleType, text_line_height: f32) -> Offset;

    /// Upstream's `canCut`: **not while the selection is collapsed**, because
    /// there is nothing to remove. The same rule as `canCopy`.
    fn can_cut(&self, state: SelectionState) -> bool {
        state.cut_enabled && !state.is_collapsed
    }

    fn can_copy(&self, state: SelectionState) -> bool {
        state.copy_enabled && !state.is_collapsed
    }

    /// Upstream's `canPaste`, which asks about **nothing but the flag**.
    /// Pasting replaces whatever is selected, including nothing, so a caret is
    /// as good a target as a range.
    fn can_paste(&self, state: SelectionState) -> bool {
        state.paste_enabled
    }

    /// Upstream's `canSelectAll`, and the one that surprises: it requires the
    /// selection to **be** collapsed.
    ///
    /// Select-all is offered only when nothing is selected yet. Once the
    /// reader has a range, offering it again would either do nothing (if the
    /// range is already everything) or throw their selection away for a
    /// command they can reach another way -- and a toolbar full of commands
    /// that undo the reader's own work is a worse toolbar than a short one.
    fn can_select_all(&self, state: SelectionState) -> bool {
        state.select_all_enabled && state.has_text && state.is_collapsed
    }

    /// Whether this implementation draws handles at all -- the question three
    /// of the four here answer "no" to, each for its own reason.
    fn draws_handles(&self, text_line_height: f32) -> bool {
        self.handle_size(text_line_height) != Size::ZERO
    }
}

/// Upstream `EmptyTextSelectionControls`: controls that draw nothing.
///
/// For a field that wants no selection UI at all. It still answers every
/// question rather than being absent, so a field can hold one without a null
/// check at every call -- which is the difference between "no handles" and "no
/// controls".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct EmptyTextSelectionControls;

impl TextSelectionControls for EmptyTextSelectionControls {
    fn handle_size(&self, _text_line_height: f32) -> Size {
        Size::ZERO
    }

    fn handle_anchor(&self, _kind: TextSelectionHandleType, _text_line_height: f32) -> Offset {
        Offset::ZERO
    }
}

/// Upstream `TextSelectionHandleControls`: handles, and no toolbar.
///
/// A mixin upstream, applied over another controls class -- so it is a
/// *wrapper* here, holding the controls whose handles it keeps. Rust has no
/// mixins, and the relationship really is "these controls, minus the toolbar".
///
/// It exists because upstream moved the toolbar to `contextMenuBuilder` and
/// needed the old classes to keep drawing handles while refusing to build a
/// toolbar. So every `can*` answers **false**: not "there is nothing to cut",
/// but "do not ask me about cutting, ask the context menu".
pub struct TextSelectionHandleControls<T> {
    pub handles: T,
}

impl<T: TextSelectionControls> TextSelectionHandleControls<T> {
    pub fn new(handles: T) -> TextSelectionHandleControls<T> {
        TextSelectionHandleControls { handles }
    }
}

impl<T: TextSelectionControls> TextSelectionControls for TextSelectionHandleControls<T> {
    fn handle_size(&self, text_line_height: f32) -> Size {
        self.handles.handle_size(text_line_height)
    }

    fn handle_anchor(&self, kind: TextSelectionHandleType, text_line_height: f32) -> Offset {
        self.handles.handle_anchor(kind, text_line_height)
    }

    fn can_cut(&self, _state: SelectionState) -> bool {
        false
    }

    fn can_copy(&self, _state: SelectionState) -> bool {
        false
    }

    fn can_paste(&self, _state: SelectionState) -> bool {
        false
    }

    fn can_select_all(&self, _state: SelectionState) -> bool {
        false
    }
}

/// Upstream `MaterialTextSelectionControls` (`material/text_selection.dart`):
/// the round handles a touch device drags a selection by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MaterialTextSelectionControls;

impl MaterialTextSelectionControls {
    /// Upstream's `_kHandleSize`: the handle is a 22-pixel square holding a
    /// circle with one square corner, which is what makes it point.
    pub const HANDLE_SIZE: f32 = 22.0;
}

impl TextSelectionControls for MaterialTextSelectionControls {
    fn handle_size(&self, _text_line_height: f32) -> Size {
        Size::new(
            MaterialTextSelectionControls::HANDLE_SIZE,
            MaterialTextSelectionControls::HANDLE_SIZE,
        )
    }

    /// Upstream's anchors, and the asymmetry is the point: a left handle
    /// anchors at its *right* edge and a right handle at its left, so each
    /// one's square corner meets the text and its round body hangs outside
    /// the selection. A collapsed handle anchors at its middle, because it
    /// marks a point rather than an edge.
    fn handle_anchor(&self, kind: TextSelectionHandleType, _text_line_height: f32) -> Offset {
        let size = MaterialTextSelectionControls::HANDLE_SIZE;
        match kind {
            TextSelectionHandleType::Left => Offset::new(size, size),
            TextSelectionHandleType::Right => Offset::new(0.0, 0.0),
            TextSelectionHandleType::Collapsed => Offset::new(size / 2.0, 0.0),
        }
    }
}

/// Upstream `MaterialTextSelectionHandleControls`, which upstream declares as
/// `MaterialTextSelectionControls with TextSelectionHandleControls`.
pub type MaterialTextSelectionHandleControls =
    TextSelectionHandleControls<MaterialTextSelectionControls>;

/// Upstream `DesktopTextSelectionControls` (`material/desktop_text_selection.dart`):
/// no handles at all.
///
/// Not an oversight and not the same as [`EmptyTextSelectionControls`]: a
/// desktop has a mouse, so a selection is dragged with the pointer directly.
/// A handle would be a touch affordance nobody needs and one more thing to
/// catch a stray click.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DesktopTextSelectionControls;

impl TextSelectionControls for DesktopTextSelectionControls {
    fn handle_size(&self, _text_line_height: f32) -> Size {
        Size::ZERO
    }

    fn handle_anchor(&self, _kind: TextSelectionHandleType, _text_line_height: f32) -> Offset {
        Offset::ZERO
    }
}

/// Upstream `DesktopTextSelectionToolbar`: the small card of commands a
/// right-click puts up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopTextSelectionToolbar {
    /// Where the toolbar points, in global coordinates.
    pub anchor: Offset,
}

impl DesktopTextSelectionToolbar {
    /// Upstream's `_kToolbarWidth`, with its comment kept: measured from a
    /// screenshot of TextEdit on macOS. A *fixed* width rather than one that
    /// fits its contents, so the menu does not change shape as commands come
    /// and go with the selection.
    pub const WIDTH: f32 = 222.0;
    /// Upstream's `_kToolbarScreenPadding`: how far the toolbar stays from
    /// the screen's edges.
    pub const SCREEN_PADDING: f32 = 8.0;
    /// Upstream's corner radius.
    pub const CORNER_RADIUS: f32 = 7.0;

    pub fn new(anchor: Offset) -> DesktopTextSelectionToolbar {
        DesktopTextSelectionToolbar { anchor }
    }

    /// Upstream's `localAdjustment`: the anchor is given in screen
    /// coordinates, and the toolbar is laid out inside padding, so the anchor
    /// has to move into that padded frame or the toolbar would point eight
    /// pixels off in each direction.
    pub fn local_anchor(&self, padding_above: f32) -> Offset {
        Offset::new(
            self.anchor.dx - DesktopTextSelectionToolbar::SCREEN_PADDING,
            self.anchor.dy - (padding_above + DesktopTextSelectionToolbar::SCREEN_PADDING),
        )
    }
}

/// Upstream `DesktopTextSelectionToolbarButton`: one command in that card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopTextSelectionToolbarButton;

impl DesktopTextSelectionToolbarButton {
    /// Upstream's `_kToolbarButtonPadding`. The bottom 3 with no top is
    /// upstream's optical centring: the text sits above its box's middle
    /// because of the ascender, so the padding leans the other way.
    pub const PADDING: crate::render::EdgeInsets =
        crate::render::EdgeInsets::only(20.0, 0.0, 20.0, 3.0);
    /// Upstream's minimum size: a full interactive height, but only 36 wide
    /// before it stretches -- the button fills the toolbar's width anyway.
    pub const MIN_HEIGHT: f32 = 36.0;
    pub const FONT_SIZE: f32 = 14.0;
    /// Upstream's `letterSpacing`, which is *negative*: the menu is dense and
    /// the labels are tightened to fit a fixed width.
    pub const LETTER_SPACING: f32 = -0.15;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_can_be_cut_or_copied_while_the_selection_is_collapsed() {
        // There is nothing to remove, and nothing to put on the clipboard.
        let controls = MaterialTextSelectionControls;
        let caret = SelectionState::editable();
        assert!(!controls.can_cut(caret));
        assert!(!controls.can_copy(caret));

        let selected = SelectionState::editable().with_selection();
        assert!(controls.can_cut(selected));
        assert!(controls.can_copy(selected));
    }

    #[test]
    fn pasting_asks_about_nothing_but_the_flag() {
        // Pasting replaces whatever is selected, including nothing, so a caret
        // is as good a target as a range.
        let controls = MaterialTextSelectionControls;
        assert!(controls.can_paste(SelectionState::editable()));
        assert!(controls.can_paste(SelectionState::editable().with_selection()));
        assert!(!controls.can_paste(SelectionState {
            paste_enabled: false,
            ..SelectionState::editable()
        }));
    }

    #[test]
    fn select_all_is_offered_only_when_nothing_is_selected_yet() {
        // The rule that surprises. Once the reader has a range, offering
        // select-all again would either do nothing or throw their selection
        // away -- and a toolbar full of commands that undo the reader's own
        // work is worse than a short one.
        let controls = MaterialTextSelectionControls;
        assert!(controls.can_select_all(SelectionState::editable()));
        assert!(
            !controls.can_select_all(SelectionState::editable().with_selection()),
            "already has a range"
        );
        // And there has to be something to select.
        assert!(!controls.can_select_all(SelectionState {
            has_text: false,
            ..SelectionState::editable()
        }));
    }

    #[test]
    fn a_desktop_has_no_handles_because_it_has_a_mouse() {
        // Not an oversight: a selection is dragged with the pointer, so a
        // handle would be a touch affordance nobody needs and one more thing
        // to catch a stray click.
        let desktop = DesktopTextSelectionControls;
        assert_eq!(desktop.handle_size(16.0), Size::ZERO);
        assert!(!desktop.draws_handles(16.0));
        for kind in [
            TextSelectionHandleType::Left,
            TextSelectionHandleType::Right,
            TextSelectionHandleType::Collapsed,
        ] {
            assert_eq!(desktop.handle_anchor(kind, 16.0), Offset::ZERO);
        }
        // But it still answers the command questions, which a desktop toolbar
        // very much does ask.
        assert!(desktop.can_copy(SelectionState::editable().with_selection()));
    }

    #[test]
    fn a_touch_handle_anchors_so_its_square_corner_meets_the_text() {
        // The asymmetry is the point: a left handle anchors at its right edge
        // and a right handle at its left, so each one's body hangs *outside*
        // the selection rather than over the words being selected.
        let material = MaterialTextSelectionControls;
        let size = MaterialTextSelectionControls::HANDLE_SIZE;
        assert_eq!(
            material.handle_anchor(TextSelectionHandleType::Left, 16.0),
            Offset::new(size, size)
        );
        assert_eq!(
            material.handle_anchor(TextSelectionHandleType::Right, 16.0),
            Offset::ZERO
        );
        // A collapsed handle marks a point rather than an edge, so it is
        // centred.
        assert_eq!(
            material.handle_anchor(TextSelectionHandleType::Collapsed, 16.0),
            Offset::new(size / 2.0, 0.0)
        );
        assert!(material.draws_handles(16.0));
    }

    #[test]
    fn empty_controls_answer_every_question_rather_than_being_absent() {
        // Which is the difference between "no handles" and "no controls": a
        // field can hold one without a null check at every call.
        let empty = EmptyTextSelectionControls;
        assert!(!empty.draws_handles(16.0));
        // The `can*` defaults are inherited, so a field with empty controls
        // still knows a selection can be copied -- it simply draws nothing.
        assert!(empty.can_copy(SelectionState::editable().with_selection()));
    }

    #[test]
    fn handle_controls_keep_the_handles_and_refuse_every_toolbar_question() {
        // Upstream's migration seam: the toolbar moved to
        // `contextMenuBuilder`, so this refuses every command question while
        // leaving the handles alone. "False" here means "ask the context
        // menu", not "there is nothing to cut".
        let wrapped = MaterialTextSelectionHandleControls::new(MaterialTextSelectionControls);
        let selected = SelectionState::editable().with_selection();

        assert!(wrapped.draws_handles(16.0), "the handles are still drawn");
        assert_eq!(
            wrapped.handle_anchor(TextSelectionHandleType::Left, 16.0),
            MaterialTextSelectionControls.handle_anchor(TextSelectionHandleType::Left, 16.0),
            "and anchored exactly as the class it wraps"
        );

        assert!(!wrapped.can_cut(selected));
        assert!(!wrapped.can_copy(selected));
        assert!(!wrapped.can_paste(selected));
        assert!(!wrapped.can_select_all(SelectionState::editable()));
        // Where the class it wraps says yes to all of those it can.
        assert!(MaterialTextSelectionControls.can_cut(selected));
    }

    #[test]
    fn the_desktop_toolbar_moves_its_anchor_into_the_padded_frame() {
        // The anchor arrives in screen coordinates and the toolbar is laid out
        // inside padding, so without this it would point eight pixels off in
        // each direction.
        let toolbar = DesktopTextSelectionToolbar::new(Offset::new(100.0, 200.0));
        // No system inset: eight off each way.
        assert_eq!(toolbar.local_anchor(0.0), Offset::new(92.0, 192.0));
        // With a 24-pixel status bar the vertical shift includes it.
        assert_eq!(toolbar.local_anchor(24.0), Offset::new(92.0, 168.0));
    }

    #[test]
    fn the_desktop_toolbar_is_a_fixed_width() {
        // So the menu does not change shape as commands come and go with the
        // selection -- which they do, since cut and copy appear only once
        // there is a range.
        assert_eq!(DesktopTextSelectionToolbar::WIDTH, 222.0);
        assert_eq!(DesktopTextSelectionToolbar::SCREEN_PADDING, 8.0);
    }

    #[test]
    fn the_toolbar_buttons_padding_leans_downwards() {
        // 3 at the bottom and none at the top: optical centring, because text
        // sits above its box's middle on account of the ascender.
        let padding = DesktopTextSelectionToolbarButton::PADDING;
        assert_eq!(padding.top, 0.0);
        assert_eq!(padding.bottom, 3.0);
        assert_eq!((padding.left, padding.right), (20.0, 20.0));
        // And the labels are tightened, because the width is fixed.
        assert!(DesktopTextSelectionToolbarButton::LETTER_SPACING < 0.0);
    }
}
