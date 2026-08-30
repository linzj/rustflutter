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
//! # What is here and what is next door
//!
//! `buildHandle` and `buildToolbar` return widgets built from a
//! `TextSelectionDelegate`, a `ClipboardStatusNotifier` and a list of
//! `TextSelectionPoint`s. What is here is the part that decides *whether* and
//! *where*: the handle sizes and anchors, and the four `can*` rules. Those are
//! the answers the rest is built on, and they are the half that is testable
//! without an overlay.
//!
//! The overlay machinery those answers feed is [`crate::selection_host`],
//! which puts the handles and the toolbar on the screen. This section used to
//! say it did not exist.

use crate::engine::{Color, Rect};
use crate::render::{EdgeInsets, Offset, Size};

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
    ///
    /// **Both y values are upstream's and neither is zero-by-symmetry.** The
    /// left handle's is `0`, not the handle size -- anchoring it a whole
    /// handle up would hang it over the line of text instead of below it --
    /// and the collapsed one's is **-4**, four pixels *above* the point, which
    /// is upstream's only magic number here and the thing that stops a caret's
    /// teardrop touching the glyph bottoms. Both were wrong until a handle was
    /// actually drawn with them.
    fn handle_anchor(&self, kind: TextSelectionHandleType, _text_line_height: f32) -> Offset {
        let size = MaterialTextSelectionControls::HANDLE_SIZE;
        match kind {
            TextSelectionHandleType::Left => Offset::new(size, 0.0),
            TextSelectionHandleType::Right => Offset::new(0.0, 0.0),
            TextSelectionHandleType::Collapsed => Offset::new(size / 2.0, -4.0),
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

// -- Where the selection toolbar goes -----------------------------------------

/// Upstream `TextSelectionToolbarAnchors` (`widgets/text_selection_toolbar_anchors.dart`):
/// the two places a selection toolbar may point at.
///
/// **Two anchors rather than one, because the toolbar has two homes.** It goes
/// above the selection when there is room and below it when there is not, and
/// which of those happens is decided at layout by whoever draws it -- so the
/// anchors have to carry both possibilities rather than a choice already made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionToolbarAnchors {
    /// Above the selection: horizontally centred on it, at its top.
    pub primary_anchor: Offset,
    /// Below it. `None` for a selection with no rectangle at all, where there
    /// is nothing to be below.
    pub secondary_anchor: Option<Offset>,
}

impl TextSelectionToolbarAnchors {
    pub fn new(primary_anchor: Offset) -> TextSelectionToolbarAnchors {
        TextSelectionToolbarAnchors {
            primary_anchor,
            secondary_anchor: None,
        }
    }

    /// Upstream's `TextSelectionToolbarAnchors.fromSelection`.
    ///
    /// Both anchors are **clamped into the editing region**, and that is the
    /// part worth keeping: a selection scrolled half out of a field would
    /// otherwise put the toolbar somewhere the field is not, pointing at text
    /// the reader cannot see. Clamped, the toolbar stays against the field's
    /// edge and keeps pointing into it.
    ///
    /// An empty selection rectangle answers a zero anchor and no secondary,
    /// which is upstream's early return -- there is nothing to point at, so
    /// there is nothing to be above or below.
    pub fn from_selection(
        selection_rect: Rect,
        editing_region: Rect,
    ) -> TextSelectionToolbarAnchors {
        if selection_rect == Rect::ltrb(0.0, 0.0, 0.0, 0.0) {
            return TextSelectionToolbarAnchors::new(Offset::ZERO);
        }
        let centre_x = selection_rect.left + selection_rect.width() / 2.0;
        TextSelectionToolbarAnchors {
            primary_anchor: Offset::new(
                centre_x,
                selection_rect
                    .top
                    .clamp(editing_region.top, editing_region.bottom),
            ),
            secondary_anchor: Some(Offset::new(
                centre_x,
                selection_rect
                    .bottom
                    .clamp(editing_region.top, editing_region.bottom),
            )),
        }
    }
}

/// Upstream `TextSelectionToolbar` (`material/text_selection_toolbar.dart`):
/// the Android-style selection menu, and the geometry that decides where it
/// sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionToolbar {
    pub anchor_above: Offset,
    pub anchor_below: Offset,
}

impl TextSelectionToolbar {
    /// Upstream's `_kToolbarHeight`.
    pub const TOOLBAR_HEIGHT: f32 = 44.0;
    /// Upstream's `_kToolbarContentDistance`: the gap between the toolbar and
    /// the text it is about, when it sits *above*.
    pub const CONTENT_DISTANCE: f32 = 8.0;
    /// Upstream's `kHandleSize`.
    pub const HANDLE_SIZE: f32 = 22.0;
    /// Upstream's `kToolbarContentDistanceBelow`, written as
    /// `kHandleSize - 2` rather than as 20.
    ///
    /// The arithmetic is the point: below the selection there is a *drag
    /// handle* in the way, so the gap has to clear it rather than being the
    /// same 8 used above. Written as the subtraction so that a change to the
    /// handle's size carries.
    pub const CONTENT_DISTANCE_BELOW: f32 = TextSelectionToolbar::HANDLE_SIZE - 2.0;
    /// Upstream reads this from `CupertinoTextSelectionToolbar`, which is the
    /// same 8 both platforms keep from the screen's edge.
    pub const SCREEN_PADDING: f32 = 8.0;

    pub fn new(anchor_above: Offset, anchor_below: Offset) -> TextSelectionToolbar {
        TextSelectionToolbar {
            anchor_above,
            anchor_below,
        }
    }

    /// The upper anchor moved *up* by the content distance, so the toolbar
    /// does not sit against the text.
    pub fn padded_anchor_above(&self) -> Offset {
        Offset::new(
            self.anchor_above.dx,
            self.anchor_above.dy - TextSelectionToolbar::CONTENT_DISTANCE,
        )
    }

    /// The lower anchor moved *down* past the drag handle. See
    /// [`TextSelectionToolbar::CONTENT_DISTANCE_BELOW`].
    pub fn padded_anchor_below(&self) -> Offset {
        Offset::new(
            self.anchor_below.dx,
            self.anchor_below.dy + TextSelectionToolbar::CONTENT_DISTANCE_BELOW,
        )
    }

    /// Upstream's `fitsAbove`: whether there is room for the toolbar between
    /// the top of the selection and the top of the screen.
    ///
    /// The content distance is subtracted **twice** -- once in the padded
    /// anchor and once here. That is upstream's arithmetic, and it is not a
    /// slip: the first is the gap the toolbar leaves above the text, the
    /// second is the gap it leaves below the status bar, and a toolbar that
    /// touched either would look wrong.
    pub fn fits_above(&self, system_padding_top: f32) -> bool {
        let padding_above = system_padding_top + TextSelectionToolbar::SCREEN_PADDING;
        let available =
            self.padded_anchor_above().dy - TextSelectionToolbar::CONTENT_DISTANCE - padding_above;
        TextSelectionToolbar::TOOLBAR_HEIGHT <= available
    }
}

/// Where a button sits in the toolbar's row, which decides its padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSelectionToolbarItemPosition {
    First,
    Middle,
    Last,
    /// The only button there is, which takes the *end* padding on both sides
    /// -- it is simultaneously the first and the last.
    Only,
}

/// Upstream `TextSelectionToolbarTextButton`
/// (`material/text_selection_toolbar_text_button.dart`): one command in that
/// menu.
pub struct TextSelectionToolbarTextButton;

impl TextSelectionToolbarTextButton {
    /// Upstream's `_kMiddlePadding`, with its comment kept: eyeballed to match
    /// the native menu on a Pixel 2 running Android 10.
    pub const MIDDLE_PADDING: f32 = 9.5;
    /// Upstream's `_kEndPadding`.
    pub const END_PADDING: f32 = 14.5;

    /// Upstream's `_getPosition`.
    pub fn position(index: usize, total: usize) -> TextSelectionToolbarItemPosition {
        debug_assert!(total > 0 && index < total);
        if index == 0 {
            return if total == 1 {
                TextSelectionToolbarItemPosition::Only
            } else {
                TextSelectionToolbarItemPosition::First
            };
        }
        if index + 1 == total {
            return TextSelectionToolbarItemPosition::Last;
        }
        TextSelectionToolbarItemPosition::Middle
    }

    /// Upstream's `getPadding`: wider at the menu's two ends than between its
    /// buttons.
    ///
    /// The reason the two differ: **the gap between two buttons is shared**,
    /// so half from each gives the 19 that reads right, while the gap at an
    /// end is one button's alone and has to be the whole 14.5 by itself.
    /// Splitting the middle evenly is also what keeps the buttons' hit areas
    /// touching, with no dead strip between them.
    pub fn padding(index: usize, total: usize) -> EdgeInsets {
        let position = TextSelectionToolbarTextButton::position(index, total);
        let start = match position {
            TextSelectionToolbarItemPosition::First | TextSelectionToolbarItemPosition::Only => {
                TextSelectionToolbarTextButton::END_PADDING
            }
            _ => TextSelectionToolbarTextButton::MIDDLE_PADDING,
        };
        let end = match position {
            TextSelectionToolbarItemPosition::Last | TextSelectionToolbarItemPosition::Only => {
                TextSelectionToolbarTextButton::END_PADDING
            }
            _ => TextSelectionToolbarTextButton::MIDDLE_PADDING,
        };
        EdgeInsets::only(start, 0.0, end, 0.0)
    }
}

// -- Cupertino's side of the same seam ----------------------------------------

/// Upstream `CupertinoTextSelectionControls` (`cupertino/text_selection.dart`):
/// the iOS selection handles.
///
/// **A lollipop, not a square.** Where
/// [`crate::text_selection_controls::MaterialTextSelectionControls`] draws a
/// fixed 22-pixel square sitting under the line, this draws a knob on a stem
/// that runs the *height of the text line* -- so its size depends on the text
/// it is selecting, and a handle beside a large heading is taller than one
/// beside body copy.
///
/// The stem and the knob overlap by
/// [`CupertinoTextSelectionControls::HANDLE_OVERLAP`], which is why the height
/// is `textLineHeight + 2 * radius - overlap` rather than a plain sum: without
/// the overlap the two shapes would meet exactly and leave a hairline seam
/// where the anti-aliasing of each falls short of the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoTextSelectionControls;

impl CupertinoTextSelectionControls {
    /// Upstream's `_kSelectionHandleRadius`, taken from Apple's own design
    /// resources.
    pub const HANDLE_RADIUS: f32 = 6.0;
    /// Upstream's `_kSelectionHandleOverlap`: how far the stem runs into the
    /// knob, so the join has no seam.
    pub const HANDLE_OVERLAP: f32 = 1.5;
}

impl TextSelectionControls for CupertinoTextSelectionControls {
    fn handle_size(&self, text_line_height: f32) -> Size {
        Size::new(
            CupertinoTextSelectionControls::HANDLE_RADIUS * 2.0,
            text_line_height + CupertinoTextSelectionControls::HANDLE_RADIUS * 2.0
                - CupertinoTextSelectionControls::HANDLE_OVERLAP,
        )
    }

    /// Upstream's anchors, and each of the three has its own reason:
    ///
    /// * **Left**: the knob is at the *top*, and the anchor is all the way at
    ///   the bottom -- so the stem lies along the text and the knob sits above
    ///   the line's start, out of the way of the words.
    /// * **Right**: the handle is drawn flipped, so the anchor is near the top
    ///   of its knob. The `+ overlap` is the same seam-hiding fudge, applied
    ///   to the anchor rather than the shape.
    /// * **Collapsed**: centred on the line, because it marks a caret rather
    ///   than an edge and has no side to prefer.
    fn handle_anchor(&self, kind: TextSelectionHandleType, text_line_height: f32) -> Offset {
        let size = self.handle_size(text_line_height);
        let radius = CupertinoTextSelectionControls::HANDLE_RADIUS;
        let overlap = CupertinoTextSelectionControls::HANDLE_OVERLAP;
        match kind {
            TextSelectionHandleType::Left => Offset::new(size.width / 2.0, size.height),
            TextSelectionHandleType::Right => {
                Offset::new(size.width / 2.0, size.height - 2.0 * radius + overlap)
            }
            TextSelectionHandleType::Collapsed => Offset::new(
                size.width / 2.0,
                text_line_height + (size.height - text_line_height) / 2.0,
            ),
        }
    }
}

/// Upstream `CupertinoTextSelectionHandleControls`.
pub type CupertinoTextSelectionHandleControls =
    TextSelectionHandleControls<CupertinoTextSelectionControls>;

/// Upstream `CupertinoDesktopTextSelectionControls`
/// (`cupertino/desktop_text_selection.dart`): no handles, for the same reason
/// [`crate::text_selection_controls::DesktopTextSelectionControls`] has none.
///
/// A separate class from the Material one because the *toolbar* differs, not
/// the handles -- both answer `Size.zero` here, and it is the menu each puts
/// up that makes them two classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoDesktopTextSelectionControls;

impl TextSelectionControls for CupertinoDesktopTextSelectionControls {
    fn handle_size(&self, _text_line_height: f32) -> Size {
        Size::ZERO
    }

    fn handle_anchor(&self, _kind: TextSelectionHandleType, _text_line_height: f32) -> Offset {
        Offset::ZERO
    }
}

/// Upstream `CupertinoTextSelectionToolbar` (`cupertino/text_selection_toolbar.dart`):
/// the iOS selection menu, the one with the arrow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoTextSelectionToolbar {
    pub anchor_above: Offset,
    pub anchor_below: Offset,
}

impl CupertinoTextSelectionToolbar {
    /// Upstream's `kToolbarScreenPadding`, which the Material toolbar also
    /// reads -- the one constant the two platforms share.
    pub const SCREEN_PADDING: f32 = 8.0;
    /// Upstream's `_kToolbarContentDistance`.
    pub const CONTENT_DISTANCE: f32 = 8.0;
    pub const BORDER_RADIUS: f32 = 8.0;
    /// Upstream's `_kToolbarArrowSize`: the little triangle that points at the
    /// selection. Wider than it is tall, so it reads as a pointer rather than
    /// as a spike.
    pub const ARROW_WIDTH: f32 = 14.0;
    pub const ARROW_HEIGHT: f32 = 7.0;
    /// Upstream's `_kArrowScreenPadding`: how far the *arrow's tip* stays from
    /// the screen's sides.
    ///
    /// Much larger than the toolbar's own 8, and the difference is the point:
    /// the toolbar may reach nearly to the edge, but the arrow may not, or it
    /// would be drawn against the toolbar's own rounded corner and lose its
    /// point.
    pub const ARROW_SCREEN_PADDING: f32 = 26.0;
    /// Upstream's `_kToolbarTransitionDuration`, in microseconds.
    pub const TRANSITION_MICROS: i64 = 125_000;

    pub fn new(anchor_above: Offset, anchor_below: Offset) -> CupertinoTextSelectionToolbar {
        CupertinoTextSelectionToolbar {
            anchor_above,
            anchor_below,
        }
    }

    /// Upstream's two padded anchors.
    ///
    /// **Both move by the same 8, in opposite directions** -- unlike the
    /// Material toolbar, whose lower distance has to clear a drag handle. An
    /// iOS handle is a stem *along* the line rather than a knob below it, so
    /// there is nothing under the selection to clear.
    pub fn padded_anchors(&self, padding_above: f32) -> (Offset, Offset) {
        (
            Offset::new(
                self.anchor_above.dx,
                self.anchor_above.dy
                    - CupertinoTextSelectionToolbar::CONTENT_DISTANCE
                    - padding_above,
            ),
            Offset::new(
                self.anchor_below.dx,
                self.anchor_below.dy + CupertinoTextSelectionToolbar::CONTENT_DISTANCE
                    - padding_above,
            ),
        )
    }

    /// Where the arrow's tip may sit horizontally, given the screen's width.
    pub fn arrow_tip_range(screen_width: f32) -> (f32, f32) {
        (
            CupertinoTextSelectionToolbar::ARROW_SCREEN_PADDING,
            screen_width - CupertinoTextSelectionToolbar::ARROW_SCREEN_PADDING,
        )
    }
}

/// Upstream `CupertinoTextSelectionToolbarButton`.
pub struct CupertinoTextSelectionToolbarButton;

impl CupertinoTextSelectionToolbarButton {
    /// Upstream's `_kToolbarButtonPadding`: 18 vertical against 16 horizontal.
    ///
    /// **Taller than it is wide**, which is the opposite of the Material
    /// button's shape. An iOS selection menu is one row of words with dividers
    /// between them and no icons, so the height is what gives each word a
    /// target; a Material menu is a row of chips that already have their own.
    pub const PADDING: EdgeInsets = EdgeInsets::symmetric(16.0, 18.0);

    /// Upstream's `_kToolbarTextColor`: black on light, white on dark.
    pub const TEXT_COLOR: crate::cupertino::CupertinoDynamicColor =
        crate::cupertino::CupertinoDynamicColor::with_brightness(
            crate::cupertino::CupertinoColors::BLACK,
            crate::cupertino::CupertinoColors::WHITE,
        );

    /// Upstream's `_kToolbarPressedColor`: `0x10000000` on light,
    /// `0x10FFFFFF` on dark.
    ///
    /// **Six per cent of an alpha, and the colour under it is the toolbar's
    /// own.** It is a wash rather than a fill, so a pressed button reads as
    /// the same button darkened and not as a new surface.
    pub const PRESSED_COLOR: crate::cupertino::CupertinoDynamicColor =
        crate::cupertino::CupertinoDynamicColor::with_brightness(
            Color::argb(0x10, 0x00, 0x00, 0x00),
            Color::argb(0x10, 0xFF, 0xFF, 0xFF),
        );

    /// Upstream's `_kToolbarButtonFontStyle`.
    ///
    /// Upstream writes `inherit: false`, which is the part worth naming: the
    /// toolbar's text takes **nothing** from the ambient style. A selection
    /// menu floats over whatever the reader was reading, and inheriting from
    /// it would size the menu's own words by the paragraph underneath.
    pub const FONT_SIZE: f32 = 15.0;
    /// Negative -- the letters are pulled *together*, which is how iOS sets
    /// this row of short words.
    pub const LETTER_SPACING: f32 = -0.15;
    pub const FONT_WEIGHT: i32 = 400;

    /// Whether the button's own opacity fades while it is held down.
    ///
    /// **It does not.** Upstream passes `pressedOpacity: 1.0` to switch off
    /// the fade a `CupertinoButton` would otherwise do, and darkens the
    /// background instead: "There's no foreground fade on iOS toolbar
    /// anymore, just the background is darkened."
    ///
    /// The two are not interchangeable. Fading the foreground would take the
    /// *label* with it, so the word the reader is pressing would be the
    /// hardest one on the row to read at the moment they press it.
    pub const PRESSED_OPACITY: f32 = 1.0;

    /// The background while the button is held, and while it is not.
    ///
    /// Transparent at rest **and** transparent when disabled -- upstream
    /// passes `disabledColor: CupertinoColors.transparent` -- so a disabled
    /// button is distinguished only by its grey text. There is no third
    /// background.
    pub fn background(pressed: bool, dark: bool) -> Color {
        if !pressed {
            return crate::cupertino::CupertinoColors::TRANSPARENT;
        }
        let color = CupertinoTextSelectionToolbarButton::PRESSED_COLOR;
        if dark { color.dark_color } else { color.color }
    }

    /// The label's colour, which is the one thing that marks a disabled
    /// button: upstream chooses `CupertinoColors.inactiveGray` when
    /// `onPressed` is null.
    pub fn label_color(enabled: bool, dark: bool) -> Color {
        if !enabled {
            let grey = crate::cupertino::CupertinoColors::INACTIVE_GRAY;
            return if dark { grey.dark_color } else { grey.color };
        }
        let color = CupertinoTextSelectionToolbarButton::TEXT_COLOR;
        if dark { color.dark_color } else { color.color }
    }

    /// Whether the button listens for taps at all.
    ///
    /// # The button's own `onPressed` does not handle the tap
    ///
    /// Upstream builds a `CupertinoButton` with `onPressed` **only so that it
    /// enables and disables correctly** -- its own comment says so -- and then
    /// wraps it in a `GestureDetector` that does the work, because the press
    /// has to change a background colour rather than an opacity and only the
    /// outer detector can see `onTapDown`, `onTapUp` and `onTapCancel`
    /// separately.
    ///
    /// The wrapper is added **only when there is something to press**. A
    /// disabled button is returned bare, so it has no gesture arena entry at
    /// all -- it does not merely ignore taps, it never competes for them, and
    /// a scroll starting on top of a disabled menu button is not delayed by
    /// one.
    pub fn wraps_in_gesture_detector(enabled: bool) -> bool {
        enabled
    }

    /// Upstream's `SizedBox.square(dimension: 13.0)` around the live-text
    /// icon.
    ///
    /// **The one button whose content is not text.** Every other kind falls to
    /// the same `Text` widget; `liveTextInput` gets its own arm of upstream's
    /// switch and a drawn glyph instead. That joins up with the label table,
    /// where `liveTextInput` answers the empty string: it has no label
    /// **because it shows no label**, not because one was left out.
    pub const LIVE_TEXT_ICON_DIMENSION: f32 = 13.0;

    /// Upstream's `_LiveTextIconPainter` stroke: round caps, round joins,
    /// width 1, stroke rather than fill.
    pub const LIVE_TEXT_ICON_STROKE_WIDTH: f32 = 1.0;
}

/// Upstream `_LiveTextIconPainter`: the viewfinder-with-lines glyph on the
/// live-text button, drawn rather than looked up.
///
/// # One corner, drawn four times, with the canvas turned between
///
/// Upstream builds a single path -- an arm, a rounded right-angle, another arm
/// -- and then draws it four times, rotating the **canvas** by a quarter turn
/// each time rather than recomputing the path. The canvas is translated to the
/// centre first, so the rotation is about the middle of the square; `origin`
/// then walks back out to the top-left corner.
///
/// Doing it the other way -- writing out four corners -- is where the mistakes
/// live: the four have to agree about the arc's direction as well as its
/// place, and three of them are the first one read backwards in some axis.
pub struct LiveTextIconPainter;

impl LiveTextIconPainter {
    /// How far each arm of a corner reaches from the corner itself.
    pub const ARM: f32 = 3.5;
    /// The radius of the rounded turn between the two arms.
    pub const CORNER_RADIUS: f32 = 1.0;
    /// Upstream draws the corner path four times, a quarter turn apart.
    pub const CORNERS: usize = 4;

    /// The three lines of "text" inside the viewfinder, as (start, end) pairs
    /// in the centred coordinate space.
    ///
    /// **The last one is short.** Two run the full `-3..3` and the third stops
    /// at 1. That is a ragged last line of a paragraph, and it is what makes
    /// the glyph read as *text* rather than as three rules or an equals sign.
    /// It is also the detail a redraw from memory loses first.
    pub fn lines() -> [(Offset, Offset); 3] {
        [
            (Offset::new(-3.0, -3.0), Offset::new(3.0, -3.0)),
            (Offset::new(-3.0, 0.0), Offset::new(3.0, 0.0)),
            (Offset::new(-3.0, 3.0), Offset::new(1.0, 3.0)),
        ]
    }

    /// The corner path's four points, from the top-left `origin` of a square
    /// of `size`: down the left arm to the turn, round it, and out along the
    /// top arm.
    ///
    /// Answered as points rather than as a built path because a
    /// [`crate::painting::RenderPath`] is an engine allocation -- the same
    /// split [`crate::animated_icons`] makes, and for the same reason.
    pub fn corner(size: f32) -> (Offset, Offset, Offset, Offset) {
        let origin = Offset::new(-size / 2.0, -size / 2.0);
        (
            // The far end of the vertical arm.
            Offset::new(origin.dx, origin.dy + LiveTextIconPainter::ARM),
            // Where the turn begins.
            Offset::new(origin.dx, origin.dy + LiveTextIconPainter::CORNER_RADIUS),
            // Where it ends.
            Offset::new(origin.dx + LiveTextIconPainter::CORNER_RADIUS, origin.dy),
            // The far end of the horizontal arm.
            Offset::new(origin.dx + LiveTextIconPainter::ARM, origin.dy),
        )
    }
}

/// Upstream `CupertinoDesktopTextSelectionToolbar`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoDesktopTextSelectionToolbar {
    pub anchor: Offset,
}

impl CupertinoDesktopTextSelectionToolbar {
    /// Upstream's `_kToolbarWidth` -- the same 222 the Material desktop
    /// toolbar uses, since both were measured from the same macOS menu.
    pub const WIDTH: f32 = 222.0;
    pub const SCREEN_PADDING: f32 = 8.0;
    /// Upstream's `_kToolbarPadding`, around the whole menu.
    pub const PADDING: EdgeInsets = EdgeInsets::all(6.0);
    /// Upstream's `_kToolbarBlurSigma`: the menu is translucent and blurs what
    /// is behind it.
    pub const BLUR_SIGMA: f32 = 20.0;
    /// Upstream's `_kToolbarSaturationBoost`, which goes *with* the blur: a
    /// blur averages colours together and washes them out, so the saturation
    /// is pushed back up to keep what shows through recognisable. One without
    /// the other would look wrong.
    pub const SATURATION_BOOST: f32 = 3.0;

    pub fn new(anchor: Offset) -> CupertinoDesktopTextSelectionToolbar {
        CupertinoDesktopTextSelectionToolbar { anchor }
    }
}

/// Upstream `CupertinoDesktopTextSelectionToolbarButton`.
pub struct CupertinoDesktopTextSelectionToolbarButton;

impl CupertinoDesktopTextSelectionToolbarButton {
    /// Upstream's `_kToolbarButtonPadding`, `fromLTRB(8, 2, 8, 5)`.
    ///
    /// The same downward lean as the Material desktop button's -- optical
    /// centring for text, which sits above its box's middle -- but tighter
    /// all round, because a macOS menu row is denser than a Material one.
    pub const PADDING: EdgeInsets = EdgeInsets::only(8.0, 2.0, 8.0, 5.0);
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
        //
        // The **y** values are upstream's `getHandleAnchor` verbatim and were
        // both wrong here until a handle was drawn with them: this test used
        // to assert the left handle anchored a whole handle *up*, which hung
        // it over the line of text instead of below it, and that the collapsed
        // one anchored at zero rather than at upstream's -4.
        let material = MaterialTextSelectionControls;
        let size = MaterialTextSelectionControls::HANDLE_SIZE;
        assert_eq!(
            material.handle_anchor(TextSelectionHandleType::Left, 16.0),
            Offset::new(size, 0.0)
        );
        assert_eq!(
            material.handle_anchor(TextSelectionHandleType::Right, 16.0),
            Offset::ZERO
        );
        // A collapsed handle marks a point rather than an edge, so it is
        // centred -- and lifted four pixels, which is the one magic number in
        // upstream's switch.
        assert_eq!(
            material.handle_anchor(TextSelectionHandleType::Collapsed, 16.0),
            Offset::new(size / 2.0, -4.0)
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

    #[test]
    fn the_anchors_are_centred_on_the_selection_at_its_top_and_bottom() {
        // Two anchors rather than one, because the toolbar has two homes: it
        // goes above the selection when there is room and below it when there
        // is not, and which happens is decided at layout.
        let selection = Rect::ltrb(100.0, 200.0, 300.0, 220.0);
        let field = Rect::ltrb(0.0, 0.0, 400.0, 800.0);
        let anchors = TextSelectionToolbarAnchors::from_selection(selection, field);
        assert_eq!(anchors.primary_anchor, Offset::new(200.0, 200.0));
        assert_eq!(anchors.secondary_anchor, Some(Offset::new(200.0, 220.0)));
    }

    #[test]
    fn both_anchors_are_clamped_into_the_field() {
        // A selection scrolled half out of a field would otherwise put the
        // toolbar somewhere the field is not, pointing at text the reader
        // cannot see. Clamped, it stays against the field's edge and keeps
        // pointing into it.
        let field = Rect::ltrb(0.0, 100.0, 400.0, 300.0);

        let scrolled_off_the_top = Rect::ltrb(100.0, -50.0, 300.0, 20.0);
        let above = TextSelectionToolbarAnchors::from_selection(scrolled_off_the_top, field);
        assert_eq!(above.primary_anchor.dy, 100.0, "clamped to the field's top");
        assert_eq!(above.secondary_anchor.unwrap().dy, 100.0);

        let scrolled_off_the_bottom = Rect::ltrb(100.0, 500.0, 300.0, 560.0);
        let below = TextSelectionToolbarAnchors::from_selection(scrolled_off_the_bottom, field);
        assert_eq!(below.primary_anchor.dy, 300.0, "and to its bottom");
    }

    #[test]
    fn an_empty_selection_has_nothing_to_be_above_or_below() {
        // Upstream's early return. There is no rectangle to point at, so there
        // is no second anchor either.
        let anchors = TextSelectionToolbarAnchors::from_selection(
            Rect::ltrb(0.0, 0.0, 0.0, 0.0),
            Rect::ltrb(0.0, 0.0, 400.0, 800.0),
        );
        assert_eq!(anchors.primary_anchor, Offset::ZERO);
        assert_eq!(anchors.secondary_anchor, None);
    }

    #[test]
    fn the_gap_below_clears_the_drag_handle_and_the_gap_above_does_not_have_to() {
        // Upstream writes the lower distance as `kHandleSize - 2` rather than
        // as 20, because below the selection there is a handle in the way.
        // Above, there is only text, so 8 is enough.
        assert_eq!(TextSelectionToolbar::CONTENT_DISTANCE, 8.0);
        assert_eq!(TextSelectionToolbar::CONTENT_DISTANCE_BELOW, 20.0);
        assert_eq!(
            TextSelectionToolbar::CONTENT_DISTANCE_BELOW,
            TextSelectionToolbar::HANDLE_SIZE - 2.0,
            "written as the arithmetic so a change to the handle carries"
        );

        let toolbar = TextSelectionToolbar::new(Offset::new(50.0, 200.0), Offset::new(50.0, 220.0));
        assert_eq!(toolbar.padded_anchor_above(), Offset::new(50.0, 192.0));
        assert_eq!(toolbar.padded_anchor_below(), Offset::new(50.0, 240.0));
    }

    #[test]
    fn a_selection_near_the_top_of_the_screen_puts_the_toolbar_below_it() {
        // Which is the whole reason there are two anchors.
        let low = TextSelectionToolbar::new(Offset::new(50.0, 400.0), Offset::new(50.0, 420.0));
        assert!(low.fits_above(0.0), "plenty of room");

        let high = TextSelectionToolbar::new(Offset::new(50.0, 30.0), Offset::new(50.0, 50.0));
        assert!(!high.fits_above(0.0), "no room above");
    }

    #[test]
    fn the_status_bar_eats_into_the_room_above() {
        // A selection that fits above on a screen with no notch may not on one
        // with a tall status bar, and the toolbar has to know before it picks
        // an anchor.
        let toolbar = TextSelectionToolbar::new(Offset::new(50.0, 80.0), Offset::new(50.0, 100.0));
        assert!(toolbar.fits_above(0.0));
        assert!(!toolbar.fits_above(44.0), "the notch took the room");
    }

    #[test]
    fn a_lone_toolbar_button_is_both_the_first_and_the_last() {
        assert_eq!(
            TextSelectionToolbarTextButton::position(0, 1),
            TextSelectionToolbarItemPosition::Only
        );
        assert_eq!(
            TextSelectionToolbarTextButton::position(0, 3),
            TextSelectionToolbarItemPosition::First
        );
        assert_eq!(
            TextSelectionToolbarTextButton::position(1, 3),
            TextSelectionToolbarItemPosition::Middle
        );
        assert_eq!(
            TextSelectionToolbarTextButton::position(2, 3),
            TextSelectionToolbarItemPosition::Last
        );
    }

    #[test]
    fn the_padding_between_two_buttons_is_shared_and_the_padding_at_an_end_is_not() {
        // Half from each button gives the gap that reads right between them,
        // while the gap at an end is one button's alone and has to be the
        // whole thing by itself.
        let end = TextSelectionToolbarTextButton::END_PADDING;
        let middle = TextSelectionToolbarTextButton::MIDDLE_PADDING;

        let first = TextSelectionToolbarTextButton::padding(0, 3);
        assert_eq!((first.left, first.right), (end, middle));
        let inner = TextSelectionToolbarTextButton::padding(1, 3);
        assert_eq!((inner.left, inner.right), (middle, middle));
        let last = TextSelectionToolbarTextButton::padding(2, 3);
        assert_eq!((last.left, last.right), (middle, end));

        // A lone button takes the end padding on both sides.
        let only = TextSelectionToolbarTextButton::padding(0, 1);
        assert_eq!((only.left, only.right), (end, end));

        // And the shared gap comes to more than one end's, which is what
        // makes the row read as evenly spaced rather than crowded in the
        // middle.
        assert!(middle * 2.0 > end);
    }

    #[test]
    fn the_buttons_hit_areas_touch_with_no_dead_strip_between_them() {
        // Splitting the middle gap evenly is what does it: the right padding
        // of one button and the left padding of the next are the same number,
        // so there is no pixel belonging to neither.
        let left = TextSelectionToolbarTextButton::padding(0, 3);
        let right = TextSelectionToolbarTextButton::padding(1, 3);
        assert_eq!(left.right, right.left);
    }

    #[test]
    fn a_cupertino_handle_grows_with_the_text_and_a_material_one_does_not() {
        // The difference between a lollipop and a square. iOS draws a knob on
        // a stem that runs the height of the line, so a handle beside a large
        // heading is taller than one beside body copy; Material draws a fixed
        // square that sits under the line whatever the text.
        let ios = CupertinoTextSelectionControls;
        let material = MaterialTextSelectionControls;

        let small = ios.handle_size(14.0);
        let large = ios.handle_size(40.0);
        assert!(large.height > small.height, "{large:?} vs {small:?}");
        assert_eq!(small.width, large.width, "only the stem grows");

        assert_eq!(
            material.handle_size(14.0),
            material.handle_size(40.0),
            "a square is a square"
        );
    }

    #[test]
    fn the_stem_overlaps_the_knob_so_the_join_has_no_seam() {
        // Which is why the height is a sum *minus* the overlap rather than a
        // plain sum: without it the two shapes would meet exactly and the
        // anti-aliasing of each would fall short of the other, leaving a
        // hairline.
        let ios = CupertinoTextSelectionControls;
        let radius = CupertinoTextSelectionControls::HANDLE_RADIUS;
        let overlap = CupertinoTextSelectionControls::HANDLE_OVERLAP;
        assert!(overlap > 0.0);
        assert_eq!(
            ios.handle_size(20.0).height,
            20.0 + radius * 2.0 - overlap,
            "the sum, less the overlap"
        );
    }

    #[test]
    fn the_left_handles_anchor_is_at_the_bottom_and_the_right_ones_near_its_knob() {
        // The left handle's knob is at the top and its stem lies along the
        // text, so the anchor is all the way down; the right one is drawn
        // flipped, so its anchor is near the top of its knob with the same
        // seam-hiding overlap applied.
        let ios = CupertinoTextSelectionControls;
        let line = 20.0;
        let size = ios.handle_size(line);
        assert_eq!(
            ios.handle_anchor(TextSelectionHandleType::Left, line),
            Offset::new(size.width / 2.0, size.height)
        );
        let right = ios.handle_anchor(TextSelectionHandleType::Right, line);
        assert!(
            right.dy < size.height,
            "near the top rather than the bottom"
        );
        assert_eq!(right.dx, size.width / 2.0, "both are horizontally centred");

        // And a collapsed handle is centred on the line, having no side to
        // prefer.
        let collapsed = ios.handle_anchor(TextSelectionHandleType::Collapsed, line);
        assert_eq!(collapsed.dy, line + (size.height - line) / 2.0);
    }

    #[test]
    fn both_desktop_controls_draw_no_handles_and_are_still_two_classes() {
        // The toolbar is what differs, not the handles -- so the two answer
        // identically here, and the reason they are two classes is the menu
        // each puts up.
        let ios = CupertinoDesktopTextSelectionControls;
        let material = DesktopTextSelectionControls;
        assert_eq!(ios.handle_size(20.0), material.handle_size(20.0));
        assert_eq!(ios.handle_size(20.0), Size::ZERO);
        assert!(!ios.draws_handles(20.0));
    }

    #[test]
    fn the_ios_toolbar_moves_both_anchors_by_the_same_distance() {
        // Unlike the Material toolbar, whose lower distance has to clear a
        // drag handle: an iOS handle is a stem *along* the line rather than a
        // knob below it, so there is nothing under the selection to clear.
        let toolbar =
            CupertinoTextSelectionToolbar::new(Offset::new(50.0, 200.0), Offset::new(50.0, 220.0));
        let (above, below) = toolbar.padded_anchors(0.0);
        assert_eq!(above.dy, 192.0);
        assert_eq!(below.dy, 228.0);
        assert_eq!(
            200.0 - above.dy,
            below.dy - 220.0,
            "the same 8 in each direction"
        );
        // Where Material's two differ, because of the handle.
        assert_ne!(
            TextSelectionToolbar::CONTENT_DISTANCE,
            TextSelectionToolbar::CONTENT_DISTANCE_BELOW
        );
    }

    #[test]
    fn the_arrow_stays_much_further_from_the_screen_edge_than_the_toolbar_does() {
        // The toolbar may reach nearly to the edge; the arrow may not, or it
        // would be drawn against the toolbar's own rounded corner and lose its
        // point.
        assert!(
            CupertinoTextSelectionToolbar::ARROW_SCREEN_PADDING
                > CupertinoTextSelectionToolbar::SCREEN_PADDING * 3.0
        );
        let (left, right) = CupertinoTextSelectionToolbar::arrow_tip_range(400.0);
        assert_eq!((left, right), (26.0, 374.0));
    }

    #[test]
    fn the_arrow_is_wider_than_it_is_tall() {
        // So it reads as a pointer rather than as a spike.
        assert!(
            CupertinoTextSelectionToolbar::ARROW_WIDTH
                > CupertinoTextSelectionToolbar::ARROW_HEIGHT
        );
    }

    #[test]
    fn the_last_line_of_the_live_text_glyph_is_short() {
        // A ragged last line is what makes the three strokes read as a
        // paragraph rather than as three rules. It is the detail a redraw
        // from memory loses first.
        let lines = LiveTextIconPainter::lines();
        let widths: Vec<f32> = lines.iter().map(|(a, b)| b.dx - a.dx).collect();
        assert_eq!(widths[0], widths[1], "the first two are the same length");
        assert!(
            widths[2] < widths[0],
            "and the third is shorter: {widths:?}"
        );

        // All three start at the same left edge, so it is the end that is
        // pulled in rather than the line being centred and shrunk.
        assert!(lines.iter().all(|(start, _)| start.dx == -3.0));

        // Evenly spaced down the middle.
        assert_eq!(lines[0].0.dy, -3.0);
        assert_eq!(lines[1].0.dy, 0.0);
        assert_eq!(lines[2].0.dy, 3.0);
    }

    #[test]
    fn the_corner_is_drawn_once_and_turned_four_times() {
        // Writing out four corners is where the mistakes live: they have to
        // agree about the arc's direction as well as its place.
        assert_eq!(LiveTextIconPainter::CORNERS, 4);

        // The two numbers themselves, since every relationship below is
        // written in terms of them and would hold for any pair.
        assert_eq!(LiveTextIconPainter::ARM, 3.5);
        assert_eq!(LiveTextIconPainter::CORNER_RADIUS, 1.0);
        assert!(
            LiveTextIconPainter::CORNER_RADIUS < LiveTextIconPainter::ARM,
            "the turn has to fit inside the arm it turns from"
        );

        let size = CupertinoTextSelectionToolbarButton::LIVE_TEXT_ICON_DIMENSION;
        let (arm_end, turn_start, turn_end, other_arm_end) = LiveTextIconPainter::corner(size);

        // The path starts at the far end of one arm and finishes at the far
        // end of the other, both `ARM` from the corner.
        let corner = Offset::new(-size / 2.0, -size / 2.0);
        assert_eq!(arm_end.dy - corner.dy, LiveTextIconPainter::ARM);
        assert_eq!(other_arm_end.dx - corner.dx, LiveTextIconPainter::ARM);
        assert_eq!(arm_end.dx, corner.dx, "the vertical arm is vertical");
        assert_eq!(other_arm_end.dy, corner.dy, "and the horizontal one is not");

        // The turn is one radius in from the corner on each side, which is
        // what makes it a quarter circle rather than a chamfer.
        assert_eq!(
            turn_start.dy - corner.dy,
            LiveTextIconPainter::CORNER_RADIUS
        );
        assert_eq!(turn_end.dx - corner.dx, LiveTextIconPainter::CORNER_RADIUS);
        assert_eq!(turn_start.dx, corner.dx);
        assert_eq!(turn_end.dy, corner.dy);
    }

    #[test]
    fn the_corner_is_placed_from_the_centre_so_the_turns_are_about_the_middle() {
        // Upstream translates to the centre before rotating, so the corner is
        // expressed as an offset from there. A bigger square pushes the
        // corner out by half the difference, and the arms keep their length.
        let small = LiveTextIconPainter::corner(13.0);
        let large = LiveTextIconPainter::corner(20.0);
        assert!(large.0.dx < small.0.dx, "further out from the centre");
        assert_eq!(
            large.3.dx - large.0.dx,
            small.3.dx - small.0.dx,
            "and the same size of corner either way"
        );
    }

    #[test]
    fn live_text_is_the_one_button_that_shows_no_words() {
        // It answers the empty label because it draws a glyph, not because a
        // label was left out.
        use crate::icon_data::ContextMenuButtonType;
        assert_eq!(ContextMenuButtonType::LiveTextInput.cupertino_label(), "");
        assert_eq!(
            CupertinoTextSelectionToolbarButton::LIVE_TEXT_ICON_DIMENSION,
            13.0
        );
        assert!(
            !ContextMenuButtonType::Copy.cupertino_label().is_empty(),
            "while a button that does show words has some"
        );
    }

    #[test]
    fn a_pressed_menu_button_darkens_its_background_and_does_not_fade() {
        // Fading the foreground would take the label with it, so the word the
        // reader is pressing would be the hardest one on the row to read at
        // the moment they press it.
        type Button = CupertinoTextSelectionToolbarButton;
        assert_eq!(Button::PRESSED_OPACITY, 1.0, "the fade is switched off");

        let resting = Button::background(false, false);
        let pressed = Button::background(true, false);
        assert_eq!(resting, crate::cupertino::CupertinoColors::TRANSPARENT);
        assert_ne!(pressed, resting, "the background is what changes");
    }

    #[test]
    fn the_press_is_a_wash_over_the_toolbar_rather_than_a_new_surface() {
        // Six per cent of an alpha, and opposite colours by brightness: the
        // button reads as itself darkened, not as something laid on top.
        type Button = CupertinoTextSelectionToolbarButton;
        let light = Button::background(true, false);
        let dark = Button::background(true, true);
        assert_eq!(light, crate::engine::Color::argb(0x10, 0x00, 0x00, 0x00));
        assert_eq!(dark, crate::engine::Color::argb(0x10, 0xFF, 0xFF, 0xFF));
        assert_ne!(light, dark, "black on light, white on dark");
    }

    #[test]
    fn a_disabled_menu_button_is_marked_only_by_its_grey_text() {
        // `disabledColor: transparent`, so there is no third background --
        // disabled and resting look the same behind the word.
        type Button = CupertinoTextSelectionToolbarButton;
        assert_eq!(
            Button::background(false, false),
            crate::cupertino::CupertinoColors::TRANSPARENT
        );

        let enabled = Button::label_color(true, false);
        let disabled = Button::label_color(false, false);
        assert_ne!(enabled, disabled);
        assert_eq!(
            disabled,
            crate::cupertino::CupertinoColors::INACTIVE_GRAY.color
        );
        assert_eq!(enabled, crate::cupertino::CupertinoColors::BLACK);
        assert_eq!(
            Button::label_color(true, true),
            crate::cupertino::CupertinoColors::WHITE,
            "and white on dark"
        );
    }

    #[test]
    fn a_disabled_menu_button_never_enters_the_gesture_arena() {
        // Upstream returns the bare button when `onPressed` is null rather
        // than a detector that ignores taps, so a scroll starting on top of a
        // disabled button is not delayed by one.
        type Button = CupertinoTextSelectionToolbarButton;
        assert!(Button::wraps_in_gesture_detector(true));
        assert!(!Button::wraps_in_gesture_detector(false));
    }

    #[test]
    fn the_menu_text_takes_nothing_from_the_page_underneath() {
        // Upstream's `inherit: false`. A selection menu floats over whatever
        // was being read, and inheriting would size its words by that
        // paragraph.
        type Button = CupertinoTextSelectionToolbarButton;
        assert_eq!(Button::FONT_SIZE, 15.0);
        assert_eq!(Button::FONT_WEIGHT, 400);
        assert!(
            Button::LETTER_SPACING < 0.0,
            "pulled together, not apart: {}",
            Button::LETTER_SPACING
        );
    }

    #[test]
    fn an_ios_menu_button_is_taller_than_it_is_wide_and_a_desktop_one_is_not() {
        // An iOS selection menu is one row of words with dividers and no
        // icons, so the height is what gives each word a target. A macOS menu
        // row is denser and leans on its width instead.
        let ios = CupertinoTextSelectionToolbarButton::PADDING;
        assert!(ios.top > ios.left, "{ios:?}");

        let desktop = CupertinoDesktopTextSelectionToolbarButton::PADDING;
        assert!(desktop.left > desktop.top);
        // And the same downward lean the Material desktop button has, for the
        // same optical-centring reason.
        assert!(desktop.bottom > desktop.top);
    }

    #[test]
    fn the_two_desktop_toolbars_agree_on_their_width() {
        // Both were measured from the same macOS menu, so a disagreement here
        // would mean one of them had drifted.
        assert_eq!(
            CupertinoDesktopTextSelectionToolbar::WIDTH,
            DesktopTextSelectionToolbar::WIDTH
        );
    }

    #[test]
    fn the_blur_comes_with_a_saturation_boost() {
        // The two go together: a blur averages colours and washes them out, so
        // the saturation is pushed back up to keep what shows through
        // recognisable. One without the other would look wrong.
        assert!(CupertinoDesktopTextSelectionToolbar::BLUR_SIGMA > 0.0);
        assert!(CupertinoDesktopTextSelectionToolbar::SATURATION_BOOST > 1.0);
    }
}
