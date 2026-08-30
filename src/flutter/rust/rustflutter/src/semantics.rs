// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the screen says, for a reader who is not looking at it.
//!
//! Everything else in this framework describes pixels. This describes the same
//! interface again, in the only terms a screen reader can use: here is a
//! button, it says "Increment", tapping it does something, and it is at these
//! coordinates so a finger dragged across the glass can find it. Upstream this
//! is `semantics/semantics.dart` and the `Semantics` widget over it; the shape
//! here is the same and the vocabulary is the same, because the vocabulary is
//! not ours -- it is what `SemanticsNode` carries across to the platform, and
//! from there what TalkBack and Narrator understand.
//!
//! # Why it is off until it is asked for
//!
//! Nothing is built unless the platform says a screen reader is listening.
//! Upstream does the same (`PlatformView::SetSemanticsEnabled`, reaching
//! `SemanticsBinding.semanticsEnabled`), and the reason is not only cost: a
//! semantics tree that nothing consumes cannot be wrong in any way anybody
//! notices, so it would rot. Built only when it is read, it is read.
//!
//! # Where the rectangles come from
//!
//! A node needs a rectangle in root coordinates, and only the walk down the
//! render tree can work one out -- each parent knows where it put each child,
//! and nothing knows where it is itself. Upstream walks the render tree for
//! exactly this, through `visitChildrenForSemantics` and `applyPaintTransform`;
//! [`flush`] does the same walk through
//! [`crate::render::RenderBox::visit_children_for_semantics`], which carries
//! the offset because there is no `parentData` here to read it off the child.
//!
//! **It used to ride on the paint walk instead**, which had the offset already
//! and cost nothing extra. Two things were wrong with that and both were
//! structural. A repaint boundary that handed back the layer it kept did not
//! walk, so a subtree that had not been drawn again said nothing -- which was
//! patched by making the boundary redraw whenever a reader was listening,
//! throwing away every retained layer on the screen for as long as a screen
//! reader was open. And the offsets were wrong: a boundary paints its child at
//! the origin and puts the offset in the layer, so every node inside one
//! reported its position *within the boundary* as though it were the position
//! on the glass -- and [`crate::scrolling::LazyList`] puts a boundary around
//! every row. Both stop being possible once the walk is its own.
//!
//! # What the walk clips away
//!
//! The walk also carries the clips down. Every box that paints through one --
//! a viewport's window, a `ClipRect`'s bounds -- is asked for it as the walk
//! passes ([`crate::render::RenderBox::describe_approximate_paint_clip`] and
//! [`crate::render::RenderBox::describe_semantics_clip`], the pair upstream's
//! `_SemanticsGeometry.computeChildGeometry` accumulates), the answers are
//! intersected into the clips already carried, and each node's rectangle is
//! then cut by the result. A rectangle the clips empty does not reach the
//! platform at all: upstream keeps a paint-clipped node in the tree under a
//! `hidden` flag, and this bridge has no such flag to put in
//! [`SemanticsNode`], so -- of the choices that do not report a rectangle
//! outside the window -- the node is dropped.
//!
//! # The three gates
//!
//! A walk of its own is a walk somebody has to pay for, and the first version
//! of it paid on every frame a reader was listening. Upstream does not, and it
//! avoids the work at three separate places. All three are here:
//!
//! 1. **Nothing is marked when nobody is reading.** Upstream's
//!    `markNeedsSemanticsUpdate` returns at once while `_semanticsOwner` is
//!    null, and `flushSemantics` returns at once for the same reason. Here
//!    [`enabled`] is that gate.
//! 2. **A frame that changed nothing is not walked.** Upstream keeps
//!    `PipelineOwner._nodesNeedingSemanticsUpdate` and visits what is in it;
//!    [`mark_needs_update`] fills the same role, and [`flush`] returns without
//!    walking when it is empty. What marks is listed on [`mark_needs_update`],
//!    and each entry is a line upstream also has.
//! 3. **A walk that came out the same sends nothing.** Upstream's
//!    `SemanticsOwner.sendSemanticsUpdate` opens with
//!    `if (_dirtyNodes.isEmpty) return;` and puts only the dirty nodes on the
//!    wire. The tree the platform is holding is kept here (see [`tree`]) and
//!    compared, which answers the same question for a tree small enough that
//!    comparing it is cheaper than keeping a change log -- the same trade the
//!    Windows bridge already makes on the other side of the boundary.
//!
//! The one upstream gate that is *not* here is the fourth: upstream re-walks
//! only the subtree under the dirtied semantics boundary, because its dirty
//! list holds render objects and its node rectangles are relative to the parent
//! node, so a scrolled viewport moves one transform instead of every rectangle
//! under it. Here the rectangles are absolute -- both bridges below want "where
//! on the glass" -- so a subtree cannot be reused where it moved, and the walk
//! descends from the root. That is the same trade [`crate::render::RenderRef`]
//! makes for layout, for the same missing piece: there is no pipeline owner
//! holding a list of boundaries to resume a descent from.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::direction::TextDirection;
use crate::engine::Rect;
use crate::framework::{AnyWidget, BuildContext, Component, component, single};
use crate::render::{BoxConstraints, BoxedRender, Offset, PaintContext, RenderBox, Size};
use crate::services::text_boundary::TextRange;

/// **What kind of thing** a node is, where saying so needs more than a flag.
///
/// Upstream's `SemanticsRole` (`semantics.dart`, mirrored as
/// `flutter::SemanticsRole` in `semantics_node.h`), which this port had none of
/// -- every node it produced crossed to the engine as `kNone`.
///
/// # A role is not a flag
///
/// The flags this crate already sets say what a control *does*: it can be
/// checked, it is selected, it is a button. A role says what it **is** in the
/// structure of the page, and a platform's accessibility layer maps it onto a
/// native control -- a cell of a table, a header of a column, an item of a
/// menu. The difference shows up where there is nothing to do: a column header
/// has no state and no action, so a flag has nothing to say about it, and
/// without a role a reader meets the word "Name" and no reason to think it
/// names a column.
///
/// The discriminants are the engine's, and they have to match exactly: the
/// value crosses as a plain integer, so a role inserted in the middle on one
/// side would arrive as its neighbour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum SemanticsRole {
    /// No role, which is upstream's default and what every node in this port
    /// said until roles existed here.
    #[default]
    None = 0,
    Tab = 1,
    TabBar = 2,
    TabPanel = 3,
    Dialog = 4,
    AlertDialog = 5,
    Table = 6,
    Cell = 7,
    Row = 8,
    ColumnHeader = 9,
    DragHandle = 10,
    SpinButton = 11,
    ComboBox = 12,
    MenuBar = 13,
    Menu = 14,
    MenuItem = 15,
    MenuItemCheckbox = 16,
    MenuItemRadio = 17,
    List = 18,
    ListItem = 19,
    Form = 20,
    Tooltip = 21,
    LoadingSpinner = 22,
    ProgressBar = 23,
    HotKey = 24,
    RadioGroup = 25,
    Status = 26,
    Alert = 27,
    Complementary = 28,
    ContentInfo = 29,
    Main = 30,
    Navigation = 31,
    Region = 32,
}

impl SemanticsRole {
    /// Whether this node has a role at all, which is the test upstream's
    /// merge uses: a merging node takes a descendant's role only if it has
    /// none of its own.
    pub fn is_set(self) -> bool {
        self != SemanticsRole::None
    }
}

/// What a reader can be told to do with a node.
///
/// The discriminants are `flutter::SemanticsAction`, which is in turn
/// `SemanticsAction` in `semantics.dart` and in every embedder. Four copies of
/// one set of bits upstream; this is the fifth, and it has to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum SemanticsAction {
    Tap = 1 << 0,
    LongPress = 1 << 1,
    ScrollLeft = 1 << 2,
    ScrollRight = 1 << 3,
    ScrollUp = 1 << 4,
    ScrollDown = 1 << 5,
    Increase = 1 << 6,
    Decrease = 1 << 7,
    ShowOnScreen = 1 << 8,
    /// The nine that make a text field editable by a screen reader. This port
    /// had the *field* -- `is_text_field`, `is_obscured` and `is_read_only`
    /// are all on [`SemanticsFlags`] -- and none of the verbs, so a reader
    /// could be told it had found one and given no way to work it.
    MoveCursorForwardByCharacter = 1 << 9,
    MoveCursorBackwardByCharacter = 1 << 10,
    SetSelection = 1 << 11,
    Copy = 1 << 12,
    Cut = 1 << 13,
    Paste = 1 << 14,
    DidGainAccessibilityFocus = 1 << 15,
    DidLoseAccessibilityFocus = 1 << 16,
    /// The only bit that does not name what it does.
    ///
    /// It says "one of the application's own actions", and **which one arrives
    /// in a separate integer** -- so a bridge that treats this like the
    /// others has thrown away the only part that carried the meaning.
    CustomAction = 1 << 17,
    Dismiss = 1 << 18,
    MoveCursorForwardByWord = 1 << 19,
    MoveCursorBackwardByWord = 1 << 20,
    SetText = 1 << 21,
    Focus = 1 << 22,
    /// **Not a fourth scroll direction.** The four directions are a nudge --
    /// move by about a screenful -- and this one carries a destination. A
    /// reader dragging a scrollbar sends this; one pressing a page key sends
    /// [`SemanticsAction::ScrollDown`].
    ScrollToOffset = 1 << 23,
    Expand = 1 << 24,
    Collapse = 1 << 25,
}

impl SemanticsAction {
    /// Every action, in bit order.
    pub const ALL: [SemanticsAction; 26] = [
        SemanticsAction::Tap,
        SemanticsAction::LongPress,
        SemanticsAction::ScrollLeft,
        SemanticsAction::ScrollRight,
        SemanticsAction::ScrollUp,
        SemanticsAction::ScrollDown,
        SemanticsAction::Increase,
        SemanticsAction::Decrease,
        SemanticsAction::ShowOnScreen,
        SemanticsAction::MoveCursorForwardByCharacter,
        SemanticsAction::MoveCursorBackwardByCharacter,
        SemanticsAction::SetSelection,
        SemanticsAction::Copy,
        SemanticsAction::Cut,
        SemanticsAction::Paste,
        SemanticsAction::DidGainAccessibilityFocus,
        SemanticsAction::DidLoseAccessibilityFocus,
        SemanticsAction::CustomAction,
        SemanticsAction::Dismiss,
        SemanticsAction::MoveCursorForwardByWord,
        SemanticsAction::MoveCursorBackwardByWord,
        SemanticsAction::SetText,
        SemanticsAction::Focus,
        SemanticsAction::ScrollToOffset,
        SemanticsAction::Expand,
        SemanticsAction::Collapse,
    ];

    /// Upstream's `kVerticalScrollSemanticsActions`, which is the one place
    /// the engine bundles two of these bits into a name: a node that can be
    /// scrolled vertically offers both, and offering one alone would be a
    /// list you can go down and not back up.
    pub const VERTICAL_SCROLL: i32 =
        SemanticsAction::ScrollUp as i32 | SemanticsAction::ScrollDown as i32;

    /// Whether this action is one of the nine that edit text.
    ///
    /// Worth a name because it is the shape of what was missing: this port
    /// had the *field* and none of the verbs, so a reader could be told it
    /// had found a text field and given no way to work it.
    pub fn edits_text(self) -> bool {
        use SemanticsAction::*;
        matches!(
            self,
            MoveCursorForwardByCharacter
                | MoveCursorBackwardByCharacter
                | MoveCursorForwardByWord
                | MoveCursorBackwardByWord
                | SetSelection
                | Copy
                | Cut
                | Paste
                | SetText
        )
    }

    /// The action a bit stands for, or `None` for one this framework has no
    /// name for yet.
    pub fn from_bits(bits: i32) -> Option<SemanticsAction> {
        use SemanticsAction::*;
        Some(match bits {
            x if x == Tap as i32 => Tap,
            x if x == LongPress as i32 => LongPress,
            x if x == ScrollLeft as i32 => ScrollLeft,
            x if x == ScrollRight as i32 => ScrollRight,
            x if x == ScrollUp as i32 => ScrollUp,
            x if x == ScrollDown as i32 => ScrollDown,
            x if x == Increase as i32 => Increase,
            x if x == Decrease as i32 => Decrease,
            x if x == ShowOnScreen as i32 => ShowOnScreen,
            x if x == MoveCursorForwardByCharacter as i32 => MoveCursorForwardByCharacter,
            x if x == MoveCursorBackwardByCharacter as i32 => MoveCursorBackwardByCharacter,
            x if x == SetSelection as i32 => SetSelection,
            x if x == Copy as i32 => Copy,
            x if x == Cut as i32 => Cut,
            x if x == Paste as i32 => Paste,
            x if x == DidGainAccessibilityFocus as i32 => DidGainAccessibilityFocus,
            x if x == DidLoseAccessibilityFocus as i32 => DidLoseAccessibilityFocus,
            x if x == CustomAction as i32 => CustomAction,
            x if x == Dismiss as i32 => Dismiss,
            x if x == MoveCursorForwardByWord as i32 => MoveCursorForwardByWord,
            x if x == MoveCursorBackwardByWord as i32 => MoveCursorBackwardByWord,
            x if x == SetText as i32 => SetText,
            x if x == Focus as i32 => Focus,
            x if x == ScrollToOffset as i32 => ScrollToOffset,
            x if x == Expand as i32 => Expand,
            x if x == Collapse as i32 => Collapse,
            _ => return None,
        })
    }
}

/// Upstream `SemanticsTristate`: a flag that can be unset, true or false.
///
/// The same shape as [`SemanticsCheckState`] with one value fewer, and the
/// same reason for existing: "this node has no opinion about being toggled"
/// and "this node is a switch that is off" are different things, and a reader
/// that cannot tell them apart says "off" about a heading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SemanticsTristate {
    #[default]
    None,
    True,
    False,
}

impl SemanticsTristate {
    pub fn of(value: bool) -> SemanticsTristate {
        if value {
            SemanticsTristate::True
        } else {
            SemanticsTristate::False
        }
    }

    /// Whether the node has an opinion at all.
    pub fn is_set(self) -> bool {
        self != SemanticsTristate::None
    }

    /// The union, said for three values: **on wins, then off, then no
    /// opinion**.
    ///
    /// Upstream's `Tristate.merge` (`semantics.dart:1136`), and it is
    /// deliberately not first-wins:
    ///
    /// ```dart
    /// if (this == Tristate.isTrue || other == Tristate.isTrue) return Tristate.isTrue;
    /// if (this == Tristate.isFalse || other == Tristate.isFalse) return Tristate.isFalse;
    /// return Tristate.none;
    /// ```
    ///
    /// This used to keep the first and say so in prose -- "two that disagree
    /// keep the first, because it is the rule every other singular slot
    /// follows". That reasoning was about this crate rather than about
    /// upstream, and it is wrong: a merged node stands for **everything folded
    /// into it**, so if any part of it is selected, the node a reader lands on
    /// is selected. First-wins would have made the answer depend on which
    /// child the walk happened to reach first.
    ///
    /// It could not be observed while nothing reached it -- see
    /// [`SemanticsConfiguration::absorb`] -- and round 389 is what gave it a
    /// caller.
    pub fn merge(self, other: SemanticsTristate) -> SemanticsTristate {
        if self == SemanticsTristate::True || other == SemanticsTristate::True {
            return SemanticsTristate::True;
        }
        if self == SemanticsTristate::False || other == SemanticsTristate::False {
            return SemanticsTristate::False;
        }
        SemanticsTristate::None
    }
}

/// Upstream `SemanticsCheckState`: what a checkable node's box looks like.
///
/// Four values and not two booleans, and the fourth is the reason. This port
/// carried `has_checked_state` and `is_checked`, which say three things --
/// not checkable, checked, unchecked -- and had nowhere for **mixed** to come
/// from. The whole chain agreed: two bits in the ABI, and a ternary in
/// `runtime_controller.cc` that could only ever produce `kTrue` or `kFalse`.
///
/// Meanwhile this port *has* tristate checkboxes:
/// [`crate::list_tiles::ControlListTile::value`] is an `Option<bool>` whose
/// doc says "`None` is the indeterminate state". So a half-checked "select
/// all" box -- the one above a list where some rows are chosen -- announced
/// as **not checked**, which is one of the two things it is not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SemanticsCheckState {
    /// Not a checkable thing at all. What stops a reader saying "not checked"
    /// about a label.
    #[default]
    None,
    Checked,
    Unchecked,
    /// Some of what this stands for is checked and some is not.
    Mixed,
}

impl SemanticsCheckState {
    /// What a control holding `value` looks like, in this port's idiom for a
    /// checkable thing: `None` is the indeterminate state and not the absence
    /// of one.
    pub fn of(value: Option<bool>) -> SemanticsCheckState {
        match value {
            Some(true) => SemanticsCheckState::Checked,
            Some(false) => SemanticsCheckState::Unchecked,
            None => SemanticsCheckState::Mixed,
        }
    }

    /// Whether there is a box at all. Upstream's `kNone` against the rest,
    /// and the old `has_checked_state` boolean.
    pub fn is_checkable(self) -> bool {
        self != SemanticsCheckState::None
    }

    /// The `merge` rule, which is the union the rest of the flags use, said
    /// for four values instead of two.
    ///
    /// A node that is not checkable takes the other's state outright. Two
    /// that disagree are **mixed**, which is what the value is for: folding a
    /// checked thing and an unchecked thing into one node produces a node
    /// that is partly checked, and saying "checked" or "unchecked" there
    /// would be picking one of them at random.
    pub fn merge(self, other: SemanticsCheckState) -> SemanticsCheckState {
        // Upstream's `CheckedState.merge` (`semantics.dart:1101`), in its
        // order: **mixed, then checked, then unchecked, then none**.
        //
        // The rule this replaced said that two checkable things which disagree
        // fold into *mixed*, which sounds right and is not upstream's. Mixed
        // is what a control says when **it** is partly ticked -- a parent
        // checkbox over some ticked children -- not what two separate nodes
        // become by disagreeing. Announcing a merged row as "partially
        // checked" because a ticked and an unticked thing ended up inside it
        // describes a control that does not exist.
        if self == SemanticsCheckState::Mixed || other == SemanticsCheckState::Mixed {
            return SemanticsCheckState::Mixed;
        }
        if self == SemanticsCheckState::Checked || other == SemanticsCheckState::Checked {
            return SemanticsCheckState::Checked;
        }
        if self == SemanticsCheckState::Unchecked || other == SemanticsCheckState::Unchecked {
            return SemanticsCheckState::Unchecked;
        }
        SemanticsCheckState::None
    }
}

/// What a node *is*, as opposed to what can be done to it.
///
/// A subset of upstream's `SemanticsFlags`: the ones that change what a screen
/// reader says out loud rather than how a particular platform arranges its
/// accessibility tree. Adding one is adding a field here, a bit in the C
/// struct, and a line in whichever bridge cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemanticsFlags {
    pub is_button: bool,
    pub is_text_field: bool,
    pub is_header: bool,
    /// Upstream's `namesRoute`: this node's label is the name of the page.
    ///
    /// What makes a route change speakable. When a new route arrives the
    /// reader announces the node carrying this rather than reading the whole
    /// screen, and upstream sets it on exactly one line per route -- an app
    /// bar's title, a dialog's title, a bottom sheet's.
    pub names_route: bool,
    /// Upstream's `scopesRoute`: **focus lives inside this subtree now**.
    ///
    /// Its companion, and not the same claim. `names_route` says *what to
    /// announce* when a route arrives; this says *where the reader now is* --
    /// a platform confines exploration to the scoped subtree, so the page
    /// underneath a dialog stops being reachable by swiping past it. Without
    /// it a reader can walk straight out of a modal into the page it was
    /// meant to block, and be dealing with a screen that is not listening.
    ///
    /// Upstream sets it on every modal surface: both Material dialogs
    /// (`dialog.dart:935` and `1365`), the bottom sheet, the drawer, the
    /// dropdown menu, and every page route (`page.dart:194`). In the dialogs
    /// it sits **inside `if (label != null)`** alongside `namesRoute` -- so
    /// unlike the role, it rides with the label rather than standing on its
    /// own.
    pub scopes_route: bool,
    /// Upstream's `isHidden`: in the tree and not on the screen.
    ///
    /// Covered by something, or scrolled past. **Not the same as excluding a
    /// node**, which takes it out: a hidden node keeps its place, so "3 of
    /// 40" still counts it and a reader moving forward still arrives.
    pub is_hidden: bool,
    pub is_image: bool,
    pub is_link: bool,
    pub is_slider: bool,
    pub is_obscured: bool,
    pub is_read_only: bool,
    pub is_live_region: bool,
    /// Whether this node can be checked at all and which way, in one value.
    ///
    /// This was two booleans, and two booleans cannot say *mixed*. See
    /// [`SemanticsCheckState`].
    pub checked: SemanticsCheckState,
    /// Upstream's `isToggled`, which is a **switch** and not a checkbox.
    ///
    /// A reader says "on" and "off" for this and "checked" and "not checked"
    /// for [`SemanticsFlags::checked`]. The port raised the checked flag for
    /// its `Switch`, under a comment that was right about the need and wrong
    /// about the flag, so every switch announced itself as a checkbox.
    pub toggled: SemanticsTristate,
    /// Upstream's `isExpanded`: "expanded" or "collapsed", which is what an
    /// expansion tile is and what its arrow means.
    pub expanded: SemanticsTristate,
    /// Upstream's `isRequired`: "required", which is what a form field is
    /// before it is wrong -- said when the reader arrives rather than after
    /// they have left it empty.
    pub required: SemanticsTristate,
    pub has_enabled_state: bool,
    pub is_enabled: bool,
    /// Upstream's `isSelected`, a tristate.
    ///
    /// This was a boolean, and the bridge turned its false into the engine's
    /// `kNone` -- so a tab that is selectable and currently is not crossed as
    /// a node with no opinion, and a reader that would have said "not
    /// selected" said nothing.
    pub selected: SemanticsTristate,
    /// Upstream's `isInMutuallyExclusiveGroup`: one of a set where choosing
    /// this one un-chooses the others.
    ///
    /// This is what separates a **radio** from a checkbox for a screen reader.
    /// Both are checkable and both announce on and off; only this says that
    /// turning one on turns another off, which is the difference between
    /// "seven of these are on" being possible and being nonsense. Without it a
    /// column of radios is read as a column of checkboxes.
    pub is_in_mutually_exclusive_group: bool,
    /// Whether the keyboard is here. Separate from the framework's own focus
    /// because accessibility focus and keyboard focus are different things
    /// that happen to coincide most of the time.
    /// Upstream's `isFocused`, a tristate for the same reason
    /// [`SemanticsFlags::selected`] is: a field that can hold the keyboard
    /// and does not is "not focused", and a heading has no opinion.
    pub focused: SemanticsTristate,
}

impl SemanticsFlags {
    /// Upstream's `SemanticsFlags.merge`: the union.
    ///
    /// A flag is a claim -- "this is a button", "this is checked" -- and two
    /// things folded into one node make both claims. Nothing here has an
    /// off-beats-on rule, so the union is the whole of it.
    pub fn merge(&self, other: &SemanticsFlags) -> SemanticsFlags {
        SemanticsFlags {
            is_button: self.is_button || other.is_button,
            is_text_field: self.is_text_field || other.is_text_field,
            is_header: self.is_header || other.is_header,
            names_route: self.names_route || other.names_route,
            scopes_route: self.scopes_route || other.scopes_route,
            is_hidden: self.is_hidden || other.is_hidden,
            is_image: self.is_image || other.is_image,
            is_link: self.is_link || other.is_link,
            is_slider: self.is_slider || other.is_slider,
            is_obscured: self.is_obscured || other.is_obscured,
            is_read_only: self.is_read_only || other.is_read_only,
            is_live_region: self.is_live_region || other.is_live_region,
            // Not a union: two checkable things that disagree fold into a
            // node that is *mixed*, which is the value's whole reason for
            // existing. `||` would have picked "checked" whenever either was.
            checked: self.checked.merge(other.checked),
            toggled: self.toggled.merge(other.toggled),
            expanded: self.expanded.merge(other.expanded),
            required: self.required.merge(other.required),
            has_enabled_state: self.has_enabled_state || other.has_enabled_state,
            is_enabled: self.is_enabled || other.is_enabled,
            selected: self.selected.merge(other.selected),
            is_in_mutually_exclusive_group: self.is_in_mutually_exclusive_group
                || other.is_in_mutually_exclusive_group,
            focused: self.focused.merge(other.focused),
        }
    }

    /// Upstream's `SemanticsFlags.hasConflictingFlags`: whether merging these
    /// two would describe something that cannot exist.
    ///
    /// **The rule is the same flag on both sides**, not two different flags.
    /// Two buttons cannot merge into one node, because the result would be one
    /// thing a reader can press where there were two. A button and a text
    /// field *can*, and upstream lets them -- what stops the pairs that should
    /// not merge is the separate role check,
    /// [`SemanticsConfiguration::has_explicit_role`].
    ///
    /// This is worth stating because the plausible reading is the other one. I
    /// wrote a test asserting a button and a text field conflict, and it was
    /// the test that was wrong.
    ///
    /// Upstream's list also has an inequality at the end --
    /// `isAccessibilityFocusBlocked != other.isAccessibilityFocusBlocked` --
    /// which has no counterpart here because that flag is not modelled.
    pub fn conflicts_with(&self, other: &SemanticsFlags) -> bool {
        let both = |mine: bool, theirs: bool| mine && theirs;
        both(self.is_button, other.is_button)
            || both(self.is_text_field, other.is_text_field)
            || both(self.is_header, other.is_header)
            || both(self.is_obscured, other.is_obscured)
            || both(self.is_image, other.is_image)
            || both(self.is_live_region, other.is_live_region)
            || both(self.is_read_only, other.is_read_only)
            || both(self.is_link, other.is_link)
            || both(self.is_slider, other.is_slider)
            // The tri-state ones: upstream's `hasConflict` on a flag that can
            // be unset, set true or set false.
            //
            // `checked` is no longer one of the bools this comment used to
            // group it with -- it is a four-valued state now -- so the test
            // for it is the right one: two *checkable* nodes conflict
            // whichever way each is set, because merging them loses which was
            // which. An unchecked node and a checked one are as much a
            // conflict as two checked ones, and `both(is_checked, ...)` said
            // they were not.
            || (self.checked.is_checkable() && other.checked.is_checkable())
            || (self.toggled.is_set() && other.toggled.is_set())
            || (self.expanded.is_set() && other.expanded.is_set())
            || (self.selected.is_set() && other.selected.is_set())
            || both(self.is_enabled, other.is_enabled)
            || (self.focused.is_set() && other.focused.is_set())
    }
}

/// Everything said about one thing on screen.
///
/// Upstream's `SemanticsProperties`, narrowed to what the bridges below
/// actually deliver.
#[derive(Clone, Debug, Default)]
pub struct SemanticsProperties {
    /// What it is called. The first thing read out.
    pub label: String,
    /// What it currently says -- a field's text, a slider's number.
    pub value: String,
    /// What it is for, read after the label when a reader asks for more.
    pub hint: String,
    /// What the value would become if increased or decreased.
    pub increased_value: String,
    pub decreased_value: String,
    /// What the control's tip says -- upstream's `SemanticsConfiguration.tooltip`,
    /// which its `Semantics(tooltip: ...)` fills in and `raw_tooltip.dart` is
    /// the only widget in the framework to use.
    ///
    /// **Not part of the label**, and that is the whole reason it is a field
    /// of its own: the label is what the thing is and the tip is the extra
    /// sentence somebody wrote for whoever hovers over it. Appended to the
    /// label it would be read out as the control's name.
    ///
    /// It sat on `SemanticsConfiguration` and `SemanticsData` from the day
    /// those were ported and never on this struct, so nothing a widget could
    /// write reached either of them -- the shape of a rule with no producer
    /// these rounds keep turning up.
    pub tooltip: String,
    /// What kind of thing this is -- see [`SemanticsRole`]. `None` is
    /// upstream's default: a node that is described by its words and its flags
    /// and has no structural part to declare.
    pub role: SemanticsRole,
    /// The reading direction of everything said above: `label`, `value`,
    /// `hint`, and the two value forecasts.
    ///
    /// Upstream's `SemanticsConfiguration.textDirection`, which the
    /// `Semantics` widget defaults to the ambient `Directionality` and a
    /// paragraph takes from its own build (`paragraph.dart` sets it on the
    /// same line as the label), carried to the embedder as
    /// `SemanticsData.textDirection` and from there as
    /// `FlutterSemanticsNode2.text_direction`. `None` is that null: a node
    /// with nothing to read has no direction to read it in.
    pub text_direction: Option<TextDirection>,
    pub flags: SemanticsFlags,
    /// The actions this node accepts, as a bit set.
    pub actions: i32,
    /// How far down a scrollable the reader is, **in pixels**. `NaN` for a
    /// node that does not scroll, which is what upstream uses for the same "no
    /// answer".
    ///
    /// This used to say it was what a screen reader announces as "row 3 of
    /// 40". It is not, and the difference is the point of the two fields
    /// below: pixels are what the list moved by, items are what the reader is
    /// counting. "340 of 1200" is not a sentence anybody wants read aloud.
    pub scroll_position: f32,
    pub scroll_extent_max: f32,
    pub scroll_extent_min: f32,
    /// Which item of the list is the first one showing, and how many there are
    /// -- the two halves of "row 3 of 40".
    ///
    /// Upstream's `scrollIndex` and `scrollChildCount`, and they arrive from
    /// two different places, which is why they are two fields and not a pair.
    /// The **count** is declared: `scrollable.dart` writes
    /// `config.scrollChildCount = semanticChildCount` on the same line as the
    /// extents, because only the list knows how long it is. The **index** is
    /// discovered: `assembleSemanticsNode` walks the children it was handed
    /// and takes `firstVisibleIndex ??= child.indexInParent` off the first one
    /// that is not hidden, because only the walk knows which of them survived.
    ///
    /// `None` is upstream's null for both: a node that is not a list, or a
    /// list that never said how long it is.
    pub scroll_child_count: Option<i32>,
    pub scroll_index: Option<i32>,
}

/// Two of these are the same when a reader would be told the same thing.
///
/// Written out rather than derived because of the three scroll fields: they
/// hold `NaN` for "this does not scroll", which is what upstream's
/// `double? scrollPosition` becomes the moment it crosses to an embedder, and
/// two boxes that both do not scroll are saying the same thing. Derived
/// equality would call them different, and it is asked twice on every frame --
/// once by [`RenderSemantics::update_from`] to decide whether a label changed,
/// and once by [`flush`] to decide whether the platform needs telling. A
/// comparison that always answered "different" would defeat both gates while
/// looking like it worked.
impl PartialEq for SemanticsProperties {
    fn eq(&self, other: &SemanticsProperties) -> bool {
        /// Equal, or both of them "no answer".
        fn same(a: f32, b: f32) -> bool {
            a == b || (a.is_nan() && b.is_nan())
        }
        self.label == other.label
            && self.value == other.value
            && self.hint == other.hint
            && self.increased_value == other.increased_value
            && self.decreased_value == other.decreased_value
            && self.text_direction == other.text_direction
            && self.flags == other.flags
            && self.actions == other.actions
            && same(self.scroll_position, other.scroll_position)
            && same(self.scroll_extent_max, other.scroll_extent_max)
            && same(self.scroll_extent_min, other.scroll_extent_min)
            // Ordinary equality: these two say "no answer" with `None` rather
            // than with a value that is not equal to itself, so they need no
            // help. They are compared all the same -- a list whose first
            // showing row changed has something new to tell a reader even when
            // every label on the screen is the same as it was.
            && self.scroll_child_count == other.scroll_child_count
            && self.scroll_index == other.scroll_index
    }
}

impl SemanticsProperties {
    pub fn label(text: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            label: text.into(),
            scroll_position: f32::NAN,
            scroll_extent_max: f32::NAN,
            scroll_extent_min: f32::NAN,
            ..SemanticsProperties::default()
        }
    }

    /// Whether this node accepts `action`.
    pub fn has(&self, action: SemanticsAction) -> bool {
        self.actions & action as i32 != 0
    }

    /// Whether a reader would be told any words, which is whether a direction
    /// is worth carrying for them.
    ///
    /// Upstream's `SemanticsData` insists on a `textDirection` for exactly
    /// these fields and none other -- `label == '' || textDirection != null`,
    /// and the same assert for `value`, `increasedValue`, `decreasedValue`,
    /// and `hint` -- so those are the ones asked about here.
    fn reads_aloud(&self) -> bool {
        !(self.label.is_empty()
            && self.value.is_empty()
            && self.hint.is_empty()
            && self.increased_value.is_empty()
            && self.decreased_value.is_empty())
    }

    pub fn with_action(mut self, action: SemanticsAction) -> Self {
        self.actions |= action as i32;
        self
    }
}

/// Upstream `AccessibilityFocusBlockType`: how far a node keeps a screen
/// reader's focus out.
///
/// Upstream's doc is careful to say this "does not affect the actual keyboard
/// focus handled by [FocusNode]" -- it is only about the focus a reader moves
/// with its own gestures.
///
/// # A ladder, and merging takes the higher rung
///
/// Two nodes that merge have to end up with one answer, and upstream's
/// `_merge` is three ifs that come to "the stronger of the two":
/// `blockSubtree` beats everything, then `blockNode`, and otherwise both were
/// `none`. So this is the same shape as
/// [`crate::painting::RenderComparison`] -- a total order where merging is the
/// maximum, `None` is the identity and the top is absorbing.
///
/// The rung between the two blocking values is the one worth having: blocking
/// a node is not blocking its children. A container that should not itself be
/// stopped on, whose contents should still be reachable, is a real thing --
/// and a two-valued version of this type could not say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AccessibilityFocusBlockType {
    /// Not blocked.
    #[default]
    None,
    /// This node cannot take accessibility focus; its descendants still can.
    BlockNode,
    /// Neither this node nor anything under it.
    BlockSubtree,
}

impl AccessibilityFocusBlockType {
    pub const ALL: [AccessibilityFocusBlockType; 3] = [
        AccessibilityFocusBlockType::None,
        AccessibilityFocusBlockType::BlockNode,
        AccessibilityFocusBlockType::BlockSubtree,
    ];

    /// How much this blocks, as a number. `blockSubtree` is the top.
    pub fn strength(self) -> u8 {
        match self {
            AccessibilityFocusBlockType::None => 0,
            AccessibilityFocusBlockType::BlockNode => 1,
            AccessibilityFocusBlockType::BlockSubtree => 2,
        }
    }

    /// Upstream's `_merge`, which two nodes use when they become one.
    ///
    /// Written as the maximum rather than as upstream's three ifs, because
    /// that is what the three ifs say and it cannot be got half right: an
    /// ordering has one maximum, where three conditions can be edited into
    /// disagreeing with each other.
    pub fn merge(self, other: AccessibilityFocusBlockType) -> AccessibilityFocusBlockType {
        if other.strength() > self.strength() {
            other
        } else {
            self
        }
    }
}

/// Upstream `DebugSemanticsDumpOrder`: which order a semantics dump walks a
/// node's children in.
///
/// The two are reverses of one another, and each is the right one for a
/// different question:
///
/// * `traversalOrder` is the order a reader moves through the interface with
///   "next" and "previous". It is what a dump is usually read against, and
///   upstream's default everywhere `toStringDeep` and friends take this.
/// * `inverseHitTest` is the order children are *asked* whether they want a
///   touch: the last child first, then the second last, until one takes it.
///   Later children are drawn over earlier ones, so the last is on top and
///   has to be offered the touch first.
///
/// **They are reverses because painting and hit-testing are reverses** -- the
/// same rule [`crate::render::SliverPaintOrder`] carries for slivers, arriving
/// here from the other end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DebugSemanticsDumpOrder {
    /// The order a reader navigates in. Upstream's default.
    #[default]
    TraversalOrder,
    /// The order a touch is offered around, last child first.
    InverseHitTest,
}

impl DebugSemanticsDumpOrder {
    pub const ALL: [DebugSemanticsDumpOrder; 2] = [
        DebugSemanticsDumpOrder::TraversalOrder,
        DebugSemanticsDumpOrder::InverseHitTest,
    ];

    /// A node's children in this order.
    ///
    /// `children` is kept in traversal order, so the other one is its reverse
    /// rather than a second list -- which is what keeps the two from drifting
    /// apart when a child is added.
    pub fn children_of(self, children: &[i32]) -> Vec<i32> {
        match self {
            DebugSemanticsDumpOrder::TraversalOrder => children.to_vec(),
            DebugSemanticsDumpOrder::InverseHitTest => children.iter().rev().copied().collect(),
        }
    }
}

/// One node of the tree that goes to the platform.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsNode {
    /// Stable for as long as the widget that produced it keeps the same
    /// identifier. The platform keys its own accessibility nodes on this, and
    /// a node whose id changed is, to a screen reader, a different thing
    /// appearing where the old one was -- so it re-reads it.
    pub id: i32,
    pub properties: SemanticsProperties,
    /// In root coordinates, which is what every bridge below wants: the
    /// platform asks "what is at this point on the glass".
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    /// In paint order, which is reading order.
    pub children: Vec<i32>,
    /// Upstream's `isMergedIntoParent`: this node's meaning was folded into an
    /// ancestor's by [`SemanticsConfiguration::absorb`], so it has no separate
    /// existence for a reader.
    ///
    /// It is the guard in [`SemanticsNode::is_invisible`], and that is what it
    /// is for here.
    pub is_merged_into_parent: bool,
    /// Upstream's `indexInParent`, which counts differently from
    /// [`SemanticsNode::children`] -- see
    /// [`SemanticsNode::index_counts_the_children_that_are_not_here`].
    pub index_in_parent: Option<i32>,
}

impl SemanticsNode {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Upstream's `isInvisible`:
    /// `!isMergedIntoParent && (rect.isEmpty || transform.isZero())`.
    ///
    /// # It is a licence to drop, which is why the merged guard is there
    ///
    /// Upstream says what the predicate is *for*: "an invisible node can be
    /// safely dropped from the semantic tree without losing semantic
    /// information that is relevant for describing the content currently shown
    /// on screen."
    ///
    /// A merged node has no geometry of its own to be judged by -- its label
    /// is being read as part of its parent's -- so an empty rect says nothing
    /// about whether it is on screen. Dropping it would lose the words. **The
    /// guard is not an optimisation; it is the difference between a node with
    /// no size and a node with no meaning.**
    ///
    /// The port carries no zero-transform case: geometry here is a rectangle
    /// rather than a matrix, so a transform that collapses to nothing arrives
    /// as an empty rect and is caught by the first clause.
    pub fn is_invisible(&self) -> bool {
        !self.is_merged_into_parent && (self.width() <= 0.0 || self.height() <= 0.0)
    }

    /// Upstream on `indexInParent`: it "includes all semantic nodes, not just
    /// those currently in the child list".
    ///
    /// Upstream's own example: a scrollable with five children whose first two
    /// are not visible has three nodes, and the last of them still has index
    /// **4**. So the index is a position in the list the reader is being told
    /// about, not a position in the array that survived clipping -- which is
    /// what lets a screen reader say "item 5 of 5" while three nodes exist.
    ///
    /// Reading it off `children.len()` would say "item 3 of 3" and quietly
    /// tell the reader the list is shorter than it is.
    ///
    /// `kept` is where the surviving children sat in the full list; each keeps
    /// that position. The function is one line because the rule is -- what it
    /// is *not* is renumbering the survivors from zero, and the two agree
    /// whenever nothing was dropped, which is the case a careless test picks.
    pub fn indices_in_parent(kept: &[usize]) -> Vec<i32> {
        kept.iter().map(|&index| index as i32).collect()
    }
}

// -- What a render object says about itself -----------------------------------

/// One render object's answer to "what are you, for a reader".
///
/// Upstream's `SemanticsConfiguration`, filled in by
/// `describeSemanticsConfiguration`. Narrower here because there is no
/// merging step to configure: what an object says is what its node says.
pub struct SemanticsAnnotation {
    pub id: i32,
    pub properties: SemanticsProperties,
    pub on_action: Option<ActionHandler>,
    /// Whether an enclosing label already speaks for this.
    ///
    /// Set on text and nothing else. A button says "Save" and the text inside
    /// it says "Save"; read as two nodes a reader hears it twice, which is
    /// worse than hearing it once in the wrong voice. Upstream reaches the same
    /// place with `excludeSemantics` and `MergeSemantics` -- this is the common
    /// case of both, and it is the text that yields because the label is the
    /// one somebody chose.
    pub yields_to_a_label: bool,
}

impl SemanticsAnnotation {
    /// What [`crate::render::RenderBox::describe_semantics`] hands back for an
    /// annotation somebody wrote.
    pub fn new(
        id: i32,
        properties: SemanticsProperties,
        on_action: Option<ActionHandler>,
    ) -> SemanticsAnnotation {
        SemanticsAnnotation {
            id,
            properties,
            on_action,
            yields_to_a_label: false,
        }
    }

    /// What a paragraph hands back for text nobody annotated.
    ///
    /// The direction is taken with the words, because text is the thing a
    /// direction is *of*: upstream's `RenderParagraph` sets
    /// `config.textDirection = textDirection` on the same line as the label.
    /// A paragraph here does not carry a direction of its own yet -- the
    /// render-tree half of `directionality` is still landing -- so the
    /// ambient direction stands in for it, the same stand-in the shaper
    /// takes for the same reason, and
    /// [`SemanticsAnnotation::with_text_direction`] is the way in once the
    /// paragraph has one to give.
    pub fn text(id: i32, said: &str) -> SemanticsAnnotation {
        SemanticsAnnotation {
            id,
            properties: SemanticsProperties {
                text_direction: Some(crate::direction::current_direction()),
                ..SemanticsProperties::label(said)
            },
            on_action: None,
            yields_to_a_label: true,
        }
    }

    /// Says which way this node's words run, for a render object that knows.
    ///
    /// [`crate::render::RenderParagraph`] is the case: it will capture the
    /// ambient direction where it was built, the way it already captures the
    /// text scale, and hand it back here so the node says which way its text
    /// runs however late the walk asks. Everything else takes the ambient
    /// direction by default instead, which is upstream's `Semantics` widget
    /// defaulting the configuration with `Directionality.maybeOf`.
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.properties.text_direction = Some(direction);
        self
    }
}

// -- The frame's collection ---------------------------------------------------

#[derive(Default)]
struct Collector {
    /// Whether anything is listening. Nothing is collected otherwise.
    enabled: bool,
    nodes: Vec<SemanticsNode>,
    /// Indices into `nodes` for the annotations currently open, outermost
    /// last. This is what turns the paint recursion into a tree.
    open: Vec<usize>,
    /// The tree the platform is holding -- what the last [`flush`] handed
    /// over, or nothing if it has never been handed one.
    ///
    /// Upstream keeps the same thing, as the live `SemanticsNode` tree under
    /// `SemanticsOwner`, and for the same two reasons: a walk that comes out
    /// the same as this sends nothing, and a frame that is never walked leaves
    /// this standing as the answer -- which is right, because nothing that
    /// would have changed it happened.
    sent: Vec<SemanticsNode>,
    /// How many labelled annotations are open above the paint in progress.
    ///
    /// Text inside one of those is *what the annotation says*, and reading it
    /// again as its own node would make a screen reader say a button's name
    /// twice. Upstream reaches the same place with `excludeSemantics` and
    /// `MergeSemantics`; the rule here is the common case of both -- a node
    /// that gave itself a label speaks for everything under it.
    labelled_depth: usize,
    /// Indices of the nodes opened by a merging box, innermost last. While
    /// this is not empty, a descendant's label joins the node on top instead
    /// of opening one of its own -- upstream's
    /// `isMergingSemanticsOfDescendants`, which is why the merging box has to
    /// be a boundary: there has to be a node to fold *into*.
    merging: Vec<usize>,
    /// An index a box declared for whatever node opens next inside it, waiting
    /// to be claimed. Upstream's `SemanticsConfiguration.indexInParent`, which
    /// travels up through `absorb` to the node that ends up carrying it; here
    /// it travels down to the same node, because the walk builds nodes on the
    /// way in rather than merging configs on the way out.
    ///
    /// It is taken, not copied: the item's own node claims it, and the nodes
    /// beneath that one are parts of the item rather than further items.
    /// Whether each open node raised [`Collector::labelled_depth`], innermost
    /// last. `close` has to undo exactly what `open` did, and it cannot work
    /// that out by looking at the label again: a merging node's label **grows**
    /// while it is open, as its descendants fold into it.
    labelled: Vec<bool>,
    pending_index: Option<i32>,
    /// Where automatic ids for text nodes are handed out from.
    next_text_id: i32,
}

pub type ActionHandler = Rc<dyn Fn(SemanticsAction)>;

thread_local! {
    static COLLECTOR: RefCell<Collector> = RefCell::new(Collector::default());

    /// Whether anything has happened that the semantics tree would notice.
    ///
    /// This is upstream's `PipelineOwner._nodesNeedingSemanticsUpdate`, which
    /// is a set of the render objects to revisit. It is one boolean here for
    /// the reason [`crate::render::RenderRef::mark_needs_layout`] walks all
    /// the way to the root instead of stopping at a boundary: a set is only
    /// worth keeping if a descent can be *started* from what is in it, and
    /// there is no pipeline owner here to start one. So the answer this holds
    /// is "walk" or "do not walk", and the saving is the frames where it says
    /// not to -- which, for a screen that is being read rather than animated,
    /// is nearly all of them.
    ///
    /// Starts true: a tree nobody has walked yet has told nobody anything.
    static NEEDS_UPDATE: Cell<bool> = const { Cell::new(true) };
}

/// Whether the platform has said a screen reader is listening.
pub fn enabled() -> bool {
    COLLECTOR.with(|collector| collector.borrow().enabled)
}

/// Says the semantics tree is no longer what the platform is holding.
///
/// Upstream's `RenderObject.markNeedsSemanticsUpdate`, and like it this is
/// called from exactly the places that can change what a reader would hear:
///
/// * [`crate::render::RenderRef::layout`], on the path that actually lays out
///   -- upstream calls it on the line after `performLayout` for the same
///   reason, that a box which was just measured may have moved, resized, or
///   stopped existing, and a rectangle is made of all three.
/// * [`RenderSemantics::update_from`], when the annotation itself changed --
///   upstream's `RenderSemanticsAnnotations.set properties`.
/// * `RenderOpacity::update_from`, when the opacity crossed zero in either
///   direction -- upstream's `set opacity` marks on exactly that condition,
///   because a subtree that stopped being drawn stopped being describable
///   while a fade between two visible values changes nothing anybody hears.
/// * [`set_enabled`], when a reader arrives -- upstream's
///   `scheduleInitialSemantics`.
///
/// Cheap enough to call unconditionally: it is one thread-local boolean, where
/// upstream has to reach the owner to find out whether to bother.
///
/// Public for the reason upstream's is: a `RenderBox` written outside this
/// crate whose [`crate::render::RenderBox::describe_semantics`] answer changed
/// has the same thing to say, and no other way to say it.
pub fn mark_needs_update() {
    NEEDS_UPDATE.with(|needs| needs.set(true));
}

/// The platform saying an assistive technology arrived or left.
///
/// **This is not the switch, it is one client of it.** Upstream's binding holds
/// a single [`SemanticsHandle`] on the platform's behalf and disposes it when
/// the platform loses interest -- `_semanticsHandle ??= ensureSemantics()` one
/// way and `_semanticsHandle?.dispose()` the other -- so a reader leaving does
/// not switch semantics off for anyone else who asked. See [`SemanticsBinding`].
///
/// Called by the shell; idempotent, because the platform reports its state
/// rather than a change to it.
pub fn set_enabled(on: bool) {
    PLATFORM_HANDLE.with(|held| {
        if held.get() == on {
            return;
        }
        held.set(on);
        if on {
            // Deliberately leaked: the platform's interest lasts until the
            // platform says otherwise, which is the next call here, and there
            // is nowhere to keep the handle that outlives this function.
            // Upstream keeps it in a field for the same lifetime.
            std::mem::forget(SemanticsBinding::ensure_semantics());
        } else {
            // The matching release, by hand for the same reason.
            let mut handle = SemanticsHandle { released: false };
            handle.dispose();
        }
    });
}

/// What the collector does once the count says yes or no.
///
/// Split out from [`set_enabled`] because there are two ways in now -- the
/// platform's handle and anyone else's -- and both have to leave the collector
/// in the same state.
///
/// **Only ever called on an edge.** The callers gate on the count crossing zero
/// (`was == 0` going up, `remaining == 0` coming down), which is what upstream
/// gets from `ValueNotifier` only notifying on a change. A second guard in here
/// would be a line that cannot be wrong -- it was written and then removed once
/// a mutation showed nothing could observe it.
/// Turns collection on or off, and tells everyone who asked to be told.
///
/// # Why two of these are `try_with` and the rest are not
///
/// This runs from a drop -- [`SemanticsHandle`] releases itself -- and a drop
/// can run during thread teardown, when a thread-local may already have been
/// destroyed. `with` panics there, and a panic inside a drop inside teardown
/// **aborts the process** rather than failing anything.
///
/// It is not every thread-local that can be gone, which is why this is not a
/// blanket rule: a `Cell` initialised with `const` has nothing to drop, so no
/// destructor is registered for it and it stays reachable for the whole life of
/// the thread. `NEEDS_UPDATE` and `HANDLES` are those, and they are read
/// plainly. `COLLECTOR` and `ENABLED_LISTENERS` hold `Vec`s, are registered,
/// and are the two that can vanish underfoot.
fn apply_enabled(on: bool) {
    if COLLECTOR
        .try_with(|collector| {
            let mut collector = collector.borrow_mut();
            collector.enabled = on;
            if !on {
                collector.nodes.clear();
                collector.sent.clear();
            }
        })
        .is_err()
    {
        return;
    }
    // A reader that has just arrived has been told nothing, so everything is
    // news; a reader that has just left leaves an empty tree behind, so the
    // next one to arrive is not compared against a stale one.
    mark_needs_update();
    let Ok(listeners) = ENABLED_LISTENERS.try_with(|listeners| -> Vec<Rc<dyn Fn(bool)>> {
        listeners.borrow().iter().flatten().cloned().collect()
    }) else {
        return;
    };
    for listener in listeners {
        listener(on);
    }
}

// -- Who is asking for semantics ----------------------------------------------

/// Upstream `SemanticsBinding`, reduced to the part that decides whether the
/// tree is built at all.
///
/// # There is one mechanism, and the platform is only one of its clients
///
/// It is tempting to read `semanticsEnabled` as "the platform turned a screen
/// reader on", and the crate read it that way until now: a boolean the shell
/// set. Upstream's is a **count of outstanding handles**, asserted equal to it
/// on every read -- `assert(_semanticsEnabled.value == (_outstandingHandles >
/// 0))` -- and the platform's own interest is expressed by the binding holding
/// one handle and disposing it, exactly as any other client would.
///
/// The difference shows the moment there are two clients. With a boolean, an
/// inspector that turned semantics on to read the tree is switched off again
/// the next time the platform says no reader is attached, halfway through the
/// inspection. With a count, the platform releasing its handle leaves the
/// inspector's standing.
///
/// So [`set_enabled`] is no longer the switch. It is the platform taking and
/// releasing its one handle, and the switch is what the count says.
pub struct SemanticsBinding;

impl SemanticsBinding {
    /// Upstream's `ensureSemantics()`: asks for the tree to be built and hands
    /// back the reason it is.
    ///
    /// The tree is collected while any handle is alive. Drop the handle and, if
    /// it was the last, collection stops.
    pub fn ensure_semantics() -> SemanticsHandle {
        let was = HANDLES.with(|handles| {
            let count = handles.get();
            handles.set(count + 1);
            count
        });
        if was == 0 {
            apply_enabled(true);
        }
        SemanticsHandle { released: false }
    }

    /// Upstream's `debugOutstandingSemanticsHandles`.
    pub fn outstanding_handles() -> usize {
        HANDLES.with(|handles| handles.get())
    }

    /// Upstream's `addSemanticsEnabledListener`.
    ///
    /// Answers a token to remove it with, since a Rust closure has no identity
    /// to compare -- upstream removes by passing the same function object back,
    /// which only works because a Dart method tear-off is stable.
    pub fn add_enabled_listener(listener: impl Fn(bool) + 'static) -> usize {
        ENABLED_LISTENERS.with(|listeners| {
            let mut listeners = listeners.borrow_mut();
            let token = listeners.len();
            listeners.push(Some(Rc::new(listener)));
            token
        })
    }

    /// Upstream's `removeSemanticsEnabledListener`.
    pub fn remove_enabled_listener(token: usize) -> bool {
        ENABLED_LISTENERS.with(|listeners| {
            let mut listeners = listeners.borrow_mut();
            match listeners.get_mut(token) {
                Some(slot) => slot.take().is_some(),
                None => false,
            }
        })
    }

    /// Forgets every handle and listener. For tests, which share a thread and
    /// would otherwise inherit each other's counts.
    pub fn reset_for_tests() {
        HANDLES.with(|handles| handles.set(0));
        PLATFORM_HANDLE.with(|held| held.set(false));
        ENABLED_LISTENERS.with(|listeners| listeners.borrow_mut().clear());
        apply_enabled(false);
    }
}

/// Upstream `SemanticsHandle`: a client's standing interest in the semantics
/// tree.
///
/// # Dropping is disposing
///
/// Upstream's has a `dispose()` that must be called by hand, and a debug
/// allocation tracker to catch the ones that are not. Here the drop does it, so
/// a handle that goes out of scope has released its interest and there is
/// nothing to leak and nothing to assert about.
///
/// [`SemanticsHandle::dispose`] is kept for callers reading across from
/// upstream, and for the case where the release has to happen before the end of
/// a scope. It is idempotent, and the drop after it does nothing.
pub struct SemanticsHandle {
    released: bool,
}

impl SemanticsHandle {
    /// Upstream's `dispose()`. Calling it twice is not an error; the second
    /// call has nothing left to release.
    pub fn dispose(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let remaining = HANDLES.with(|handles| {
            let count = handles.get().saturating_sub(1);
            handles.set(count);
            count
        });
        if remaining == 0 {
            apply_enabled(false);
        }
    }
}

impl Drop for SemanticsHandle {
    fn drop(&mut self) {
        self.dispose();
    }
}

thread_local! {
    /// Upstream's `_outstandingHandles`.
    static HANDLES: Cell<usize> = const { Cell::new(0) };
    /// Whether the platform is currently holding one. Upstream's
    /// `_semanticsHandle`, which the binding keeps and nulls.
    static PLATFORM_HANDLE: Cell<bool> = const { Cell::new(false) };
    #[allow(clippy::type_complexity)]
    static ENABLED_LISTENERS: RefCell<Vec<Option<Rc<dyn Fn(bool)>>>> =
        const { RefCell::new(Vec::new()) };
}

/// The tree the platform is holding.
///
/// Upstream's `SemanticsOwner.rootSemanticsNode` and what hangs from it. This
/// is the answer between frames as well as during them: a frame that found
/// nothing marked did not walk, and what it did not walk is still true.
pub fn tree() -> Vec<SemanticsNode> {
    COLLECTOR.with(|collector| collector.borrow().sent.clone())
}

/// The view's own node, which everything painted into it hangs from.
///
/// Upstream this is `RenderView`'s semantics node, and it is always zero. It
/// exists here for the reason it exists there -- a screen reader is handed one
/// tree, not a heap of unrelated ones -- and for one more that upstream never
/// had to think about: the order the nodes reach a platform is lost on the way
/// (`SemanticsNodeUpdates` is a map, on this branch and upstream both), so the
/// order a reader meets them in has to be carried by a parent's child list.
/// Without a parent above them, the top-level nodes have nowhere to carry it.
pub const ROOT_ID: i32 = 0;

/// Brings the semantics tree up to date, and returns it if the platform needs
/// telling.
///
/// This is upstream's `PipelineOwner.flushSemantics` followed by
/// `SemanticsOwner.sendSemanticsUpdate`, and it declines to do the work at all
/// of the same three places they do -- see "The three gates" in the module
/// documentation. `None` means there is nothing to send, and it is the answer
/// on nearly every frame: nobody is reading, or nothing marked itself, or the
/// walk found the tree the platform already has. Only the last of those costs
/// a walk.
///
/// `size` is the view, and becomes [`ROOT_ID`]'s rectangle. The tree must be
/// laid out already: every offset this reads was written during layout.
pub fn flush(size: Size, root: &dyn RenderBox) -> Option<Vec<SemanticsNode>> {
    // Gate one: nobody is reading. Upstream's `if (_semanticsOwner == null)`.
    if !enabled() {
        return None;
    }
    // Gate two: nothing that a reader would notice has happened since the last
    // walk. Upstream takes the render objects out of
    // `_nodesNeedingSemanticsUpdate` here and revisits those; this has one
    // boolean rather than a list, so it either walks or it does not.
    if !NEEDS_UPDATE.with(|needs| needs.replace(false)) {
        return None;
    }
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.nodes.clear();
        collector.open.clear();
        collector.labelled_depth = 0;
        // Belt and braces, and known to be: a mutation that deletes either of
        // the next two lines turns nothing red. `describe_subtree` balances
        // both of them -- the merging stack is pushed and popped around one
        // recursive call, and the pending index is restored by a drop guard on
        // every way out -- so the only way to leave one stale is a panic
        // mid-walk, after which nobody flushes again. They stay because the
        // three lines above are the same promise (a walk starts from nothing),
        // and what they would do if they ever were stale is not small: a
        // merging stack that survived would fold the next frame's whole tree
        // into a node that no longer exists, and a pending index that survived
        // would hand the next frame's first node a position from the last one.
        collector.merging.clear();
        collector.labelled.clear();
        collector.pending_index = None;
    });
    // Opened before the walk and closed after it, so that everything the walk
    // finds lands inside it -- in paint order, which is reading order.
    let opened = open(
        ROOT_ID,
        SemanticsProperties::label(""),
        (0.0, 0.0, size.width, size.height),
    );
    describe_subtree(root, Offset::ZERO, Clips::UNCLIPPED);
    if let Some(index) = opened {
        close(index);
    }
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.open.clear();
        collector.labelled_depth = 0;
        // Gate three: the walk happened and came out the same. Upstream's
        // `if (_dirtyNodes.isEmpty) return;`. A frame that relaid out anything
        // at all arrives here -- a growing ripple, a settling scroll that has
        // stopped moving, a rebuild that changed only a colour -- and most of
        // them have nothing to say that was not said last time.
        if collector.nodes == collector.sent {
            return None;
        }
        collector.sent = std::mem::take(&mut collector.nodes);
        Some(collector.sent.clone())
    })
}

/// Runs a closure when it goes out of scope, however it goes.
///
/// [`describe_subtree`] leaves by three doors -- off the end, the clip drop,
/// and the merging branch's early return -- and what it puts back on the way
/// out has to be put back through all three. A guard says that once; three
/// hand-written restores would be three chances to add a fourth door and
/// forget one, and the symptom would be a *sibling* silently taking an index
/// meant for a subtree that was thrown away.
struct OnDrop<F: FnMut()>(F);

impl<F: FnMut()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// One render object and everything under it, at `offset` from the root and
/// inside `clips`.
///
/// Upstream's `_RenderObjectSemantics` walk. The recursion is the tree: what a
/// node opens stays open until its children have been described, so the nesting
/// of the render tree becomes the nesting a reader is handed.
fn describe_subtree(render: &dyn RenderBox, offset: Offset, clips: Clips) {
    if render.blocks_previously_painted_semantics() {
        block_previously_painted();
    }

    // An index this box declared for its subtree, offered until a node takes
    // it. The previous value is put back afterwards rather than cleared: two
    // indexed boxes can nest (a table cell inside a row), and the outer one's
    // offer must survive the inner one's subtree -- an unclaimed offer that
    // vanished would silently renumber the outer list.
    let displaced = render.index_in_parent_for_semantics().map(|index| {
        COLLECTOR.with(|collector| collector.borrow_mut().pending_index.replace(index))
    });
    let _restore = OnDrop(move || {
        if let Some(previous) = displaced {
            COLLECTOR.with(|collector| collector.borrow_mut().pending_index = previous);
        }
    });

    // A merging box is a boundary and a target at once: it opens a node with
    // no label of its own, and its descendants fold their labels into that
    // node rather than opening any. Upstream pairs `isSemanticBoundary` with
    // `isMergingSemanticsOfDescendants` for exactly this reason -- there has
    // to be a node to fold into.
    if render.merges_descendant_semantics() {
        let size = render.size();
        let bounds = Rect::xywh(offset.dx, offset.dy, size.width, size.height);
        // The same question the annotated branch below asks, for the same
        // reason: a folded node is still a node, and one with no rect on the
        // glass is a stop nobody can point at. Its descendants leave with it --
        // they have no nodes of their own to be left behind in, which is what
        // folding means.
        let Some(rect) = clips.applied_to(bounds) else {
            return;
        };
        // Opened with **no label**, and given one immediately afterwards.
        //
        // The words matter but the *depth* must not: a merging node that
        // counted as "something above already speaks" would silence the very
        // text it opened to gather. Round 369 fixed that by switching the
        // suppression off inside a merge altogether, and that was too broad --
        // a `Label` folded into a banner then said its words twice, once from
        // its own annotation and once from the `Text` underneath it, which is
        // what `spoken_census` caught in round 380.
        let mut properties = render
            .merged_node_properties()
            .unwrap_or_else(|| SemanticsProperties::label(""));
        let own_label = std::mem::take(&mut properties.label);
        // A fold that offers an action names itself; one that only gathers
        // words takes an invented number. The difference is that an action
        // comes *back* from the engine by number, so a node that can be
        // pressed cannot be renumbered between walks.
        let id = match render.merged_node_action() {
            Some((id, _)) => id,
            None => take_text_id(),
        };
        let merged = open(
            id,
            properties,
            (rect.left, rect.top, rect.right, rect.bottom),
        );
        if let Some(index) = merged {
            COLLECTOR.with(|collector| {
                collector.borrow_mut().nodes[index].properties.label = own_label;
            });
        }
        if let Some(index) = merged {
            COLLECTOR.with(|collector| collector.borrow_mut().merging.push(index));
            render.visit_children_for_semantics(&mut |child, child_offset| {
                describe_subtree(
                    child,
                    offset.plus(child_offset),
                    clips.refined_by(render, child, offset),
                );
            });
            COLLECTOR.with(|collector| {
                collector.borrow_mut().merging.pop();
            });
            close(index);
            return;
        }
    }

    let mut folded_label = false;
    let opened = match render.describe_semantics() {
        // Text that something above already speaks for. Its children are still
        // walked -- suppressing what a node says is not suppressing what is
        // under it -- though a paragraph has none.
        //
        // **Not inside a merge**, and that exception is the whole point of a
        // merge: folding exists to collect the words below, so a merging node
        // that happens to carry a label of its own would otherwise swallow the
        // very text it was opened to gather. A tab is the case in hand -- its
        // node says "Tab 3 of 5" and its words come from the `Text` inside it,
        // and with this rule missing a reader heard the position and never the
        // tab. Round 349's own tests all used a merging node with an empty
        // label, so nothing noticed.
        Some(annotation) if annotation.yields_to_a_label && inside_labelled() => None,
        Some(annotation) => {
            let size = render.size();
            let bounds = Rect::xywh(offset.dx, offset.dy, size.width, size.height);
            match clips.applied_to(bounds) {
                // Nothing of it is inside the clips, so nothing of it is on the
                // glass. Upstream drops the node out of its parent's children
                // (`children.removeWhere(shouldDrop)`), and everything under it
                // leaves with the node -- so the subtree is not walked.
                None => return,
                Some(rect) => {
                    // An annotation with words **speaks for what is under it**,
                    // whether it becomes a node or is folded into one. Folded,
                    // `open` returns `None` and there is no `close` to pair
                    // with, so the depth is raised here and lowered by the
                    // guard below.
                    //
                    // Without this a `Label` inside a merging box said its
                    // words twice -- once from its own annotation, folded in,
                    // and once from the `Text` underneath it, which no longer
                    // had anything above it to yield to. `spoken_census` caught
                    // that on a `MaterialBanner`.
                    folded_label = inside_merge() && !annotation.properties.label.is_empty();
                    if folded_label {
                        COLLECTOR.with(|collector| collector.borrow_mut().labelled_depth += 1);
                    }
                    open(
                        annotation.id,
                        annotation.properties,
                        (rect.left, rect.top, rect.right, rect.bottom),
                    )
                }
            }
        }
        None => None,
    };
    let _lower = OnDrop(move || {
        if folded_label {
            COLLECTOR.with(|collector| {
                let mut collector = collector.borrow_mut();
                collector.labelled_depth = collector.labelled_depth.saturating_sub(1);
            });
        }
    });
    render.visit_children_for_semantics(&mut |child, child_offset| {
        describe_subtree(
            child,
            offset.plus(child_offset),
            clips.refined_by(render, child, offset),
        );
    });
    if let Some(index) = opened {
        close(index);
    }
}

// -- The clips the walk carries ------------------------------------------------

/// The clip rectangles that apply below a render object, in root coordinates.
///
/// Upstream's `_SemanticsGeometry`, which holds one `paintClipRect` and one
/// `semanticsClipRect` per semantics node and rebuilds both by walking the
/// render chain between two nodes, transforming as it goes. Here the walk is
/// already in root coordinates -- that is what the offsets it carries are --
/// so the same two rectangles are simply carried down it, each contributor
/// translating its own answer from its own coordinates.
#[derive(Clone, Copy, Debug, Default)]
struct Clips {
    paint: Option<Rect>,
    semantics: Option<Rect>,
}

impl Clips {
    /// No clip at all, which is what the walk starts with. Upstream's
    /// `_SemanticsGeometry.root`: the view's own node is clipped by nothing.
    const UNCLIPPED: Clips = Clips {
        paint: None,
        semantics: None,
    };

    /// What `parent`'s answers about `child` leave of these clips.
    ///
    /// One link of the accumulation loop in
    /// `_SemanticsGeometry.computeChildGeometry`, with the translation that
    /// upstream needs a `Matrix4` for done by the walk instead. Paint clips
    /// intersect all the way down. A semantics clip *replaces* what was
    /// carried -- upstream's `localSemanticsClipInParent ??
    /// semanticsClipRect?.intersect(...)`, where the clip nearest the node
    /// wins -- and the paint clips below it narrow the replacement further.
    fn refined_by(
        self,
        parent: &dyn RenderBox,
        child: &dyn RenderBox,
        parent_offset: Offset,
    ) -> Clips {
        let at = |clip: Rect| {
            Rect::ltrb(
                clip.left + parent_offset.dx,
                clip.top + parent_offset.dy,
                clip.right + parent_offset.dx,
                clip.bottom + parent_offset.dy,
            )
        };
        let paint = parent.describe_approximate_paint_clip(child).map(at);
        let semantics = parent.describe_semantics_clip(child).map(at);
        Clips {
            paint: match (self.paint, paint) {
                (Some(carried), Some(local)) => Some(intersect(carried, local)),
                (carried, local) => carried.or(local),
            },
            semantics: semantics.or_else(|| {
                self.semantics.map(|carried| match paint {
                    Some(local) => intersect(carried, local),
                    None => carried,
                })
            }),
        }
    }

    /// `bounds`, cut down to what the clips leave of it, or `None` when
    /// nothing a reader could touch survives.
    ///
    /// The tail of `_SemanticsGeometry.computeChildGeometry`: the rect is cut
    /// by the semantics clip first (`semanticsClipRect?.intersect(semanticBounds)`)
    /// and by the paint clip second. Empty after the semantics clip is
    /// upstream's `isInvisible`, dropped from the tree. Empty after the paint
    /// clip but not before it is upstream's `hidden` -- kept in the tree there,
    /// for the readers that scroll to a node they have been told about. This
    /// bridge has no hidden flag to carry, and reporting the uncut rectangle
    /// puts coordinates outside the window onto the glass, so it is dropped
    /// too.
    ///
    /// # An empty rect is dropped wherever it came from
    ///
    /// Upstream's `isInvisible` is `rect.isEmpty` and does not care which clip,
    /// if any, made it empty; `_RenderObjectSemantics.shouldDrop` is that
    /// predicate and `children.removeWhere(shouldDrop)` is where it lands. The
    /// reason upstream gives is the one that matters: an invisible node "can be
    /// safely dropped ... without losing semantic information that is relevant
    /// for describing the content currently shown on screen". A stop a reader
    /// cannot point at, focus, or draw a highlight around is not a stop.
    ///
    /// This used to have a shortcut -- with neither clip present the rect was
    /// reported as it lay, empty or not -- and the reason written down for it
    /// was that "an empty rect usually means the test engine shaped no text,
    /// and a paragraph that says something is still worth reading". **That
    /// reason has since expired.** The stub paragraph "returned a hard zero
    /// until now, so every string in the crate measured nought by nought" (see
    /// [`crate::engine_test_stubs`]) and was given a width model; text measures
    /// wider the longer it is. So the shortcut was keeping a whole class of
    /// nodes alive to protect a case that no longer occurs, and doing it
    /// asymmetrically: the *same* empty rect was dropped when it happened to
    /// fall inside a clip and shipped when it did not.
    fn applied_to(&self, bounds: Rect) -> Option<Rect> {
        let mut rect = self
            .semantics
            .map_or(bounds, |clip| intersect(bounds, clip));
        if let Some(clip) = self.paint {
            let painted = intersect(rect, clip);
            if is_empty(painted) && !is_empty(rect) {
                return None; // `hidden`, upstream; dropped here.
            }
            rect = painted;
        }
        (!is_empty(rect)).then_some(rect)
    }
}

/// Upstream's `Rect.intersect`: the overlap, or an inside-out rectangle where
/// the two do not meet -- which is empty, and left that way there too.
fn intersect(a: Rect, b: Rect) -> Rect {
    Rect::ltrb(
        a.left.max(b.left),
        a.top.max(b.top),
        a.right.min(b.right),
        a.bottom.min(b.bottom),
    )
}

/// Upstream's `Rect.isEmpty`.
fn is_empty(rect: Rect) -> bool {
    rect.width() <= 0.0 || rect.height() <= 0.0
}

/// Delivers an action the platform asked for.
///
/// Returns whether anything took it. Upstream this is
/// `SemanticsOwner.performAction`, and the same rule applies: an action for a
/// node that has since gone is not an error, it is a race with the reader.
///
/// The handler is fetched from the render tree rather than from a list kept by
/// the last walk, and that is not a detail. A rebuild that changes only a
/// closure changes nothing measured and nothing drawn, so nothing marks itself
/// and no walk happens -- which is the whole point of
/// [`mark_needs_update`] -- and a remembered handler would then be last
/// build's. The live object always has this build's, because
/// [`RenderSemantics::update_from`] took it. Upstream never has to choose:
/// its `SemanticsNode` holds the render object, so reaching one reaches the
/// other.
pub fn perform_action(root: &dyn RenderBox, node_id: i32, action: SemanticsAction) -> bool {
    match find_handler(root, node_id) {
        Some(handler) => {
            handler(action);
            true
        }
        None => false,
    }
}

/// The handler the node with this id offered, if it is still on screen.
///
/// Walks the same children [`flush`] walks and under the same clips, so a node
/// a reader cannot have been told about -- one under a fully transparent
/// subtree, or one the clips cut away entirely -- cannot be activated either.
fn find_handler(render: &dyn RenderBox, node_id: i32) -> Option<ActionHandler> {
    find_handler_in(render, node_id, Offset::ZERO, Clips::UNCLIPPED)
}

/// The walk behind [`find_handler`], which is [`describe_subtree`] again with a
/// different thing collected: the same clips, the same dropping of what they
/// empty, because a node that was never in the tree is not a node a reader can
/// name.
fn find_handler_in(
    render: &dyn RenderBox,
    node_id: i32,
    offset: Offset,
    clips: Clips,
) -> Option<ActionHandler> {
    if let Some(annotation) = render.describe_semantics() {
        let size = render.size();
        clips.applied_to(Rect::xywh(offset.dx, offset.dy, size.width, size.height))?;
        if annotation.id == node_id {
            return annotation.on_action;
        }
    }
    // A folded node answers here too. It is a second place to look because a
    // fold is not an annotation -- it has no words of its own to describe, only
    // an identifier and somewhere to send the press. Missing this branch, an
    // ink well would publish an action the engine could never deliver.
    if let Some((id, handler)) = render.merged_node_action() {
        let size = render.size();
        clips.applied_to(Rect::xywh(offset.dx, offset.dy, size.width, size.height))?;
        if id == node_id {
            return Some(handler);
        }
    }
    let mut found = None;
    render.visit_children_for_semantics(&mut |child, child_offset| {
        if found.is_none() {
            found = find_handler_in(
                child,
                node_id,
                offset.plus(child_offset),
                clips.refined_by(render, child, offset),
            );
        }
    });
    found
}

/// Whether the walk is inside something that already has a label.
fn inside_labelled() -> bool {
    COLLECTOR.with(|collector| collector.borrow().labelled_depth > 0)
}

/// Whether the walk is inside a box that is folding its descendants.
fn inside_merge() -> bool {
    COLLECTOR.with(|collector| !collector.borrow().merging.is_empty())
}

/// Hands out an identifier for a node that has none of its own.
///
/// Text is the case: a paragraph is a render object built inside a closure and
/// has no identifier anybody chose. Because render objects now outlive the
/// frame, an id taken once is stable for as long as the paragraph is -- which
/// is exactly as long as a screen reader should go on believing it is the same
/// thing.
pub(crate) fn take_text_id() -> i32 {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        let id = TEXT_BASE.wrapping_add(collector.next_text_id);
        collector.next_text_id = collector.next_text_id.wrapping_add(1) & (TEXT_BASE - 1);
        id
    })
}

/// Where text node ids start. The third of the three ranges; see [`AUTO_BASE`].
const TEXT_BASE: i32 = 2 << 28;

/// What goes between two labels folded into one node.
///
/// Upstream has no name for this: it is the bare `'\n'` inside
/// `SemanticsConfiguration._concatAttributedStrings`, which is the one place a
/// merge joins two labels. Naming it here is not decoration -- the live merge in
/// [`open`] and the modelled merge in [`concat_attributed_string`] are two
/// separate pieces of code doing the same join, and a separator written out
/// twice is a separator that can drift once. A reader hears the difference
/// immediately: `"Save Ctrl S"` is one phrase, `"Save\nCtrl S"` is two.
const MERGED_LABEL_SEPARATOR: &str = "\n";

/// Opens a node during the walk, returning its index.
fn open(id: i32, properties: SemanticsProperties, rect: (f32, f32, f32, f32)) -> Option<usize> {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        if !collector.enabled {
            return None;
        }
        // Inside a merge, a descendant says its piece into the merging node
        // rather than becoming a stop of its own.
        if let Some(&into) = collector.merging.last() {
            // Upstream's `SemanticsNode.updateWith`: `flags =
            // flags.merge(node._flags)`. **A folded node makes the claims of
            // everything folded into it** -- it is not a container over them,
            // it replaced them, so a button folded into a row leaves a row
            // that can be pressed and nothing that says so would be a stop
            // saying only its words.
            //
            // Carried by [`SemanticsFlags::merge`], which had no caller at all
            // until this line: the walk folded the label, then the tooltip,
            // then the role, and never the flags. Giving it one is also what
            // made its two tristate rules observable, and both were wrong --
            // see [`SemanticsTristate::merge`] and
            // [`SemanticsCheckState::merge`].
            if properties.flags != SemanticsFlags::default() {
                let node = &mut collector.nodes[into];
                node.properties.flags = node.properties.flags.merge(&properties.flags);
            }
            // Upstream's merge, same rule as the tip and for the same
            // reason: `if (role == SemanticsRole.none) role = node._role`. A
            // merging node is one thing, so it has one kind, and the nearest
            // claim wins.
            if properties.role.is_set() {
                let node = &mut collector.nodes[into];
                if !node.properties.role.is_set() {
                    node.properties.role = properties.role;
                }
            }
            // Upstream's `SemanticsConfiguration.absorb`: the tip is taken
            // only if the merging node has none. Unlike the label, two tips
            // are not joined -- a tip is one sentence about one control, and
            // a pair run together would be a sentence about neither.
            if !properties.tooltip.is_empty() {
                let node = &mut collector.nodes[into];
                if node.properties.tooltip.is_empty() {
                    node.properties.tooltip = properties.tooltip;
                }
            }
            let label = properties.label;
            if !label.is_empty() {
                let node = &mut collector.nodes[into];
                if node.properties.label.is_empty() {
                    node.properties.label = label;
                } else {
                    node.properties.label.push_str(MERGED_LABEL_SEPARATOR);
                    node.properties.label.push_str(&label);
                }
            }
            return None;
        }
        let index = collector.nodes.len();
        let claimed = collector.pending_index.take();
        collector.nodes.push(SemanticsNode {
            id,
            properties,
            left: rect.0,
            top: rect.1,
            right: rect.2,
            bottom: rect.3,
            children: Vec::new(),
            is_merged_into_parent: false,
            // Not knowable here. Upstream sets `indexInParent` in the *render*
            // layer -- `RenderIndexedSemantics` and `RenderTable` put it on the
            // `SemanticsConfiguration`, and the node reads it back off that --
            // because by the time a node reaches this walk its dropped
            // siblings are already gone and their positions with them.
            //
            // A value computed here from `children.len()` would be the
            // renumbering the rule exists to forbid. So this is never counted,
            // only claimed: `RenderIndexedSemanticsBox` puts one in
            // `pending_index` on the way down and the first node opened inside
            // it takes it, which is the item's own node.
            index_in_parent: claimed,
        });
        if let Some(parent) = collector.open.last().copied() {
            collector.nodes[parent].children.push(id);
        }
        collector.open.push(index);
        let raised = !collector.nodes[index].properties.label.is_empty();
        if raised {
            collector.labelled_depth += 1;
        }
        collector.labelled.push(raised);
        Some(index)
    })
}

/// Upstream's `RenderBlockSemantics`: everything described so far under the
/// node currently open goes away.
///
/// # Why a truncation is the whole of it
///
/// `nodes` is filled in paint order and the walk is depth first, so once a
/// parent has opened at index `p`, its children and their descendants occupy
/// exactly `p + 1 ..` and nothing else does. Whatever was painted before the
/// blocker, under that parent, is that contiguous run -- so dropping it is a
/// truncation and a cleared child list, not a search.
///
/// The parent's own node stays: blocking hides what was painted *below a
/// common boundary*, and the boundary itself is not below itself.
///
/// The empty-stack arm is **unreachable today** and is written as a return
/// rather than an `unwrap`: [`flush`] opens the root node before the walk and
/// closes it after, so a blocker always has something open above it.
/// Replacing the guard with `unwrap_or(0)` leaves the suite green, which is
/// the honest reason it is recorded here rather than tested -- the arm exists
/// so that a future caller walking a subtree on its own gets nothing taken
/// away instead of an index into an empty list.
fn block_previously_painted() {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        if !collector.enabled {
            return;
        }
        let Some(parent) = collector.open.last().copied() else {
            return;
        };
        collector.nodes.truncate(parent + 1);
        collector.nodes[parent].children.clear();
    });
}

fn close(index: usize) {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        // A list learns which of its rows is showing only once its rows have
        // been walked, which is here and not where it described itself.
        //
        // Upstream's `assembleSemanticsNode` does the same thing at the same
        // moment: `firstVisibleIndex ??= child.indexInParent`, skipping the
        // children whose `isHidden` flag is set. The skip is already done by
        // the time the loop below runs -- a node that was not on the glass
        // never opened at all (see [`Clips::applied_to`]) -- so what survives
        // in `nodes` *is* upstream's not-hidden list.
        //
        // The walk is depth first and nodes are pushed in the order they are
        // opened, so a node at `index` owns exactly `index + 1..`; the first
        // one there carrying an index is the first showing row. This is the
        // same contiguity `block_previously_painted` relies on.
        if !collector.nodes[index].properties.scroll_position.is_nan() {
            let first = collector.nodes[index + 1..]
                .iter()
                .find_map(|node| node.index_in_parent);
            collector.nodes[index].properties.scroll_index = first;
        }
        if collector.labelled.pop().unwrap_or(false) {
            collector.labelled_depth = collector.labelled_depth.saturating_sub(1);
        }
        // Closes down to and including `index`. A child that opened and never
        // closed -- a paint that unwound -- would otherwise leave the stack
        // wrong for everything after it.
        while let Some(top) = collector.open.pop() {
            if top == index {
                break;
            }
        }
    });
}

// -- The render object --------------------------------------------------------

/// Annotates its child, and reports where the child ended up.
///
/// Upstream's `RenderSemanticsAnnotations`. It draws nothing and changes no
/// layout: it is the same box as its child, with something said about it.
pub struct RenderSemantics {
    id: i32,
    properties: SemanticsProperties,
    on_action: Option<ActionHandler>,
    child: BoxedRender,
    size: Size,
}

impl RenderSemantics {
    pub fn new(
        id: i32,
        properties: SemanticsProperties,
        child: impl RenderBox + 'static,
    ) -> RenderSemantics {
        // The direction is taken here rather than at describe time because
        // construction is the one moment the ambient direction is this
        // annotation's: the render walk pushes it around the subtree while
        // the object is being built, and the semantics walk that asks what
        // this says runs long after it has popped. Upstream's `Semantics`
        // widget does the same defaulting in its own build
        // (`textDirection ?? Directionality.maybeOf(context)`), and its
        // `SemanticsData` insists on the result -- a node that says anything
        // says which way it runs. A node with nothing to read keeps `None`,
        // which crosses as "unknown".
        let properties = if properties.reads_aloud() {
            SemanticsProperties {
                text_direction: Some(crate::direction::current_direction()),
                ..properties
            }
        } else {
            properties
        };
        RenderSemantics {
            id,
            properties,
            on_action: None,
            child: crate::render::RenderRef::new(child),
            size: Size::ZERO,
        }
    }

    pub fn with_on_action(mut self, handler: impl Fn(SemanticsAction) + 'static) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }
}

impl RenderBox for RenderSemantics {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<crate::render::UpdateEffect> {
        use crate::render::UpdateEffect;
        let fresh = fresh.as_any_mut().downcast_mut::<RenderSemantics>()?;
        // Nothing here is measured and nothing is drawn, so the effect this
        // reports is about the child alone -- which is why a changed label has
        // to say so itself. Upstream's `RenderSemanticsAnnotations.set
        // properties` ends in `markNeedsSemanticsUpdate()` for the same
        // reason, and this is the only kind of change in the whole framework
        // that neither layout nor paint would have noticed on its behalf.
        //
        // The handler is deliberately not part of that comparison. Two
        // closures cannot be told apart -- every build makes a fresh `Rc` --
        // so comparing them would mark every frame, and not comparing them
        // would be wrong if anything remembered the old one. Nothing does:
        // `perform_action` reads the handler off this object at the moment the
        // reader asks, and `self.on_action` below is always this build's.
        let changed = self.id != fresh.id || self.properties != fresh.properties;
        self.id = fresh.id;
        self.properties = fresh.properties.clone();
        self.on_action = fresh.on_action.take();
        if changed {
            mark_needs_update();
        }
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn describe_semantics(&self) -> Option<SemanticsAnnotation> {
        Some(SemanticsAnnotation::new(
            self.id,
            self.properties.clone(),
            self.on_action.as_ref().map(Rc::clone),
        ))
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    fn hit_test(&self, position: Offset, result: &mut crate::render::HitTestResult) -> bool {
        self.child.hit_test(position, result)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

// -- The widget ---------------------------------------------------------------

/// Says something about a subtree, for a reader who cannot see it.
///
/// ```ignore
/// semantics(ID_INCREMENT, SemanticsProperties::button("Increment"), button)
///     .on_action(|_| increment())
/// ```
pub struct Semantics {
    id: i32,
    properties: SemanticsProperties,
    on_action: Option<ActionHandler>,
    child: RefCell<Option<AnyWidget>>,
}

impl Semantics {
    pub fn new(id: i32, properties: SemanticsProperties, child: AnyWidget) -> Semantics {
        Semantics {
            id,
            properties,
            on_action: None,
            child: RefCell::new(Some(child)),
        }
    }

    /// What to do when the reader activates this node.
    pub fn with_on_action(mut self, handler: impl Fn(SemanticsAction) + 'static) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }

    /// Builds the widget. Not a `Component`, because there is nothing to build
    /// -- the annotation is the render object.
    pub fn build(self) -> AnyWidget {
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        let id = self.id;
        let properties = self.properties;
        let handler = self.on_action;
        single(child, move |child| {
            let mut render = RenderSemantics::new(id, properties.clone(), child);
            if let Some(handler) = &handler {
                let handler = Rc::clone(handler);
                render = render.with_on_action(move |action| handler(action));
            }
            render
        })
    }
}

impl SemanticsProperties {
    /// A thing that can be pressed.
    pub fn button(label: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                is_button: true,
                has_enabled_state: true,
                is_enabled: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap)
    }

    /// A thing that can be pressed but currently cannot.
    pub fn disabled_button(label: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                is_button: true,
                has_enabled_state: true,
                is_enabled: false,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
    }

    /// A **switch**: a thing a reader is told is on or off.
    ///
    /// This used to set the checked flag, so a switch announced as a checkbox
    /// -- "checked" where it meant "on". Upstream's `Switch` raises
    /// `toggled` and its `Checkbox` raises `checked`, and the two are not
    /// interchangeable to anything that reads them out.
    pub fn toggle(label: impl Into<String>, on: bool) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                toggled: SemanticsTristate::of(on),
                has_enabled_state: true,
                is_enabled: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap)
    }

    /// One **tab** in a bar of them: which one it is, and whether it is the
    /// one you are on.
    ///
    /// Upstream wraps each tab in
    /// `MergeSemantics(Semantics(selected: index == current, label:
    /// localizations.tabLabel(...)))`. Two things a reader needs and this port
    /// gave neither: **which tab is current** -- without it every tab in the
    /// bar sounds identical, which is the same loss a filter chip had -- and
    /// **where in the set this one sits**, which is what
    /// [`crate::material_app::DefaultMaterialLocalizations::tab_label`] says
    /// as "Tab 3 of 5". That function was written, documented and called by
    /// nothing.
    ///
    /// # The words come out in the other order from upstream's
    ///
    /// Upstream puts its `Semantics` beside the tab inside a `Stack`, so the
    /// merge takes the tab's own words first and the position second: "Home,
    /// Tab 3 of 5". Here the folded node's own label comes first and the
    /// descendants' words are appended, so it is "Tab 3 of 5, Home".
    ///
    /// Both say the same two things and neither is wrong; getting upstream's
    /// order would need the position to be a *later sibling*, and a sibling
    /// with no size is dropped before it can be folded (see
    /// [`Clips::applied_to`]). Written down rather than left for a reader of
    /// this code to wonder whether it was noticed.
    pub fn tab(index: usize, count: usize, selected: bool) -> SemanticsProperties {
        SemanticsProperties {
            // One-based for the reader, zero-based for the code -- the
            // conversion is here so that it happens once, and `tab_label`
            // refuses anything else.
            label: crate::material_app::DefaultMaterialLocalizations::tab_label(
                index as u32 + 1,
                count as u32,
            )
            .unwrap_or_default(),
            actions: SemanticsAction::Tap as i32,
            flags: SemanticsFlags {
                selected: SemanticsTristate::of(selected),
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::default()
        }
    }

    /// A **chip**: a compact thing that is pressed, and may be one of a set
    /// that is chosen from.
    ///
    /// Upstream's `RawChip.build`:
    ///
    /// ```dart
    /// Semantics(
    ///   button: widget.tapEnabled,
    ///   container: true,
    ///   selected: kIsWeb ? null : widget.selected,
    ///   checked: kIsWeb ? widget.selected : null,
    ///   enabled: widget.tapEnabled ? canTap : null,
    /// )
    /// ```
    ///
    /// # `selected`, not `checked`
    ///
    /// Upstream splits those two on `kIsWeb`, and its own comment says why:
    /// `aria-selected` works only for a few ARIA roles, so on the web the
    /// engine would drop it and `aria-checked` carries the meaning instead.
    /// **That is a fact about a platform this port does not build for**, so
    /// the non-web arm is the whole of it here -- and if a web target ever
    /// lands, this is where the other arm goes.
    ///
    /// # A chip that cannot be pressed is not a button
    ///
    /// `button: tapEnabled` rather than a constant: a chip used as a plain
    /// label should not be announced as something to press. `enabled` is null
    /// in that case too, which is a third answer again -- "this has no enabled
    /// state" rather than "this is disabled".
    pub fn chip(
        label: impl Into<String>,
        selected: bool,
        tappable: bool,
        can_tap: bool,
    ) -> SemanticsProperties {
        SemanticsProperties {
            actions: if tappable && can_tap {
                SemanticsAction::Tap as i32
            } else {
                0
            },
            flags: SemanticsFlags {
                is_button: tappable,
                selected: SemanticsTristate::of(selected),
                has_enabled_state: tappable,
                is_enabled: tappable && can_tap,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
    }

    /// A **checkbox**: a thing a reader is told is checked, not checked, or
    /// partly checked.
    ///
    /// `None` is upstream's `mixed`, which its `Checkbox` passes only when
    /// `tristate` is set -- a plain checkbox has no third state to be in.
    pub fn check(label: impl Into<String>, value: Option<bool>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                checked: SemanticsCheckState::of(value),
                has_enabled_state: true,
                is_enabled: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap)
    }

    /// Upstream's `RawRadio.build`: one of a set of choices, announced the way
    /// the platform's own screen reader expects.
    ///
    /// ```dart
    /// switch (defaultTargetPlatform) {
    ///   case android || fuchsia || linux || windows:
    ///     accessibilitySelected = null;
    ///     semanticsHint = null;
    ///   case iOS || macOS:
    ///     accessibilitySelected = value;
    ///     if (!(value ?? false)) {
    ///       semanticsHint = localizations.radioButtonUnselectedLabel;
    ///     }
    /// }
    /// Semantics(inMutuallyExclusiveGroup: true, checked: value,
    ///           selected: accessibilitySelected, hint: semanticsHint, ...)
    /// ```
    ///
    /// Three things, and the middle one is the surprise.
    ///
    /// * **`checked` carries the answer on every platform**, and
    ///   `inMutuallyExclusiveGroup` is set on every platform. Those two
    ///   together are what a radio *is*.
    /// * **`selected` is set as well, but only on the Apple platforms.** The
    ///   same fact said twice, in two properties, because the two screen
    ///   readers read different ones. Setting it everywhere is not neutral:
    ///   TalkBack would announce a radio as selected *and* checked.
    /// * **The hint appears only when the radio is off**, and upstream says
    ///   why in its own comment: iOS already announces the selected state
    ///   through `selected`, so a hint on a selected radio would say it twice.
    ///   The one that needs telling is the one that is *not* chosen, because
    ///   silence there is indistinguishable from a control that does nothing.
    pub fn radio(
        label: impl Into<String>,
        selected: bool,
        platform: crate::editable_text::TargetPlatform,
        unselected_hint: &str,
    ) -> SemanticsProperties {
        use crate::editable_text::TargetPlatform;
        let apple = matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS);
        let mut properties = SemanticsProperties {
            flags: SemanticsFlags {
                checked: SemanticsCheckState::of(Some(selected)),
                is_in_mutually_exclusive_group: true,
                // Upstream's `RawRadio` sets `selected` on Apple
                // platforms and leaves it null elsewhere. `apple && selected`
                // was right about Apple's true case and could not tell
                // Apple's *false* case from the other platforms' silence --
                // which is the difference between "not selected" and nothing.
                selected: if apple {
                    SemanticsTristate::of(selected)
                } else {
                    SemanticsTristate::None
                },
                has_enabled_state: true,
                is_enabled: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap);
        if apple && !selected {
            properties.hint = unselected_hint.to_string();
        }
        properties
    }

    /// A place text is typed.
    pub fn text_field(label: impl Into<String>, text: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            value: text.into(),
            flags: SemanticsFlags {
                is_text_field: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        // Tap because a finger reaches a field by touching it, and Focus
        // because a keyboard-shaped reader reaches it by moving to it --
        // upstream's `EditableText` offers both for the same two ways in.
        .with_action(SemanticsAction::Tap)
        .with_action(SemanticsAction::Focus)
    }

    /// A heading. Screen readers let a reader jump between these.
    pub fn header(label: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                is_header: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
    }

    /// A heading that also *names the page*, which is what an app bar's title
    /// is.
    ///
    /// Upstream:
    ///
    /// ```dart
    /// Semantics(
    ///   namesRoute: switch (defaultTargetPlatform) {
    ///     android || fuchsia || linux || windows => true,
    ///     iOS || macOS => null,
    ///   },
    ///   header: true,
    ///   child: title,
    /// )
    /// ```
    ///
    /// **Null on Apple and not false** -- the third platform branch in this
    /// port's semantics, after the radio's `selected` and the radio's
    /// unselected hint, and the same reason each time: VoiceOver announces a
    /// route change on its own, so a second announcement is a repetition
    /// rather than an aid.
    pub fn route_header(
        label: impl Into<String>,
        platform: crate::editable_text::TargetPlatform,
    ) -> SemanticsProperties {
        use crate::editable_text::TargetPlatform;
        let apple = matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS);
        SemanticsProperties {
            flags: SemanticsFlags {
                is_header: true,
                names_route: !apple,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
    }

    /// How far a screen reader's increase or decrease moves a slider.
    ///
    /// Upstream's `_SliderState._adjustmentUnit`, in **normalised** units:
    /// a tenth on Apple's platforms and a twentieth everywhere else, each
    /// matching what that platform's own slider does. A divided slider
    /// overrides both with one division, because the only values it can hold
    /// are the divisions and a smaller step would land between them.
    pub fn slider_action_unit(
        divisions: Option<u32>,
        platform: crate::editable_text::TargetPlatform,
    ) -> f32 {
        use crate::editable_text::TargetPlatform;
        match divisions {
            Some(divisions) if divisions > 0 => 1.0 / divisions as f32,
            _ => match platform {
                TargetPlatform::IOS | TargetPlatform::MacOS => 0.1,
                _ => 0.05,
            },
        }
    }

    /// Something a reader can set to a value along a range.
    ///
    /// Upstream's `_RenderSlider.describeSemanticsConfiguration`. Nothing in
    /// this port declared it, so every slider reached a screen reader as a
    /// plain box: no role, no value read out, and no way to change it -- while
    /// the flag bit and the three value strings had been crossing the FFI
    /// since they were written.
    ///
    /// # The three values, and why they are strings
    ///
    /// `value`, `increasedValue` and `decreasedValue` are what the platform
    /// reads aloud now, and what it would read after one swipe each way. They
    /// are text rather than numbers because only the widget knows what the
    /// number *means* -- upstream lets a caller replace all three with a
    /// `semanticFormatterCallback`, and the default it falls back to is a
    /// percentage of the range rather than the raw value.
    ///
    /// # The step is clamped before it is spoken, not after
    ///
    /// `clampDouble(value + unit, 0.0, 1.0)` upstream: a slider at 97% with a
    /// 5% step says its next value is **100%**, not 102%. Reading the
    /// unclamped number would promise a reader somewhere the slider cannot
    /// go, and then not go there.
    pub fn slider(
        value: f32,
        min: f32,
        max: f32,
        divisions: Option<u32>,
        label: Option<&str>,
        interactive: bool,
        platform: crate::editable_text::TargetPlatform,
    ) -> SemanticsProperties {
        let span = max - min;
        let normalised = if span == 0.0 {
            0.0
        } else {
            ((value - min) / span).clamp(0.0, 1.0)
        };
        let unit = SemanticsProperties::slider_action_unit(divisions, platform);
        let percent = |fraction: f32| format!("{}%", (fraction * 100.0).round() as i32);
        // Upstream offers the two actions only on an interactive slider. A
        // reader handed "swipe up to increase" on a disabled one has been
        // told the control works.
        let actions = if interactive {
            SemanticsAction::Increase as i32 | SemanticsAction::Decrease as i32
        } else {
            0
        };
        SemanticsProperties {
            label: label.unwrap_or_default().to_string(),
            value: percent(normalised),
            increased_value: percent((normalised + unit).clamp(0.0, 1.0)),
            decreased_value: percent((normalised - unit).clamp(0.0, 1.0)),
            text_direction: Some(crate::direction::current_direction()),
            actions,
            flags: SemanticsFlags {
                is_slider: true,
                has_enabled_state: true,
                is_enabled: interactive,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::default()
        }
    }

    /// Which way a reader may scroll from **where they are now**.
    ///
    /// Upstream's `ScrollPosition._updateSemanticActions`, whose own doc says
    /// what it is for: "If the scroll view has been scrolled all the way to
    /// the top, the action to scroll further up needs to be removed as the
    /// scroll view cannot be scrolled in that direction anymore."
    ///
    /// ```dart
    /// final actions = <SemanticsAction>{
    ///   if (pixels > minScrollExtent) backward,
    ///   if (pixels < maxScrollExtent) forward,
    /// };
    /// ```
    ///
    /// # "Scroll up" is the one that takes you down the list
    ///
    /// The pair is chosen by the **axis direction**, and the naming inverts on
    /// the way: for a plain downward list, `forward` -- the action that carries
    /// you further into the content -- is `scrollUp`, because it is the finger
    /// movement, not the travel. So the action offered when there is more list
    /// below is `ScrollUp`, and a list read the other way round would offer a
    /// reader at the top the one gesture that does nothing.
    ///
    /// An `AxisDirection::Up` list (a chat pinned to the bottom) swaps them,
    /// which is why this takes a direction rather than an axis.
    pub fn scroll_actions(
        axis_direction: crate::render::AxisDirection,
        pixels: f32,
        min: f32,
        max: f32,
    ) -> i32 {
        use crate::render::AxisDirection;
        let (forward, backward) = match axis_direction {
            AxisDirection::Up => (SemanticsAction::ScrollDown, SemanticsAction::ScrollUp),
            AxisDirection::Down => (SemanticsAction::ScrollUp, SemanticsAction::ScrollDown),
            AxisDirection::Left => (SemanticsAction::ScrollRight, SemanticsAction::ScrollLeft),
            AxisDirection::Right => (SemanticsAction::ScrollLeft, SemanticsAction::ScrollRight),
        };
        let mut actions = 0;
        if pixels > min {
            actions |= backward as i32;
        }
        if pixels < max {
            actions |= forward as i32;
        }
        actions
    }

    /// Something that scrolls, and how far down it is.
    ///
    /// # The extents are told; the actions depend on where you are
    ///
    /// Upstream writes `scrollPosition`, `scrollExtentMax` and
    /// `scrollExtentMin` whenever the position `haveDimensions`, and decides
    /// the actions separately in [`SemanticsProperties::scroll_actions`]. The
    /// two are different claims and it matters which is which -- "this is a
    /// list, and you are at the top of it" is true of a list that fits on
    /// screen, while "you can scroll this" is not, and a reader offered a
    /// gesture that does nothing has been told a small lie about the page.
    ///
    /// This used to gate the actions on `max > min` alone and then offer
    /// **both** directions, which is a smaller lie of the same kind: at the
    /// top of a list it said you could scroll back up.
    ///
    /// `child_count` is upstream's `semanticChildCount`: how many items the
    /// list has in total, not how many are built or on screen. `None` is a
    /// list that does not know -- upstream's null, and the reason
    /// `semanticChildCount` is a parameter callers may leave out.
    pub fn scrollable(
        offset: f32,
        min: f32,
        max: f32,
        axis_direction: crate::render::AxisDirection,
        child_count: Option<i32>,
    ) -> SemanticsProperties {
        let actions = SemanticsProperties::scroll_actions(axis_direction, offset, min, max);
        SemanticsProperties {
            actions,
            scroll_position: offset,
            scroll_extent_min: min,
            scroll_extent_max: max,
            scroll_child_count: child_count,
            // Not knowable here, and deliberately left for the walk: which
            // item is showing depends on which of them survived, and nothing
            // has been walked yet when a render object describes itself. See
            // [`close`].
            scroll_index: None,
            ..SemanticsProperties::default()
        }
    }
}

/// Where automatically-allocated node ids start.
///
/// There are two sources of identity here and they must not meet: an
/// identifier the caller already had (a hit-test id, so that the two answers
/// to "which control is this" agree), and one invented for a widget that has
/// none. Upstream has only the second, because there a semantics node is
/// always allocated by the framework. Splitting the range is what makes both
/// possible: below the base is the caller's, at or above it is ours.
const AUTO_BASE: i32 = 1 << 28;

/// A node id for a caller's identifier.
///
/// Folded into the low range rather than truncated, so a caller who chose a
/// large id -- the examples hand out blocks at `1 << 40` -- still lands
/// somewhere that cannot be mistaken for an automatic one. Never zero, which
/// belongs to [`ROOT_ID`].
pub fn node_id_for(caller: u64) -> i32 {
    1 + (caller % (AUTO_BASE as u64 - 1)) as i32
}

/// [`Semantics`] as a widget.
pub fn semantics(id: i32, properties: SemanticsProperties, child: AnyWidget) -> AnyWidget {
    Semantics::new(id, properties, child).build()
}

/// Annotates a subtree without the caller having to invent an identifier.
///
/// The element's own id is used, which is stable for exactly as long as the
/// widget keeps its place in the tree -- and that is the right lifetime: a
/// node that moved somewhere else in the tree *is* a different thing to a
/// screen reader, and one that merely rebuilt is not. Upstream reaches the
/// same stability from its persistent semantics tree.
///
/// Prefer [`semantics`] where an identifier already exists -- every component
/// that can be tapped already has one for hit testing, and reusing it keeps
/// the two answers to "which thing is this" in agreement.
pub struct AutoSemantics {
    properties: SemanticsProperties,
    on_action: Option<ActionHandler>,
    child: RefCell<Option<AnyWidget>>,
}

impl Component for AutoSemantics {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        let mut annotation = Semantics::new(
            AUTO_BASE.wrapping_add(context.element().index() as i32),
            self.properties.clone(),
            child,
        );
        annotation.on_action = self.on_action.clone();
        annotation.build()
    }
}

/// [`AutoSemantics`] as a widget.
pub fn describe(properties: SemanticsProperties, child: AnyWidget) -> AnyWidget {
    component(AutoSemantics {
        properties,
        on_action: None,
        child: RefCell::new(Some(child)),
    })
}

/// [`AutoSemantics`] with an action handler.
pub fn describe_with_action(
    properties: SemanticsProperties,
    child: AnyWidget,
    handler: impl Fn(SemanticsAction) + 'static,
) -> AnyWidget {
    component(AutoSemantics {
        properties,
        on_action: Some(Rc::new(handler)),
        child: RefCell::new(Some(child)),
    })
}

/// [`Semantics`] with an action handler.
pub fn semantics_with_action(
    id: i32,
    properties: SemanticsProperties,
    child: AnyWidget,
    handler: impl Fn(SemanticsAction) + 'static,
) -> AnyWidget {
    Semantics::new(id, properties, child)
        .with_on_action(handler)
        .build()
}

/// Wraps `child` in the node that says **something arrived without being asked
/// for**.
///
/// Upstream gives this to a snack bar (`_SnackBarState.build`) and to a
/// material banner (`_MaterialBannerState.build`), in both cases as
/// `Semantics(container: true, liveRegion: true, ...)`.
///
/// **Container**: one stop, so a message and its action are met together
/// rather than as two unrelated things at the edge of the screen.
/// **Live region**: the thing appeared on its own, and a reader who is
/// somewhere else on the page has to be told -- words sitting in the tree with
/// nothing pointing at them are words nobody hears.
///
/// The flag rides on the folded node because that is the node with the words:
/// a live region with nothing in it announces nothing.
///
/// # It is not only for things that vanish
///
/// A snack bar's case is easy -- it is gone in four seconds, so a reader who
/// has to hunt for it has already lost it. A banner does **not** dismiss
/// itself, and it would be reasonable to guess it therefore should not
/// interrupt. Upstream gives it the flag anyway, and the reason survives the
/// difference: a thing that appears unbidden is worth telling someone about
/// whether or not it will leave on its own. Guessed the other way, this would
/// have been a silent banner.
pub fn announces_itself(child: AnyWidget) -> AnyWidget {
    crate::framework::single(child, |inner| {
        crate::render::RenderMergeSemanticsBox::new(inner).with_properties(SemanticsProperties {
            flags: SemanticsFlags {
                is_live_region: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label("")
        })
    })
}

/// Wraps `child` in a node whose **Tap action reaches the same handler a
/// finger would**.
///
/// Upstream's rule for `Semantics.onTap`, and the reason it is worth one
/// function rather than three copies: a control with two ways in must not be
/// able to disagree with itself about what pressing it does. Written out at
/// each call site, the day one of them grows a condition the others do not is
/// the day a screen reader starts doing something slightly different from a
/// finger -- and nothing fails, it just diverges.
///
/// `on_tap` of `None` is a control that does nothing when pressed. The node is
/// still made, because *saying* what something is does not depend on its being
/// operable, and a disabled control that vanished from the tree would be worse
/// than one that says it is disabled.
pub fn tappable(
    id: i32,
    properties: SemanticsProperties,
    child: AnyWidget,
    on_tap: Option<std::rc::Rc<dyn Fn(crate::gestures::TapEvent)>>,
) -> AnyWidget {
    semantics_with_action(id, properties, child, move |action| {
        if action != SemanticsAction::Tap {
            return;
        }
        let Some(on_tap) = &on_tap else {
            return;
        };
        // The position is the one thing the two paths cannot share: a reader
        // activates the control, not a point on it. Upstream hands its own
        // `onTap` no coordinates either.
        on_tap(crate::gestures::TapEvent {
            local_position: crate::render::Offset::ZERO,
            pointer_id: 0,
        });
    })
}

// -- What a label carries besides its letters ---------------------------------

/// Upstream's `StringAttribute` family (`dart:ui`), which a screen reader reads
/// *with* rather than *instead of* the text.
///
/// Two of them, and both are about pronunciation rather than meaning: a range
/// to spell out letter by letter, and a range in another language. Declared
/// here because `dart:ui` is the engine's side and this crate needs the shape
/// to carry across; the payload reaches the platform unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringAttribute {
    /// Upstream `SpellOutStringAttribute`: read this range one letter at a
    /// time. For a code, a licence plate, a password hint -- anything a reader
    /// would otherwise pronounce as a word and get wrong.
    SpellOut { range: TextRange },
    /// Upstream `LocaleStringAttribute`: read this range as the named language.
    /// A French phrase inside an English sentence is unintelligible read with
    /// English phonetics.
    Locale { range: TextRange, locale: String },
}

impl StringAttribute {
    pub fn range(&self) -> TextRange {
        match self {
            StringAttribute::SpellOut { range } => *range,
            StringAttribute::Locale { range, .. } => *range,
        }
    }

    /// Upstream's `StringAttribute.copy(range:)`, which every concatenation
    /// needs: the attribute keeps its kind and takes a new range.
    pub fn with_range(&self, range: TextRange) -> StringAttribute {
        match self {
            StringAttribute::SpellOut { .. } => StringAttribute::SpellOut { range },
            StringAttribute::Locale { locale, .. } => StringAttribute::Locale {
                range,
                locale: locale.clone(),
            },
        }
    }
}

/// Upstream `AttributedString`: a label plus the ranges inside it that are read
/// differently.
///
/// # Concatenation is where the work is
///
/// Joining two of them has to shift the right operand's ranges by the left
/// one's length, or every attribute past the seam points at the wrong letters.
/// Upstream's `operator +` does exactly that, and then has two early returns:
/// an empty left hands back the right operand whole, and an empty right hands
/// back the left one.
///
/// **Those two are an optimisation, and they are safe because of the
/// constructor's assert.** An empty string may carry no attributes
/// (`string.isNotEmpty || attributes.isEmpty` upstream), so an empty operand
/// has nothing to contribute and returning the other one whole loses nothing --
/// the general path would compute the same answer. If that invariant were ever
/// broken, the early return would silently drop the attributes the general path
/// would have kept, which is why the assert is worth having rather than being
/// merely tidy.
///
/// This paragraph first claimed the early returns were load-bearing rather than
/// an optimisation. Removing one and watching every test stay green is what
/// corrected it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttributedString {
    string: String,
    attributes: Vec<StringAttribute>,
}

impl AttributedString {
    /// Plain text, no attributes.
    pub fn new(string: impl Into<String>) -> AttributedString {
        AttributedString {
            string: string.into(),
            attributes: Vec::new(),
        }
    }

    /// Upstream's constructor with `attributes:`.
    ///
    /// It asserts two things and both are kept: an empty string carries no
    /// attributes, and every range is inside the string. A range past the end
    /// would be handed to a screen reader that then reads whatever it finds
    /// there, which is a silent wrong rather than a crash.
    pub fn with_attributes(
        string: impl Into<String>,
        attributes: Vec<StringAttribute>,
    ) -> AttributedString {
        let string = string.into();
        debug_assert!(
            !string.is_empty() || attributes.is_empty(),
            "an empty string carries no attributes"
        );
        debug_assert!(
            attributes.iter().all(|attribute| {
                let range = attribute.range();
                range.start >= 0
                    && range.end >= 0
                    && (range.start as usize) <= string.len()
                    && (range.end as usize) <= string.len()
            }),
            "an attribute's range is outside the string it is on"
        );
        AttributedString { string, attributes }
    }

    pub fn string(&self) -> &str {
        &self.string
    }

    pub fn attributes(&self) -> &[StringAttribute] {
        &self.attributes
    }

    pub fn is_empty(&self) -> bool {
        self.string.is_empty()
    }

    /// Upstream's `operator +`.
    pub fn concat(&self, other: &AttributedString) -> AttributedString {
        if self.string.is_empty() {
            return other.clone();
        }
        if other.string.is_empty() {
            return self.clone();
        }
        // The offset is the *byte* length, which is what the ranges in this
        // crate are counted in -- `TextRange` here indexes the same string the
        // engine is handed.
        let offset = self.string.len() as isize;
        let mut attributes = self.attributes.clone();
        attributes.extend(other.attributes.iter().map(|attribute| {
            let range = attribute.range();
            attribute.with_range(TextRange::new(range.start + offset, range.end + offset))
        }));
        AttributedString {
            string: format!("{}{}", self.string, other.string),
            attributes,
        }
    }
}

impl std::ops::Add for &AttributedString {
    type Output = AttributedString;

    fn add(self, other: &AttributedString) -> AttributedString {
        self.concat(other)
    }
}

/// Upstream `AttributedStringProperty`: an [`AttributedString`] as a
/// diagnostics property.
///
/// Its own rule, and the reason it is a class rather than a `StringProperty`:
/// **it hides itself when the string is empty**, and it shows the attributes
/// only when there are some. A diagnostics dump of a tree where most nodes have
/// no label would otherwise be mostly empty quotes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributedStringProperty {
    name: String,
    value: Option<AttributedString>,
    /// Upstream's `showName`, which a caller turns off when the name is
    /// obvious from position.
    show_name: bool,
}

impl AttributedStringProperty {
    pub fn new(
        name: impl Into<String>,
        value: Option<AttributedString>,
    ) -> AttributedStringProperty {
        AttributedStringProperty {
            name: name.into(),
            value,
            show_name: true,
        }
    }

    pub fn with_show_name(mut self, show: bool) -> Self {
        self.show_name = show;
        self
    }

    /// Upstream's `isInteresting`: absent or empty is not worth printing.
    pub fn is_interesting(&self) -> bool {
        self.value
            .as_ref()
            .is_some_and(|value| !value.string().is_empty())
    }

    /// Upstream's `valueToString`: the text in quotes, and the attributes after
    /// it only when there are any.
    pub fn value_to_string(&self) -> String {
        let Some(value) = &self.value else {
            return "null".to_string();
        };
        if value.attributes().is_empty() {
            format!("\"{}\"", value.string())
        } else {
            format!("\"{}\" {:?}", value.string(), value.attributes())
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn show_name(&self) -> bool {
        self.show_name
    }
}

// -- Marking a node for someone else to find ----------------------------------

/// Upstream `SemanticsTag`: a marker a node carries so an ancestor can pick it
/// out later.
///
/// # Its name is not its identity
///
/// This is upstream's own emphasis and it is worth keeping. The name is *for
/// debugging*: "two tags created with the same name and the `new` operator are
/// not considered identical", while two `const` ones are, because Dart
/// canonicalises constants. So identity is the object, not the string.
///
/// Rust has no const canonicalisation to lean on, so identity is an id handed
/// out at construction: two [`SemanticsTag::new`] calls with the same name are
/// different tags, exactly as two `new` calls are upstream. The way to get
/// upstream's `const` behaviour is the way a Rust caller would reach for
/// anyway -- declare it once and share it:
///
/// ```ignore
/// static SCROLLED_INTO_VIEW: LazyLock<SemanticsTag> =
///     LazyLock::new(|| SemanticsTag::new("scrolled into view"));
/// ```
///
/// A tag compared by name instead would make two unrelated subsystems that
/// happened to pick the same word interfere with each other, which is the bug
/// upstream's identity rule exists to prevent.
#[derive(Clone, Debug)]
pub struct SemanticsTag {
    name: String,
    id: u64,
}

impl SemanticsTag {
    pub fn new(name: impl Into<String>) -> SemanticsTag {
        SemanticsTag {
            name: name.into(),
            id: next_tag_id(),
        }
    }

    /// A tag whose identity is *derived* rather than allocated.
    ///
    /// The only caller is [`PlaceholderSpanIndexSemanticsTag`], and its docs
    /// give the reason: a tag that has to match across frames cannot take a
    /// fresh id each time it is built. Ids from
    /// `PlaceholderSpanIndexSemanticsTag::ID_BASE` up are reserved for that and
    /// are out of the counter's reach.
    pub(crate) fn with_id(name: impl Into<String>, id: u64) -> SemanticsTag {
        SemanticsTag {
            name: name.into(),
            id,
        }
    }

    /// For debugging only. Two tags with this same name may well be different
    /// tags.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What this tag is compared by.
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl PartialEq for SemanticsTag {
    fn eq(&self, other: &SemanticsTag) -> bool {
        self.id == other.id
    }
}

impl Eq for SemanticsTag {}

impl std::hash::Hash for SemanticsTag {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Upstream `PlaceholderSpanIndexSemanticsTag`: the tag that says which inline
/// placeholder a semantics node came from.
///
/// # It is the one tag compared by value, and deliberately so
///
/// [`SemanticsTag`] is compared by **identity** -- two tags made with the same
/// name are different tags, so two subsystems that happen to pick the same word
/// do not interfere. This one overrides that: upstream's doc says outright that
/// two tags with the same `index` are considered the same.
///
/// The reason is that the paragraph makes these fresh on every layout, one per
/// placeholder, and the node from this frame has to be recognised as the node
/// from the last one. Identity would make every frame's tags unrelated to the
/// previous frame's, and nothing would ever match.
///
/// So the tag it produces carries an id derived from the index rather than
/// drawn from the counter, which is how "equal by index" is said in a scheme
/// built on identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaceholderSpanIndexSemanticsTag {
    pub index: usize,
}

impl PlaceholderSpanIndexSemanticsTag {
    /// Ids from here up belong to placeholder tags. Above anything the counter
    /// will reach, so a derived id can never collide with an allocated one.
    const ID_BASE: u64 = 1 << 48;

    pub fn new(index: usize) -> PlaceholderSpanIndexSemanticsTag {
        PlaceholderSpanIndexSemanticsTag { index }
    }

    /// The tag itself. Upstream's name is `PlaceholderSpanIndexSemanticsTag(3)`
    /// and so is this one -- it is what appears in a semantics dump.
    pub fn to_tag(&self) -> SemanticsTag {
        SemanticsTag::with_id(
            format!("PlaceholderSpanIndexSemanticsTag({})", self.index),
            PlaceholderSpanIndexSemanticsTag::ID_BASE + self.index as u64,
        )
    }

    /// Reads the index back out of a tag, if it is one of these.
    pub fn index_of(tag: &SemanticsTag) -> Option<usize> {
        let id = tag.id();
        id.checked_sub(PlaceholderSpanIndexSemanticsTag::ID_BASE)
            .map(|index| index as usize)
    }
}

thread_local! {
    static NEXT_TAG_ID: Cell<u64> = const { Cell::new(1) };
}

fn next_tag_id() -> u64 {
    NEXT_TAG_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

// -- Saying what an action does, in this node's words -------------------------

/// Upstream `SemanticsHintOverrides`: what a screen reader says a tap or a long
/// press will *do*, in place of the standard phrasing.
///
/// # The hint says what happens, not how to do it
///
/// Upstream's doc gives the rule as two pairs of examples, and they are the
/// whole of the type's value:
///
/// * Bad: "Double tap to show movies". Good: "show movies".
/// * Bad: "Double tap and hold to show tooltip". Good: "show tooltip".
///
/// The platform already tells the reader *how* to activate things -- it knows
/// whether this device wants a double tap, a split tap, or a keyboard -- and a
/// hint that repeats the gesture is both redundant and, on a device that uses a
/// different gesture, wrong.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SemanticsHintOverrides {
    on_tap_hint: Option<String>,
    on_long_press_hint: Option<String>,
}

impl SemanticsHintOverrides {
    pub fn new() -> SemanticsHintOverrides {
        SemanticsHintOverrides::default()
    }

    /// Upstream asserts `onTapHint != ''`. **Empty is not the same as absent**:
    /// absent means "use the standard hint", and empty would mean "say nothing
    /// at all", which is a way of hiding what the button does rather than
    /// describing it. A caller who meant the first wrote the second.
    pub fn with_tap_hint(mut self, hint: impl Into<String>) -> Self {
        let hint = hint.into();
        debug_assert!(
            !hint.is_empty(),
            "an empty tap hint is not the same as none"
        );
        self.on_tap_hint = Some(hint);
        self
    }

    pub fn with_long_press_hint(mut self, hint: impl Into<String>) -> Self {
        let hint = hint.into();
        debug_assert!(
            !hint.is_empty(),
            "an empty long-press hint is not the same as none"
        );
        self.on_long_press_hint = Some(hint);
        self
    }

    pub fn on_tap_hint(&self) -> Option<&str> {
        self.on_tap_hint.as_deref()
    }

    pub fn on_long_press_hint(&self) -> Option<&str> {
        self.on_long_press_hint.as_deref()
    }

    /// Upstream's `isNotEmpty`: whether either hint was set.
    pub fn is_not_empty(&self) -> bool {
        self.on_tap_hint.is_some() || self.on_long_press_hint.is_some()
    }
}

// -- Actions the standard set has no name for ---------------------------------

/// Upstream `CustomSemanticsAction`: an action offered to a screen reader
/// beyond the fixed vocabulary of [`SemanticsAction`].
///
/// Two shapes, and upstream gives each its own constructor because they are not
/// variations of one thing:
///
/// * a **new** action, which has a `label` and appears in the reader's actions
///   menu as its own entry -- [`CustomSemanticsAction::labelled`];
/// * an action that **overrides a standard one**, which has a `hint` and a
///   [`SemanticsAction`] it replaces, so the reader keeps offering the standard
///   gesture and describes it in this node's words --
///   [`CustomSemanticsAction::overriding`].
///
/// A label without an action is the first; a hint with an action is the second;
/// neither ever has both a label and a hint.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CustomSemanticsAction {
    label: Option<String>,
    hint: Option<String>,
    action: Option<SemanticsAction>,
}

impl CustomSemanticsAction {
    /// Upstream's default constructor. It asserts the label is not empty: an
    /// action with no name is an entry in the reader's menu that says nothing.
    pub fn labelled(label: impl Into<String>) -> CustomSemanticsAction {
        let label = label.into();
        debug_assert!(!label.is_empty(), "a custom action needs a name");
        CustomSemanticsAction {
            label: Some(label),
            hint: None,
            action: None,
        }
    }

    /// Upstream's `CustomSemanticsAction.overridingAction`.
    pub fn overriding(hint: impl Into<String>, action: SemanticsAction) -> CustomSemanticsAction {
        let hint = hint.into();
        debug_assert!(!hint.is_empty(), "an overriding action needs a hint");
        CustomSemanticsAction {
            label: None,
            hint: Some(hint),
            action: Some(action),
        }
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub fn action(&self) -> Option<SemanticsAction> {
        self.action
    }

    /// Upstream's static `getIdentifier`: the number this action is known by on
    /// the wire, assigned on first ask and stable afterwards.
    ///
    /// It is keyed on the action's **value**, not on the object -- unlike
    /// [`SemanticsTag`], whose whole point is the opposite. That is upstream's
    /// choice and it follows from what each is for: a tag marks one particular
    /// node for one particular ancestor, so two tags that read alike must not
    /// collide; a custom action is a menu entry, and two nodes offering the
    /// same label and hint *are* offering the same action and should share an
    /// id.
    pub fn identifier(action: &CustomSemanticsAction) -> i32 {
        CUSTOM_ACTIONS.with(|registry| {
            let registry = &mut *registry.borrow_mut();
            if let Some(id) = registry.ids.get(action) {
                return *id;
            }
            let id = registry.next;
            registry.next += 1;
            registry.ids.insert(action.clone(), id);
            registry.actions.insert(id, action.clone());
            id
        })
    }

    /// Upstream's static `getAction`: the action a number stands for, or
    /// nothing if that number was never handed out.
    pub fn from_identifier(id: i32) -> Option<CustomSemanticsAction> {
        CUSTOM_ACTIONS.with(|registry| registry.borrow().actions.get(&id).cloned())
    }

    /// Upstream's `resetForTests`, and it exists for the same reason: the
    /// registry is process-wide, so one test's actions would otherwise decide
    /// the next one's ids.
    pub fn reset_for_tests() {
        CUSTOM_ACTIONS.with(|registry| {
            let registry = &mut *registry.borrow_mut();
            registry.ids.clear();
            registry.actions.clear();
            registry.next = 0;
        });
    }
}

#[derive(Default)]
struct CustomActionRegistry {
    next: i32,
    ids: std::collections::HashMap<CustomSemanticsAction, i32>,
    actions: std::collections::HashMap<i32, CustomSemanticsAction>,
}

thread_local! {
    static CUSTOM_ACTIONS: RefCell<CustomActionRegistry> =
        RefCell::new(CustomActionRegistry::default());
}

// -- Deciding the order a reader walks in -------------------------------------

/// Upstream `SemanticsSortKey`: what decides traversal order when the geometry
/// would get it wrong.
///
/// A screen reader normally walks a screen in reading order worked out from
/// where things are. That is right until it is not -- a two-column layout whose
/// columns should be read one after the other rather than line by line, a
/// toolbar that belongs after the content it acts on -- and a sort key is how a
/// widget says so.
///
/// # Two rules, both surprising
///
/// * **Keys of different kinds never compare.** Upstream asserts on
///   `runtimeType`, because there is no meaningful answer: an ordinal 3 is
///   neither before nor after some other scheme's key. Rust gives this for
///   free -- [`OrdinalSortKey`] is its own type and there is nothing to compare
///   it against -- so the assert has no counterpart here, which is the good
///   kind of missing.
/// * **Unnamed keys sort before named ones**, and named keys sort by name
///   before their own ordering is consulted. So `name` is a grouping, not a
///   label: two keys with different names are ordered by their names whatever
///   their values say.
pub trait SemanticsSortKey {
    /// The group this key belongs to. `None` sorts first.
    fn name(&self) -> Option<&str>;

    /// Upstream's `doCompare`, called only when the names match.
    fn do_compare(&self, other: &Self) -> std::cmp::Ordering;

    /// Upstream's `compareTo`: name first, then the subclass's own ordering.
    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        match (self.name(), other.name()) {
            (a, b) if a == b => self.do_compare(other),
            // "Keys that don't have a name are sorted together and come before
            // those with a name."
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
            // Both unnamed is caught by the equality arm above. Spelled out
            // rather than left to a wildcard, so a later edit to the arms above
            // cannot silently fall through to a wrong answer.
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
}

/// Upstream `OrdinalSortKey`: a number, lowest read first.
///
/// The order must be finite. Upstream asserts it is strictly between the two
/// infinities, and the reason is that a sort is only a sort if every pair has
/// an answer: two keys both at positive infinity compare equal and would be
/// left in whatever order they arrived, which is exactly the non-determinism a
/// caller reached for a sort key to escape.
#[derive(Clone, Debug)]
pub struct OrdinalSortKey {
    order: f64,
    name: Option<String>,
}

impl OrdinalSortKey {
    pub fn new(order: f64) -> OrdinalSortKey {
        debug_assert!(order.is_finite(), "a sort key's order must be finite");
        OrdinalSortKey { order, name: None }
    }

    /// Groups this key with others of the same name. See
    /// [`SemanticsSortKey`]'s second rule.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn order(&self) -> f64 {
        self.order
    }
}

impl SemanticsSortKey for OrdinalSortKey {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn do_compare(&self, other: &OrdinalSortKey) -> std::cmp::Ordering {
        // Finite by construction, so this never sees a NaN.
        self.order
            .partial_cmp(&other.order)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl PartialEq for OrdinalSortKey {
    fn eq(&self, other: &OrdinalSortKey) -> bool {
        self.compare(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for OrdinalSortKey {}

impl PartialOrd for OrdinalSortKey {
    fn partial_cmp(&self, other: &OrdinalSortKey) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrdinalSortKey {
    fn cmp(&self, other: &OrdinalSortKey) -> std::cmp::Ordering {
        self.compare(other)
    }
}

// -- Joining text into one label ----------------------------------------------

/// Upstream `SemanticsLabelBuilder`: several pieces of text joined into one
/// label a screen reader can read straight through.
///
/// A label assembled by hand -- `"$title $subtitle"` -- reads correctly right
/// up until one of the pieces is in the other script, and then the reader runs
/// them together in whichever direction it guessed. This puts Unicode's
/// directional embedding marks around the pieces that need them.
///
/// # Three rules, and the third is a surprise
///
/// * **Empty parts are dropped**, in `add_part` rather than in `build`, so they
///   do not leave a doubled separator behind.
/// * **A part is wrapped only when its direction differs from the builder's**,
///   and a part that did not name one is never wrapped. Only an explicitly
///   contrary part gets marks.
/// * **The first part is never wrapped**, whatever direction it names.
///   Upstream writes it to the buffer unprocessed and starts the
///   direction-checking loop at the second.
///
/// That third rule looks like an oversight and behaves like one -- a label
/// whose first piece is Arabic and whose builder is left-to-right gets no
/// marks on the piece that most needs them. It is upstream's behaviour, it is
/// what an application built against upstream will have been laid out around,
/// and changing it here would make this port the odd one out. Ported as-is and
/// written down, which is the whole point of writing it down.
///
/// # Two lines of upstream that cannot change the answer
///
/// Both are kept, because a port that quietly tidies its source is a port
/// nobody can diff against it. Both are marked, because a reader should not
/// have to work out for themselves that they do nothing:
///
/// * `partTextDirection ?? textDirection`. With the fallback, an unnamed part
///   takes the builder's direction and the "differs" test is false; without it
///   the part's direction is null and the null check is false. Neither path
///   ever wraps.
/// * the single-part early return. The general path writes the first part
///   unprocessed and then iterates an empty remainder, which is the same
///   string.
///
/// Found by removing each and watching every test stay green.
#[derive(Clone, Debug)]
pub struct SemanticsLabelBuilder {
    separator: String,
    text_direction: Option<TextDirection>,
    parts: Vec<(String, Option<TextDirection>)>,
}

impl SemanticsLabelBuilder {
    /// A builder joining with a single space, upstream's default separator, and
    /// no overall direction -- which means nothing is ever wrapped, since a
    /// part can only differ from a direction that exists.
    pub fn new() -> SemanticsLabelBuilder {
        SemanticsLabelBuilder {
            separator: " ".to_string(),
            text_direction: None,
            parts: Vec::new(),
        }
    }

    /// Upstream's `separator:`. May be empty, and then the parts run together
    /// with only the directional marks between them.
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Upstream's `textDirection:`: the direction of the label as a whole, and
    /// the thing each part is compared against.
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.text_direction = Some(direction);
        self
    }

    /// Upstream's `addPart`. An empty label is ignored.
    pub fn add_part(&mut self, label: impl Into<String>) {
        let label = label.into();
        if !label.is_empty() {
            self.parts.push((label, None));
        }
    }

    /// Upstream's `addPart(label, textDirection:)`.
    pub fn add_part_in(&mut self, label: impl Into<String>, direction: TextDirection) {
        let label = label.into();
        if !label.is_empty() {
            self.parts.push((label, Some(direction)));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// How many parts were kept -- which is not how many were added, since
    /// empty ones were dropped.
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Upstream's `clear`, so one builder can make several labels.
    pub fn clear(&mut self) {
        self.parts.clear();
    }

    /// Upstream's `build`.
    pub fn build(&self) -> String {
        if self.parts.is_empty() {
            return String::new();
        }
        // A shortcut, not a rule: the general path below writes the first part
        // unprocessed and then iterates nothing. See the type's docs.
        if self.parts.len() == 1 {
            return self.parts[0].0.clone();
        }

        let mut label = String::new();
        // The first part, unprocessed. This is where the third rule lives.
        label.push_str(&self.parts[0].0);

        for (text, part_direction) in &self.parts[1..] {
            // Upstream's `partTextDirection ?? textDirection`. The fallback
            // cannot change the outcome either way -- see the type's docs --
            // and is kept so this reads as its source does.
            let direction = part_direction.or(self.text_direction);
            label.push_str(&self.separator);
            match (self.text_direction, direction) {
                (Some(overall), Some(part)) if overall != part => {
                    label.push(match part {
                        TextDirection::Rtl => crate::licenses::Unicode::RLE,
                        TextDirection::Ltr => crate::licenses::Unicode::LRE,
                    });
                    label.push_str(text);
                    label.push(crate::licenses::Unicode::PDF);
                }
                _ => label.push_str(text),
            }
        }
        label
    }
}

impl Default for SemanticsLabelBuilder {
    fn default() -> SemanticsLabelBuilder {
        SemanticsLabelBuilder::new()
    }
}

// -- Accumulating what one render object has to say ---------------------------

/// Upstream's `_concatAttributedString`: joins two labels, wrapping the second
/// if it reads the other way.
///
/// # Not the same rules as [`SemanticsLabelBuilder`], and they live in the same
/// upstream file
///
/// Both join text and put directional marks around the pieces that need them,
/// and they disagree in three places. Worth having side by side, because
/// reaching for the wrong one produces a label that is subtly misread:
///
/// | | `SemanticsLabelBuilder` | this |
/// | --- | --- | --- |
/// | separator | `" "` by default | always `"\n"` |
/// | wraps when | both directions known **and** differ | they differ **and the other's is known** |
/// | the lone/first piece | never wrapped | wrapped, then returned |
///
/// The second row is the one that bites: here a piece with a direction joined
/// onto a piece with *none* counts as differing, so it is wrapped. The builder
/// requires both to be known and so leaves it bare.
///
/// The third follows from where the check sits -- upstream wraps `other`
/// *before* testing whether `this` is empty, so an empty `this` returns the
/// already-wrapped `other`.
fn concat_attributed_string(
    this_string: &AttributedString,
    this_direction: Option<TextDirection>,
    other_string: &AttributedString,
    other_direction: Option<TextDirection>,
) -> AttributedString {
    if other_string.string().is_empty() {
        return this_string.clone();
    }
    let mut other = other_string.clone();
    if this_direction != other_direction && other_direction.is_some() {
        let embedding = match other_direction {
            Some(TextDirection::Rtl) => crate::licenses::Unicode::RLE,
            // `is_some` above, and the two-variant enum, leave only Ltr.
            _ => crate::licenses::Unicode::LRE,
        };
        other = &(&AttributedString::new(embedding.to_string()) + &other)
            + &AttributedString::new(crate::licenses::Unicode::PDF.to_string());
    }
    if this_string.string().is_empty() {
        return other;
    }
    &(&this_string.clone() + &AttributedString::new(MERGED_LABEL_SEPARATOR)) + &other
}

/// The actions a node keeps even while it is blocking user actions.
///
/// Upstream's `_kUnblockedUserActions`: the two accessibility-focus
/// notifications. A blocked node still has to be *told* the reader moved onto
/// and off it -- blocking is about refusing to act, not about refusing to know
/// where the reader is.
pub const UNBLOCKED_USER_ACTIONS: &[SemanticsAction] = &[
    SemanticsAction::DidGainAccessibilityFocus,
    SemanticsAction::DidLoseAccessibilityFocus,
];

/// Upstream `SemanticsConfiguration`: what one render object has to say, before
/// it is decided whether it gets a node of its own.
///
/// # This is the accumulator, and [`SemanticsProperties`] is the result
///
/// A render object fills one of these in; the walk then either gives it a node
/// or folds it into its parent's with [`absorb`]. The two types carry nearly
/// the same fields for that reason, and the difference is that this one knows
/// how to be merged.
///
/// # The field set is this crate's, not upstream's
///
/// Upstream's carries about forty fields, most of which describe things this
/// port has no counterpart for -- platform view ids, link URLs, validation
/// results, roles, input types, traversal identifiers. What is here is the set
/// [`SemanticsProperties`] already models, which is what the bridges below
/// actually deliver. The **merge rules** are the part worth porting, and they
/// are the same rules whatever the field list is:
///
/// * a singular value is **first-wins** -- the parent's own beats the child's,
///   because the parent is the one being described;
/// * a string is first-wins spelled for strings, on emptiness rather than
///   nullness;
/// * a label or a hint **concatenates** rather than choosing, because two
///   things merged into one node have two things to say and a reader should
///   hear both;
/// * flags merge and actions union.
///
/// # Nothing in the walk calls these yet
///
/// [`absorb`] and [`is_compatible_with`] have **no caller outside the tests**.
/// The walk in this crate runs on [`SemanticsAnnotation`], which a render
/// object hands back from `describe_semantics`, and it does its own much
/// simpler thing: a text node yields to an enclosing label and that is the
/// whole of it. So this class is the rules, written down and held by tests,
/// waiting for a walk that consults them.
///
/// That is worth knowing because it is what let two rules that are **not**
/// upstream's live here undisturbed -- `hintOverrides` and `indexInParent` were
/// being refused a merge that upstream allows, and nothing outside a test could
/// have noticed. The tests are the only thing holding these rules, which is a
/// reason to be strict about them rather than a reason to relax.
///
/// [`absorb`]: SemanticsConfiguration::absorb
/// [`is_compatible_with`]: SemanticsConfiguration::is_compatible_with
/// Upstream `SemanticsConfiguration`.
///
/// # This is a model, not the path a node in this crate is built on
///
/// Upstream's render objects fill one of these in
/// `describeSemanticsConfiguration` and the framework assembles nodes from
/// them. **Here they do not**: [`crate::render::RenderBox::describe_semantics`]
/// answers a [`SemanticsAnnotation`], which is a smaller thing on purpose --
/// one `yields_to_a_label` flag standing in for the common case of upstream's
/// `excludeSemantics` and `MergeSemantics` both.
///
/// So this struct and its `absorb`/`is_compatible_with` rules are a faithful
/// port that the tree never asks -- `absorb` itself is called from tests and
/// nowhere else -- and three of its four flags are inert in the stronger
/// sense that **no code anywhere writes them**:
///
/// * `is_semantic_boundary` -- never set, never read.
/// * `explicit_child_nodes` -- never set; read once, in `absorb`'s own
///   `debug_assert`.
/// * `is_merging_semantics_of_descendants` -- never set, never read.
/// * `is_blocking_user_actions` -- **live**, and the exception that shows the
///   others are not: it is set in `absorb` and read in `has_action`.
///
/// # Upstream's one class is three things here, and only one is wired
///
/// * this struct -- the flags and the merge rules, tests only;
/// * [`crate::render_semantics::RenderSemanticsAnnotations`] -- `container`,
///   `explicit_child_nodes` and `exclude_semantics`, also constructed only in
///   tests;
/// * [`RenderSemantics`], which the [`Semantics`] widget actually builds, and
///   which carries **properties and an action handler and neither flag**.
///
/// Said here because the fields read as working machinery. A caller reaching
/// for `is_semantic_boundary` to make a card one node (upstream's
/// `Semantics(container: ..)`) would set it, see nothing happen, and have to
/// find this out the hard way. Making them live means giving the live
/// annotation path a distinction it currently folds away, which is a piece of
/// work rather than a flag.
#[derive(Clone, Debug, Default)]
pub struct SemanticsConfiguration {
    /// Upstream's `isSemanticBoundary`: whether this gets a node of its own
    /// whatever its parent wanted.
    pub is_semantic_boundary: bool,
    /// Upstream's `explicitChildNodes`: children get their own nodes rather
    /// than being folded in. [`absorb`](SemanticsConfiguration::absorb) asserts
    /// against being called on one of these.
    pub explicit_child_nodes: bool,
    /// Upstream's `isMergingSemanticsOfDescendants`.
    pub is_merging_semantics_of_descendants: bool,
    /// Upstream's `isBlockingUserActions`: this node refuses to act, but still
    /// hears where the reader is. See [`UNBLOCKED_USER_ACTIONS`].
    pub is_blocking_user_actions: bool,
    /// Upstream's `hasBeenAnnotated`: whether anything was actually said.
    ///
    /// The whole merge turns on it -- an un-annotated config is skipped by
    /// [`absorb`](SemanticsConfiguration::absorb) and accepted by
    /// [`is_compatible_with`](SemanticsConfiguration::is_compatible_with)
    /// without further questions, because there is nothing in it to conflict.
    pub has_been_annotated: bool,

    pub label: AttributedString,
    pub value: AttributedString,
    pub hint: AttributedString,
    pub increased_value: AttributedString,
    pub decreased_value: AttributedString,
    pub tooltip: String,
    /// Upstream's `SemanticsConfiguration.role`.
    pub role: SemanticsRole,
    /// Upstream's `identifier`, whose "unset" is the empty string rather than
    /// null -- which is why the merge tests it against `""`.
    pub identifier: String,
    pub text_direction: Option<TextDirection>,
    pub flags: SemanticsFlags,
    pub actions: Vec<SemanticsAction>,
    pub sort_key: Option<OrdinalSortKey>,
    pub hint_overrides: Option<SemanticsHintOverrides>,
    pub scroll_position: Option<f32>,
    pub scroll_extent_max: Option<f32>,
    pub scroll_extent_min: Option<f32>,
    pub scroll_index: Option<i32>,
    pub scroll_child_count: Option<i32>,
    pub index_in_parent: Option<i32>,
    /// Upstream's `tagsForChildren`, added to with
    /// [`add_tag_for_children`](SemanticsConfiguration::add_tag_for_children).
    pub tags_for_children: Vec<SemanticsTag>,
    /// Upstream's `accessibilityFocusBlockType`.
    ///
    /// Here because of its merge and not because of its readers: it is the one
    /// field in this class whose rule is **strongest-wins** rather than
    /// first-wins, and [`AccessibilityFocusBlockType::merge`] was written and
    /// tested before anything carried it, so the rule had no way to be reached.
    pub accessibility_focus_block_type: AccessibilityFocusBlockType,
}

impl SemanticsConfiguration {
    pub fn new() -> SemanticsConfiguration {
        SemanticsConfiguration::default()
    }

    /// Upstream's `addTagForChildren`.
    pub fn add_tag_for_children(&mut self, tag: SemanticsTag) {
        self.tags_for_children.push(tag);
    }

    /// The actions this config will hand over, which is not always the ones it
    /// has. Upstream's `_effectiveActionsAsBits`.
    fn effective_actions(&self) -> Vec<SemanticsAction> {
        if self.is_blocking_user_actions {
            self.actions
                .iter()
                .copied()
                .filter(|action| UNBLOCKED_USER_ACTIONS.contains(action))
                .collect()
        } else {
            self.actions.clone()
        }
    }

    /// Upstream's `_hasExplicitRole`: whether this config claims to *be*
    /// something, as opposed to merely having a trait.
    ///
    /// The membership is upstream's and the omission is the interesting part:
    /// **`isButton` is not a role.** A button is a trait a node can have
    /// alongside being something else, which is why two configs one of which is
    /// a button are allowed to merge.
    ///
    /// `isHeader` is a role only on the web -- upstream guards it with
    /// `kIsWeb`, and on every other platform a header is a trait. This crate's
    /// hosts are Windows, Android and macOS, so it is left out.
    pub fn has_explicit_role(&self) -> bool {
        self.flags.is_text_field
            || self.flags.is_slider
            || self.flags.is_link
            || self.flags.is_image
    }

    /// Upstream's `isCompatibleWith`: whether `other` can be folded into this
    /// without either losing something.
    ///
    /// Every check is the same question in a different slot -- **would both
    /// configs want to fill the same singular field?** Two labels can merge
    /// because labels concatenate; two *values* cannot, because a node has one
    /// value and a reader would hear only one of them.
    ///
    /// An un-annotated `other` is always compatible, and so is an un-annotated
    /// self: nothing in an empty config can conflict with anything.
    pub fn is_compatible_with(&self, other: Option<&SemanticsConfiguration>) -> bool {
        let Some(other) = other else {
            return true;
        };
        if !other.has_been_annotated || !self.has_been_annotated {
            return true;
        }
        // Two nodes that both handle the same action would give the reader one
        // gesture with two meanings.
        if self
            .actions
            .iter()
            .any(|action| other.actions.contains(action))
        {
            return false;
        }
        if self.flags.conflicts_with(&other.flags) {
            return false;
        }
        // Two things that each claim to *be* something. Separate from the flag
        // conflict above, and it is this check -- not that one -- that stops a
        // text field merging with a slider.
        if self.has_explicit_role() && other.has_explicit_role() {
            return false;
        }
        // A node has one value, and two of them is one too many.
        if !self.value.string().is_empty() && !other.value.string().is_empty() {
            return false;
        }
        // `hint_overrides` and `index_in_parent` are **not** asked about, and
        // this port used to ask. Upstream's list is
        // `platformViewId`, `maxValueLength`, `currentValueLength`,
        // `attributedValue`, `minValue`, `maxValue` -- every one of them
        // something a render object says about *itself*, where two claimants
        // really are two objects disagreeing. A hint override and a position in
        // a parent come from outside the object: an ancestor arranged them.
        // Two of those in one merge chain is that arrangement, not a
        // disagreement, and upstream lets `absorb` keep the outer one by
        // first-wins.
        //
        // Refusing them made this port split into two nodes where upstream
        // makes one, which a reader hears as an extra stop.
        true
    }

    /// Upstream's `absorb`: folds `child` into this config.
    ///
    /// Assumes [`is_compatible_with`](SemanticsConfiguration::is_compatible_with)
    /// already said yes -- upstream does too, and what happens otherwise is that
    /// the parent's value quietly wins, which is the first-wins rule doing
    /// exactly what it says and not what the caller meant.
    pub fn absorb(&mut self, child: &SemanticsConfiguration) {
        debug_assert!(
            !self.explicit_child_nodes,
            "a config whose children get their own nodes has nothing to absorb"
        );
        if !child.has_been_annotated {
            return;
        }

        for action in child.effective_actions() {
            if !self.actions.contains(&action) {
                self.actions.push(action);
            }
        }
        self.flags = self.flags.merge(&child.flags);
        // **Not** first-wins: a parent that blocks nothing still ends up
        // blocking if the child it swallowed did. Blocking is about what a
        // reader must not reach, and a merged node is reachable by every path
        // that reached either half -- so the stronger of the two is the only
        // answer that keeps the promise the child was making.
        self.accessibility_focus_block_type = self
            .accessibility_focus_block_type
            .merge(child.accessibility_focus_block_type);

        // First-wins, one slot at a time. The parent asked first.
        self.sort_key = self.sort_key.clone().or_else(|| child.sort_key.clone());
        self.hint_overrides = self
            .hint_overrides
            .clone()
            .or_else(|| child.hint_overrides.clone());
        self.scroll_position = self.scroll_position.or(child.scroll_position);
        self.scroll_extent_max = self.scroll_extent_max.or(child.scroll_extent_max);
        self.scroll_extent_min = self.scroll_extent_min.or(child.scroll_extent_min);
        self.scroll_index = self.scroll_index.or(child.scroll_index);
        self.scroll_child_count = self.scroll_child_count.or(child.scroll_child_count);
        self.index_in_parent = self.index_in_parent.or(child.index_in_parent);

        // **Before the labels**, because the labels are joined in this
        // direction and it may be the child's. Upstream's line order, and
        // moving it below the concatenation would wrap the child's label
        // against a direction that was about to become its own.
        self.text_direction = self.text_direction.or(child.text_direction);

        if self.identifier.is_empty() {
            self.identifier = child.identifier.clone();
        }

        self.label = concat_attributed_string(
            &self.label,
            self.text_direction,
            &child.label,
            child.text_direction,
        );
        // Values do not concatenate -- a node has one. First non-empty wins.
        if self.value.string().is_empty() {
            self.value = child.value.clone();
        }
        if self.increased_value.string().is_empty() {
            self.increased_value = child.increased_value.clone();
        }
        if self.decreased_value.string().is_empty() {
            self.decreased_value = child.decreased_value.clone();
        }
        self.hint = concat_attributed_string(
            &self.hint,
            self.text_direction,
            &child.hint,
            child.text_direction,
        );
        if self.tooltip.is_empty() {
            self.tooltip = child.tooltip.clone();
        }

        self.has_been_annotated = self.has_been_annotated || child.has_been_annotated;
    }

    /// Upstream's `copy()`. A plain clone here, because nothing in this field
    /// set is shared by reference -- upstream's copies the maps by hand for
    /// exactly that reason.
    pub fn copy(&self) -> SemanticsConfiguration {
        self.clone()
    }

    /// What the walk hands on once it has decided this config gets a node.
    ///
    /// Not upstream's -- upstream's `SemanticsNode.updateWith` takes the config
    /// directly. This is the seam to the [`SemanticsProperties`] this crate's
    /// collector already speaks.
    pub fn to_properties(&self) -> SemanticsProperties {
        SemanticsProperties {
            label: self.label.string().to_string(),
            value: self.value.string().to_string(),
            hint: self.hint.string().to_string(),
            increased_value: self.increased_value.string().to_string(),
            decreased_value: self.decreased_value.string().to_string(),
            // Dropped here until now, which is the other half of the same
            // hole: a config could carry a tip through every merge rule above
            // and this seam would throw it away on the way to the collector.
            tooltip: self.tooltip.clone(),
            role: self.role,
            text_direction: self.text_direction,
            flags: self.flags,
            actions: self
                .actions
                .iter()
                .fold(0, |bits, action| bits | *action as i32),
            scroll_position: self.scroll_position.unwrap_or(f32::NAN),
            scroll_extent_max: self.scroll_extent_max.unwrap_or(f32::NAN),
            scroll_extent_min: self.scroll_extent_min.unwrap_or(f32::NAN),
            scroll_child_count: self.scroll_child_count,
            scroll_index: self.scroll_index,
        }
    }
}

// -- How a render object's children are grouped into nodes --------------------

/// Upstream `ChildSemanticsConfigurationsResult`: how one render object's
/// children are to be arranged into semantics nodes.
///
/// A render object that wants a say in this returns one of these from its
/// `childConfigurationsDelegate`, and the walk obeys it. There are two things
/// it can ask for:
///
/// * **merge up** -- fold this child's description into mine, so the reader
///   hears one thing where there were two. What a `ListTile` does with its
///   title and its subtitle.
/// * **a sibling merge group** -- take these children, merge them with each
///   other, and hang the result beside me rather than under me. Upstream's own
///   example is a render object whose child forms a node: anything from that
///   child's sibling groups attaches as a *sibling* of the child's node, not as
///   a child of it.
///
/// Neither is a promise. A config in either list that is a semantics boundary,
/// or that conflicts with the others it is being merged with, is pulled back
/// out and gets a node of its own -- upstream says so on both fields, and it is
/// the same [`SemanticsConfiguration::is_compatible_with`] that decides.
///
/// # Configs are held by handle, because the identity is the object
///
/// Upstream's duplicate check is a `Set<SemanticsConfiguration>` under Dart's
/// default identity equality -- the same *object* twice, not two objects that
/// describe themselves alike. Two children that happen to say exactly the same
/// thing are still two children, and both belong in the list.
///
/// A [`SemanticsConfiguration`] here is a value with no identity of its own, so
/// these are `Rc`s and the check is [`Rc::ptr_eq`]. The same problem
/// [`SemanticsTag`] has, solved the other way round -- a tag is constructed
/// once and handed about, so it carries an id; a config is already behind a
/// handle by the time it gets here.
#[derive(Clone, Default)]
pub struct ChildSemanticsConfigurationsResult {
    /// Upstream's `mergeUp`.
    pub merge_up: Vec<Rc<SemanticsConfiguration>>,
    /// Upstream's `siblingMergeGroups`.
    pub sibling_merge_groups: Vec<Vec<Rc<SemanticsConfiguration>>>,
}

impl ChildSemanticsConfigurationsResult {
    /// Every config named anywhere in this result, in the order the duplicate
    /// check walks them: the merge-ups first, then each sibling group.
    pub fn all(&self) -> Vec<Rc<SemanticsConfiguration>> {
        self.merge_up
            .iter()
            .chain(self.sibling_merge_groups.iter().flatten())
            .cloned()
            .collect()
    }
}

/// Upstream `ChildSemanticsConfigurationsResultBuilder`.
///
/// Mark each child config as one or the other, then [`build`].
///
/// [`build`]: ChildSemanticsConfigurationsResultBuilder::build
#[derive(Default)]
pub struct ChildSemanticsConfigurationsResultBuilder {
    merge_up: Vec<Rc<SemanticsConfiguration>>,
    sibling_merge_groups: Vec<Vec<Rc<SemanticsConfiguration>>>,
}

impl ChildSemanticsConfigurationsResultBuilder {
    pub fn new() -> ChildSemanticsConfigurationsResultBuilder {
        ChildSemanticsConfigurationsResultBuilder::default()
    }

    /// Upstream's `markAsMergeUp`.
    pub fn mark_as_merge_up(&mut self, config: Rc<SemanticsConfiguration>) {
        self.merge_up.push(config);
    }

    /// Upstream's `markAsSiblingMergeGroup`.
    pub fn mark_as_sibling_merge_group(&mut self, configs: Vec<Rc<SemanticsConfiguration>>) {
        self.sibling_merge_groups.push(configs);
    }

    /// Upstream's `build`, assert included.
    ///
    /// **A config may be named once, across both lists.** Upstream's assert
    /// spells out how it goes wrong -- "this can happen if the same
    /// `SemanticsConfiguration` was marked twice in `markAsMergeUp` and/or
    /// `markAsSiblingMergeGroup`" -- and what it prevents is one child being
    /// described in two places at once, which a reader hears as the same thing
    /// twice with no way to tell they are one.
    pub fn build(&self) -> ChildSemanticsConfigurationsResult {
        let result = ChildSemanticsConfigurationsResult {
            merge_up: self.merge_up.clone(),
            sibling_merge_groups: self.sibling_merge_groups.clone(),
        };
        debug_assert!(
            !has_duplicate(&result.all()),
            "the same SemanticsConfiguration was marked more than once"
        );
        result
    }

    /// Whether a config has already been marked, so a caller can ask instead of
    /// tripping the assert.
    ///
    /// Not upstream's -- upstream builds the set inside the assert and throws it
    /// away, which is the right shape for a check that only runs in debug. This
    /// is here because a delegate assembling groups from a loop has no other way
    /// to find out.
    pub fn is_marked(&self, config: &Rc<SemanticsConfiguration>) -> bool {
        self.merge_up
            .iter()
            .chain(self.sibling_merge_groups.iter().flatten())
            .any(|marked| Rc::ptr_eq(marked, config))
    }
}

/// Whether any handle appears twice. Quadratic, and deliberately so: it runs
/// under `debug_assert` over one render object's children, and a hash of
/// pointers would need the configs to outlive the set.
fn has_duplicate(configs: &[Rc<SemanticsConfiguration>]) -> bool {
    configs.iter().enumerate().any(|(at, config)| {
        configs[at + 1..]
            .iter()
            .any(|other| Rc::ptr_eq(config, other))
    })
}

// -- The snapshot a node carries ----------------------------------------------

/// Upstream `SemanticsData`: everything about one node, frozen.
///
/// [`SemanticsConfiguration`] is what a render object fills in and what merging
/// works over; this is what comes out the other side, and it is immutable
/// because it is what gets compared against last frame's to decide whether the
/// platform needs telling.
///
/// # Its constructor is mostly assertions, and they are the content
///
/// Six of upstream's eight say the same thing about six different fields:
/// **text with no direction is not allowed**. A label, a value, an increased or
/// decreased value, a hint or a tooltip may be empty, and if it is not then
/// `textDirection` must be set.
///
/// The reason is downstream: the embedder hands the string to a screen reader
/// along with a direction, and with none to hand it the reader guesses from the
/// characters. That is right for text that is all one script and wrong for
/// everything else -- a phone number in an Arabic interface, a name in an
/// English one -- and it is wrong silently, which is why this is caught here
/// rather than noticed by a reader.
///
/// The other two: a heading level is 0 to 6, because that is the range
/// `aria-level` and the platform bridges accept; and a link URL requires the
/// `isLink` flag, because the embedder reads the URL off a node it has already
/// decided is a link and would otherwise never look.
///
/// # The field set is this crate's
///
/// The same narrowing [`SemanticsConfiguration`] carries, for the same reason:
/// upstream's thirty-odd fields include platform view ids, validation results,
/// roles and traversal identifiers this port has no counterpart for. The
/// assertions are ported for the fields that exist.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsData {
    pub flags: SemanticsFlags,
    /// The actions this node accepts, as a bit set -- the same encoding
    /// [`SemanticsProperties::actions`] uses, because it is the one that goes
    /// over the wire.
    pub actions: i32,
    pub identifier: String,
    pub label: AttributedString,
    pub value: AttributedString,
    pub increased_value: AttributedString,
    pub decreased_value: AttributedString,
    pub hint: AttributedString,
    pub tooltip: String,
    pub text_direction: Option<TextDirection>,
    /// Where the node is, in its parent's coordinates.
    pub rect: Rect,
    /// Upstream's `headingLevel`: 0 for "not a heading", 1 to 6 otherwise.
    pub heading_level: u8,
    pub scroll_index: Option<i32>,
    pub scroll_child_count: Option<i32>,
    pub scroll_position: Option<f32>,
    pub scroll_extent_max: Option<f32>,
    pub scroll_extent_min: Option<f32>,
    /// Upstream's `tags`, which a parent uses to find nodes it marked.
    pub tags: Vec<SemanticsTag>,
    /// Upstream's `customSemanticsActionIds`, the ids
    /// [`CustomSemanticsAction::identifier`] handed out.
    pub custom_action_ids: Vec<i32>,
}

impl Default for SemanticsData {
    /// An empty snapshot at the origin.
    ///
    /// Hand-written rather than derived because [`Rect`] has no `Default` --
    /// deliberately, since a zero rectangle is a real answer ("here, with no
    /// size") and not an absence, and a type that hands one out for free
    /// invites it being read as one.
    fn default() -> SemanticsData {
        SemanticsData {
            flags: SemanticsFlags::default(),
            actions: 0,
            identifier: String::new(),
            label: AttributedString::default(),
            value: AttributedString::default(),
            increased_value: AttributedString::default(),
            decreased_value: AttributedString::default(),
            hint: AttributedString::default(),
            tooltip: String::new(),
            text_direction: None,
            rect: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
            heading_level: 0,
            scroll_index: None,
            scroll_child_count: None,
            scroll_position: None,
            scroll_extent_max: None,
            scroll_extent_min: None,
            tags: Vec::new(),
            custom_action_ids: Vec::new(),
        }
    }
}

impl SemanticsData {
    /// Checks upstream's constructor assertions.
    ///
    /// Split out from a constructor because this crate's `SemanticsData` is a
    /// plain struct a caller fills in field by field -- there is no one place
    /// every value passes through. A caller that built one by hand can ask;
    /// [`crate::semantics::flush`]'s successor will ask on every node.
    ///
    /// Returns the first thing wrong, in upstream's own words where it has
    /// them, or `None` if the snapshot is sound.
    pub fn check(&self) -> Option<String> {
        // Upstream's six, in its order.
        for (what, text) in [
            ("tooltip", self.tooltip.as_str()),
            ("label", self.label.string()),
            ("value", self.value.string()),
            ("decreasedValue", self.decreased_value.string()),
            ("increasedValue", self.increased_value.string()),
            ("hint", self.hint.string()),
        ] {
            if !text.is_empty() && self.text_direction.is_none() {
                return Some(format!(
                    "A SemanticsData object with {what} \"{text}\" had a null textDirection."
                ));
            }
        }
        if self.heading_level > 6 {
            return Some("Heading level must be between 0 and 6".to_string());
        }
        None
    }

    /// Whether this node accepts `action`. Upstream's `hasAction`.
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions & action as i32 != 0
    }

    /// Whether `tag` was put on this node. Upstream's `SemanticsNode.isTagged`,
    /// which reads the same set.
    ///
    /// Compared by tag identity, not by name -- see [`SemanticsTag`], where the
    /// reason is that two subsystems picking the same word must not collide.
    pub fn is_tagged(&self, tag: &SemanticsTag) -> bool {
        self.tags.contains(tag)
    }

    /// The snapshot a config becomes once the walk has given it a rectangle.
    ///
    /// The direction is carried across whether or not there is text, and
    /// [`SemanticsData::check`] is what says whether that was enough.
    pub fn from_configuration(config: &SemanticsConfiguration, rect: Rect) -> SemanticsData {
        SemanticsData {
            flags: config.flags,
            actions: config
                .actions
                .iter()
                .fold(0, |bits, action| bits | *action as i32),
            identifier: config.identifier.clone(),
            label: config.label.clone(),
            value: config.value.clone(),
            increased_value: config.increased_value.clone(),
            decreased_value: config.decreased_value.clone(),
            hint: config.hint.clone(),
            tooltip: config.tooltip.clone(),
            text_direction: config.text_direction,
            rect,
            heading_level: 0,
            scroll_index: config.scroll_index,
            scroll_child_count: config.scroll_child_count,
            scroll_position: config.scroll_position,
            scroll_extent_max: config.scroll_extent_max,
            scroll_extent_min: config.scroll_extent_min,
            // Upstream's `tagsForChildren` are put on the *children*, not on
            // the node that named them -- so a config's own snapshot starts
            // untagged and the walk applies them going down.
            tags: Vec::new(),
            custom_action_ids: Vec::new(),
        }
    }
}

// -- Who owns the tree the platform is holding --------------------------------

/// Upstream `SemanticsOwner`: the tree that has been handed over, and the
/// checks that run before the next one is.
///
/// # Upstream tracks dirty nodes; this rebuilds and diffs
///
/// Upstream's owner keeps a set of dirty `SemanticsNode`s, sorts them
/// shallowest-first, and sends an update built from those alone --
/// incrementally, because its nodes are long-lived objects that mutate in
/// place. This crate walks the render tree afresh whenever anything marked
/// ([`mark_needs_update`]) and compares the result against what was last sent,
/// which the `sent` field has documented since it was written.
///
/// Both arrive at "tell the platform only what changed". The difference is
/// where the bookkeeping lives, and it follows from the node representation:
/// a `SemanticsNode` here is a value in a flat list, so there is nothing to
/// mark dirty and nothing to sort by depth.
///
/// So what an owner is *for* here is the other half of upstream's
/// `sendSemanticsUpdate`: holding the tree, answering questions about it, and
/// running the check that upstream puts in front of every send.
pub struct SemanticsOwner {
    nodes: Vec<SemanticsNode>,
}

impl SemanticsOwner {
    /// An owner holding `nodes` -- what [`flush`] produced, or what [`tree`]
    /// answers.
    pub fn new(nodes: Vec<SemanticsNode>) -> SemanticsOwner {
        SemanticsOwner { nodes }
    }

    /// The owner over the tree the platform is currently holding.
    pub fn current() -> SemanticsOwner {
        SemanticsOwner::new(tree())
    }

    /// Upstream's `rootSemanticsNode`, which it reads as `_nodes[0]`. Here the
    /// root is [`ROOT_ID`] for the same reason: the view's own node is the one
    /// everything painted into it hangs from.
    pub fn root(&self) -> Option<&SemanticsNode> {
        self.node(ROOT_ID)
    }

    /// Upstream's `getSemanticsNode`.
    pub fn node(&self, id: i32) -> Option<&SemanticsNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn nodes(&self) -> &[SemanticsNode] {
        &self.nodes
    }

    /// Upstream's `dispose`: the owner forgets everything.
    pub fn dispose(&mut self) {
        self.nodes.clear();
    }

    /// Upstream's assertion in front of every `sendSemanticsUpdate`: **no
    /// invisible node may be in the tree**.
    ///
    /// An invisible node is one whose rectangle is empty and which is not
    /// merged into a visible parent. Upstream spends four error hints on why it
    /// matters, and the middle one is the whole of it: such a node "does not
    /// provide any visual indication when the user selects it via accessibility
    /// technologies". The reader hears something announced and there is nothing
    /// on the glass to look at -- worse than the thing being absent, because
    /// they now believe they have missed it.
    ///
    /// One rule makes it less obvious than it sounds, and it is upstream's:
    /// **the root may be invisible while it has no children.** An application
    /// that has not laid out yet has a zero-size root and nothing under it,
    /// which is a state to pass through rather than a bug.
    ///
    /// Upstream has a second rule -- a node that merges all its descendants
    /// stops the walk, because their rectangles describe nothing separately
    /// reachable. It has **no counterpart here and needs none**: merging in
    /// this crate happens while the tree is being collected (`labelled_depth`
    /// in [`Collector`]), so a merged descendant never becomes a node at all.
    /// There is nothing in the list to skip.
    ///
    /// Answers the offending nodes' ids, empty when the tree is sound.
    pub fn invisible_nodes(&self) -> Vec<i32> {
        let mut found = Vec::new();
        let Some(root) = self.root() else {
            return found;
        };
        if !root.children.is_empty() && is_empty_rect(root) {
            found.push(root.id);
        } else {
            for child in &root.children {
                self.find_invisible(*child, &mut found);
            }
        }
        found
    }

    fn find_invisible(&self, id: i32, found: &mut Vec<i32>) {
        let Some(node) = self.node(id) else {
            return;
        };
        if is_empty_rect(node) {
            // Upstream adds it and does *not* descend: a subtree under an
            // invisible node is invisible for the same reason, and reporting
            // all of it would bury the one that has to be fixed.
            found.push(node.id);
            return;
        }
        for child in &node.children {
            self.find_invisible(*child, found);
        }
    }
}

/// Upstream's `node.rect.isEmpty`.
///
/// `dart:ui`'s is `left >= right || top >= bottom` -- **either** extent
/// collapsing, not both. A node one pixel wide and zero tall is exactly as
/// unreachable as one that is zero by zero, so `&&` here would let a whole
/// class of invisible node through.
fn is_empty_rect(node: &SemanticsNode) -> bool {
    node.width() <= 0.0 || node.height() <= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, leaf, many};
    use crate::licenses::Unicode;
    use crate::render::{EdgeInsets, RenderFlex, RenderPadding};
    use crate::widgets::SizedBox;
    use std::cell::Cell;
    use std::cmp::Ordering;

    // -- The line that says what page you are on, tick 269 -------------------
    //
    // Upstream wraps an app bar's title in `Semantics(namesRoute: ...,
    // header: true)`. This port's `AppBar` wrapped it in nothing, so the one
    // line on the screen that names the page was an ordinary run of words to
    // a screen reader.

    #[test]
    fn an_app_bar_title_names_the_page_everywhere_but_apple() {
        // `null` on Apple and not `false`: VoiceOver announces a route change
        // on its own, so a second announcement is a repetition rather than an
        // aid. The third platform branch in this port's semantics, after the
        // radio's `selected` and its unselected hint.
        use crate::editable_text::TargetPlatform;
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            let title = SemanticsProperties::route_header("Inbox", platform);
            assert!(title.flags.names_route, "{platform:?}");
            assert!(title.flags.is_header, "{platform:?}: and still a heading");
        }
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            let title = SemanticsProperties::route_header("Inbox", platform);
            assert!(!title.flags.names_route, "{platform:?}");
            assert!(
                title.flags.is_header,
                "{platform:?}: the heading claim is not the platform's business"
            );
        }
    }

    #[test]
    fn a_plain_heading_does_not_name_a_route() {
        // There is one route-naming node per route, and a section heading in
        // the middle of a page is not it -- a reader arriving at a new screen
        // would be told the name of the third section.
        let heading = SemanticsProperties::header("Recently played");
        assert!(heading.flags.is_header);
        assert!(!heading.flags.names_route);
    }

    #[test]
    fn hidden_is_not_the_same_as_absent() {
        // A hidden node keeps its place in the tree: "3 of 40" still counts
        // it and a reader moving forward still arrives at it. Leaving a node
        // out removes it from both.
        let mut covered = SemanticsProperties::label("Behind the sheet");
        covered.flags.is_hidden = true;
        assert!(covered.flags.is_hidden);
        assert_eq!(
            covered.label, "Behind the sheet",
            "and it still has a label"
        );
        assert!(!SemanticsProperties::label("On top").flags.is_hidden);
    }

    #[test]
    fn the_two_new_claims_reach_the_abi() {
        use crate::app::pack_semantics_flags;
        let mask = (1 << 24) | (1 << 25);
        assert_eq!(pack_semantics_flags(&SemanticsFlags::default()) & mask, 0);
        assert_eq!(
            pack_semantics_flags(&SemanticsFlags {
                names_route: true,
                ..SemanticsFlags::default()
            }) & mask,
            1 << 24
        );
        assert_eq!(
            pack_semantics_flags(&SemanticsFlags {
                is_hidden: true,
                ..SemanticsFlags::default()
            }) & mask,
            1 << 25
        );
    }

    #[test]
    fn the_bar_describes_its_own_title() {
        // The widget end. Wrapping the title in `describe` is what makes the
        // resolver's answer reach anything, and the bar wrapped it in nothing
        // -- so this is the assertion that would have been red all along.
        set_enabled(true);
        let nodes = describe_tree(
            crate::framework::component(crate::components::AppBar::new("Inbox")),
            Size::new(400.0, 200.0),
        );
        set_enabled(false);
        let title = nodes
            .iter()
            .find(|node| node.properties.label == "Inbox")
            .expect("the bar's title is described");
        assert!(title.properties.flags.is_header, "it is a heading");
        assert!(
            title.properties.flags.names_route,
            "and on this platform it names the page"
        );
    }

    // -- "Not selected" is a thing to say, tick 268 --------------------------
    //
    // The bridge said it plainly:
    //
    //     out.flags.isSelected = (in.flags & kRfSemanticsIsSelected) != 0
    //                                ? SemanticsTristate::kTrue
    //                                : SemanticsTristate::kNone;
    //
    // False became "no opinion", not "false". A tab that is selectable and
    // currently is not crossed as a node with nothing to say about it, and a
    // reader that would have said "not selected" said nothing. `isFocused`
    // was the same two lines below.

    #[test]
    fn every_upstream_tristate_is_one_here_now() {
        // Seven of them upstream. `isChecked` is four-valued and the other
        // six are three-valued, and this port had all seven as booleans until
        // tick 266. A boolean can say *yes* and can say *nothing*; the state a
        // reader most needs -- "no, and I am the kind of thing that can be" --
        // is the one it cannot reach.
        let plain = SemanticsFlags::default();
        assert_eq!(plain.checked, SemanticsCheckState::None);
        for state in [
            plain.toggled,
            plain.expanded,
            plain.required,
            plain.selected,
            plain.focused,
        ] {
            assert_eq!(state, SemanticsTristate::None);
        }
        // `enabled` is still the older pair, which says the same three things
        // in two fields -- so it is not on this list and is not a gap.
        assert!(!plain.has_enabled_state);
    }

    #[test]
    fn a_field_that_does_not_hold_the_keyboard_says_so() {
        // Not silence: a text field is the kind of thing that can be focused,
        // so "not focused" is a claim it should make. A heading is not, and
        // makes none.
        let field = SemanticsProperties::text_field("Name", "");
        assert_eq!(
            field.flags.focused,
            SemanticsTristate::None,
            "until something says which way"
        );
        let mut focused = field;
        focused.flags.focused = SemanticsTristate::False;
        assert!(focused.flags.focused.is_set());
        assert_ne!(focused.flags.focused, SemanticsTristate::None);

        assert_eq!(
            SemanticsProperties::label("A heading").flags.focused,
            SemanticsTristate::None
        );
    }

    #[test]
    fn an_unfocused_text_field_says_not_focused_rather_than_nothing() {
        // The widget end of it, which the type's own tests cannot see: a
        // field that only ever *raised* the flag when it held the keyboard
        // would leave every other field silent, which is the collapse this
        // tick is about, one level up.
        set_enabled(true);
        let nodes = describe_tree(
            crate::framework::stateful(crate::editable::TextField::new(7).with_placeholder("Name")),
            Size::new(300.0, 200.0),
        );
        set_enabled(false);
        let field = nodes
            .iter()
            .find(|node| node.id == node_id_for(7))
            .expect("the field is described");
        assert!(
            field.properties.flags.focused.is_set(),
            "a field says which way it is, not nothing"
        );
        assert_eq!(field.properties.flags.focused, SemanticsTristate::False);
    }

    #[test]
    fn a_row_holding_the_focused_field_is_the_focused_stop() {
        // This test used to assert the opposite, under the name
        // "...keeps_the_first_answer", with the reasoning that a row is not
        // what holds the keyboard. That reasoning is about a row that has
        // children; a **folded** node has none -- it *is* the field, as far as
        // a reader is concerned, because the field stopped being a stop of its
        // own. Upstream merges the tristate as true-beats-false
        // (`semantics.dart:1136`), and first-wins would have made the answer
        // depend on which child the walk reached first.
        let mut row = said("Row");
        row.flags.focused = SemanticsTristate::False;
        let mut field = said("Name");
        field.flags.focused = SemanticsTristate::True;
        assert_eq!(
            row.flags.merge(&field.flags).focused,
            SemanticsTristate::True
        );
        // And a row with no opinion takes the field's.
        let plain = said("Row");
        assert_eq!(
            plain.flags.merge(&field.flags).focused,
            SemanticsTristate::True
        );
    }

    #[test]
    fn the_two_new_pairs_reach_the_abi_and_false_still_raises_its_has_bit() {
        use crate::app::pack_semantics_flags;
        let plain = SemanticsFlags::default();
        let mask = (1 << 13) | (1 << 14) | (1 << 22) | (1 << 23);
        assert_eq!(pack_semantics_flags(&plain) & mask, 0);

        // Selected: has-bit alone for false, both for true.
        let unselected = SemanticsFlags {
            selected: SemanticsTristate::False,
            ..plain
        };
        assert_eq!(pack_semantics_flags(&unselected) & mask, 1 << 22);
        let selected = SemanticsFlags {
            selected: SemanticsTristate::True,
            ..plain
        };
        assert_eq!(
            pack_semantics_flags(&selected) & mask,
            (1 << 22) | (1 << 13)
        );

        // Focused, the same shape on its own pair -- and the two do not share
        // bits, which is what makes a focused unselected tab expressible.
        let both = SemanticsFlags {
            selected: SemanticsTristate::False,
            focused: SemanticsTristate::True,
            ..plain
        };
        assert_eq!(
            pack_semantics_flags(&both) & mask,
            (1 << 22) | (1 << 23) | (1 << 14)
        );
    }

    // -- A switch is not a checkbox, tick 267 --------------------------------
    //
    // Upstream's two controls raise two different flags:
    //
    //     Switch:   Semantics(toggled: widget.value)
    //     Checkbox: Semantics(checked: widget.value ?? false,
    //                         mixed: widget.tristate ? widget.value == null : null)
    //
    // A reader says "on"/"off" for one and "checked"/"not checked" for the
    // other. This port had only the checked flag, so its `Switch` used it --
    // and three tests asserted that it did.

    #[test]
    fn a_switch_and_a_checkbox_raise_different_flags() {
        let switch = SemanticsProperties::toggle("Notifications", true);
        let checkbox = SemanticsProperties::check("Remember me", Some(true));

        assert_eq!(switch.flags.toggled, SemanticsTristate::True);
        assert_eq!(switch.flags.checked, SemanticsCheckState::None);

        assert_eq!(checkbox.flags.checked, SemanticsCheckState::Checked);
        assert_eq!(checkbox.flags.toggled, SemanticsTristate::None);

        // Both are on, and nothing that reads them out would say the same
        // words about the two -- which is the whole content of the pair.
        assert_ne!(switch.flags, checkbox.flags);
    }

    #[test]
    fn a_tristate_checkbox_can_be_partly_checked_and_a_switch_cannot() {
        // Upstream passes `mixed` only when `tristate` is set, and there is
        // no "partly on" anywhere in the semantics API -- a switch is one way
        // or the other.
        assert_eq!(
            SemanticsProperties::check("Select all", None).flags.checked,
            SemanticsCheckState::Mixed
        );
        // `SemanticsTristate` has three values and none of them is mixed.
        assert_eq!(SemanticsTristate::of(true), SemanticsTristate::True);
        assert_eq!(SemanticsTristate::of(false), SemanticsTristate::False);
    }

    #[test]
    fn having_no_opinion_is_told_apart_from_being_off() {
        // The reason for three values rather than a bool: a heading is not a
        // switch that is off, and a reader that cannot tell them apart says
        // "off" about the heading.
        assert!(!SemanticsTristate::None.is_set());
        assert!(SemanticsTristate::False.is_set());
        assert_eq!(SemanticsTristate::default(), SemanticsTristate::None);
        assert_eq!(
            SemanticsProperties::label("A heading").flags.toggled,
            SemanticsTristate::None
        );
    }

    #[test]
    fn a_tristate_merges_on_first_then_off_then_no_opinion() {
        // Upstream's order (`semantics.dart:1136`), and **symmetric** -- which
        // is the point. The rule here used to be first-wins, so the same two
        // nodes gave different answers depending on which was folded into
        // which; these two assertions were `True` and `False` before.
        assert_eq!(
            SemanticsTristate::True.merge(SemanticsTristate::False),
            SemanticsTristate::True
        );
        assert_eq!(
            SemanticsTristate::False.merge(SemanticsTristate::True),
            SemanticsTristate::True
        );
        // And a node with no opinion takes the other's.
        assert_eq!(
            SemanticsTristate::None.merge(SemanticsTristate::False),
            SemanticsTristate::False
        );
        assert_eq!(
            SemanticsTristate::True.merge(SemanticsTristate::None),
            SemanticsTristate::True
        );

        // The two kinds now agree on a disagreement, which is upstream's
        // shape: the check state has a fourth value, but it is reserved for a
        // control that is itself partly ticked rather than reached by folding.
        assert_eq!(
            SemanticsCheckState::Checked.merge(SemanticsCheckState::Unchecked),
            SemanticsCheckState::Checked
        );
    }

    #[test]
    fn the_three_tristates_reach_the_abi_as_paired_bits() {
        use crate::app::pack_semantics_flags;
        let bits = |flags| pack_semantics_flags(&flags);
        let plain = SemanticsFlags::default();

        // Nothing set: none of the six bits.
        let mask = (1 << 16) | (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 21);
        assert_eq!(bits(plain) & mask, 0);

        for (state, has, is) in [
            (
                SemanticsFlags {
                    toggled: SemanticsTristate::False,
                    ..plain
                },
                1 << 16,
                1 << 17,
            ),
            (
                SemanticsFlags {
                    expanded: SemanticsTristate::False,
                    ..plain
                },
                1 << 18,
                1 << 19,
            ),
            (
                SemanticsFlags {
                    required: SemanticsTristate::False,
                    ..plain
                },
                1 << 20,
                1 << 21,
            ),
        ] {
            // False raises the "has it" bit and not the "is it" one, which is
            // the whole point of the pair: without the first, "off" is not
            // sayable at all.
            assert_eq!(bits(state) & mask, has);
            let _ = is;
        }

        let on = SemanticsFlags {
            toggled: SemanticsTristate::True,
            ..plain
        };
        assert_eq!(bits(on) & mask, (1 << 16) | (1 << 17));
    }

    // -- Saying "partly checked", tick 266 -----------------------------------
    //
    // The engine's `SemanticsCheckState` has four values and this port
    // carried two booleans, which say three. The whole chain agreed: two bits
    // in the ABI, and a ternary in `runtime_controller.cc` that could only
    // ever produce `kTrue` or `kFalse`.
    //
    // Meanwhile this port *has* tristate checkboxes -- `ControlListTile`'s
    // value is an `Option<bool>` whose doc says "`None` is the indeterminate
    // state" -- so the half-checked box above a list announced as **not
    // checked**, which is one of the two things it is not.

    #[test]
    fn a_half_checked_box_is_neither_checked_nor_unchecked() {
        // The value the two booleans had nowhere to come from.
        assert_eq!(SemanticsCheckState::of(None), SemanticsCheckState::Mixed);
        assert_ne!(SemanticsCheckState::Mixed, SemanticsCheckState::Checked);
        assert_ne!(SemanticsCheckState::Mixed, SemanticsCheckState::Unchecked);
        // And it is still a box: a reader should say something about it, just
        // not "checked" or "unchecked".
        assert!(SemanticsCheckState::Mixed.is_checkable());
    }

    #[test]
    fn a_thing_with_no_box_is_told_apart_from_one_with_an_empty_box() {
        // The distinction the old `has_checked_state` boolean carried, kept:
        // without it a reader announces "not checked" about a label.
        assert!(!SemanticsCheckState::None.is_checkable());
        assert!(SemanticsCheckState::Unchecked.is_checkable());
        assert_eq!(SemanticsCheckState::default(), SemanticsCheckState::None);
        assert_eq!(
            SemanticsFlags::default().checked,
            SemanticsCheckState::None,
            "a plain node is not checkable"
        );
    }

    #[test]
    fn mixed_is_what_a_box_says_about_itself_and_not_what_disagreement_makes() {
        // This asserted the opposite, under the name
        // "two_boxes_that_disagree_merge_into_a_mixed_one", and the reasoning
        // read well: a checked and an unchecked thing folded together are not
        // simply "checked". But it is not upstream's
        // (`semantics.dart:1101`, mixed > checked > unchecked > none), and
        // upstream is right about what the value *means*: **mixed is what a
        // control says about itself** when it stands over some ticked and some
        // unticked children -- a parent checkbox. Reaching it by disagreement
        // announces a partly-ticked control that does not exist.
        assert_eq!(
            SemanticsCheckState::Checked.merge(SemanticsCheckState::Unchecked),
            SemanticsCheckState::Checked
        );
        assert_eq!(
            SemanticsCheckState::Unchecked.merge(SemanticsCheckState::Checked),
            SemanticsCheckState::Checked
        );
        // And a control that really is mixed keeps saying so.
        assert_eq!(
            SemanticsCheckState::Mixed.merge(SemanticsCheckState::Checked),
            SemanticsCheckState::Mixed
        );

        // Agreeing keeps the answer, and a node with no box takes the other's
        // outright rather than dragging it to mixed.
        assert_eq!(
            SemanticsCheckState::Checked.merge(SemanticsCheckState::Checked),
            SemanticsCheckState::Checked
        );
        assert_eq!(
            SemanticsCheckState::None.merge(SemanticsCheckState::Unchecked),
            SemanticsCheckState::Unchecked
        );
        assert_eq!(
            SemanticsCheckState::Unchecked.merge(SemanticsCheckState::None),
            SemanticsCheckState::Unchecked
        );
        assert_eq!(
            SemanticsCheckState::None.merge(SemanticsCheckState::None),
            SemanticsCheckState::None
        );
    }

    #[test]
    fn two_checkable_nodes_conflict_whichever_way_each_is_set() {
        // `has_conflict` grouped `is_checked` with the plain bools and tested
        // "both true", so an unchecked node and a checked one were said not
        // to conflict -- and merging them loses which was which, which is the
        // definition of one.
        let mut checked = said("Yes");
        checked.flags.checked = SemanticsCheckState::Checked;
        let mut unchecked = said("No");
        unchecked.flags.checked = SemanticsCheckState::Unchecked;
        assert!(checked.flags.conflicts_with(&unchecked.flags));

        // And a node with no box does not conflict with one that has one.
        let plain = said("Label");
        assert!(!plain.flags.conflicts_with(&checked.flags));
    }

    #[test]
    fn the_mixed_state_reaches_the_abi_as_a_third_bit() {
        // Three bits for four states: "checkable" gates the other two, and
        // "mixed" outranks "checked". A sender that never raises the third
        // bit is unchanged, which is what makes this a widening rather than a
        // break.
        use crate::app::pack_semantics_flags;
        let bits = |state| {
            pack_semantics_flags(&SemanticsFlags {
                checked: state,
                ..SemanticsFlags::default()
            })
        };
        let checkable = 1 << 9;
        let is_checked = 1 << 10;
        let mixed = 1 << 15;

        assert_eq!(
            bits(SemanticsCheckState::None) & (checkable | is_checked | mixed),
            0
        );
        assert_eq!(
            bits(SemanticsCheckState::Unchecked) & (checkable | is_checked | mixed),
            checkable
        );
        assert_eq!(
            bits(SemanticsCheckState::Checked) & (checkable | is_checked | mixed),
            checkable | is_checked
        );
        assert_eq!(
            bits(SemanticsCheckState::Mixed) & (checkable | is_checked | mixed),
            checkable | mixed,
            "mixed is not also checked"
        );
    }

    // -- The thirteen actions this port did not have, tick 265 ---------------
    //
    // `ffi_tables.py` found them, against an enum whose own doc says "four
    // copies of one set of bits upstream; this is the fifth, and it has to
    // match".

    #[test]
    fn every_bit_from_zero_to_twenty_five_has_exactly_one_action() {
        // Upstream's list is dense: twenty-six actions on twenty-six
        // consecutive bits, no gaps and no spares. This port's had nine holes
        // in it -- 9 through 14, 17, and 19 through 21 and 23 through 25 --
        // and a hole is not visible from inside: `from_bits` answered `None`
        // for each, which is also what it answers for a bit the engine has
        // not defined.
        assert_eq!(SemanticsAction::ALL.len(), 26);
        for (position, action) in SemanticsAction::ALL.iter().enumerate() {
            assert_eq!(
                *action as i32,
                1 << position,
                "{action:?} should be bit {position}"
            );
            assert_eq!(SemanticsAction::from_bits(1 << position), Some(*action));
        }
        // And bit 26 is past the end, which is where `None` starts meaning
        // what it says.
        assert_eq!(SemanticsAction::from_bits(1 << 26), None);
    }

    #[test]
    fn a_text_field_now_has_verbs_as_well_as_a_name() {
        // The shape of what was missing. `SemanticsFlags` already had
        // `is_text_field`, `is_obscured` and `is_read_only`, so a reader
        // could be told it had found a text field -- and every action that
        // works one was absent. Nine of the thirteen were these.
        let editing: Vec<SemanticsAction> = SemanticsAction::ALL
            .into_iter()
            .filter(|action| action.edits_text())
            .collect();
        assert_eq!(editing.len(), 9);
        assert!(editing.contains(&SemanticsAction::SetText));
        assert!(editing.contains(&SemanticsAction::MoveCursorForwardByWord));
        assert!(editing.contains(&SemanticsAction::Paste));

        // Tapping and scrolling are not editing, which is what makes the
        // predicate say something.
        assert!(!SemanticsAction::Tap.edits_text());
        assert!(!SemanticsAction::ScrollUp.edits_text());
        assert!(!SemanticsAction::Focus.edits_text());
    }

    #[test]
    fn moving_by_a_word_is_a_different_bit_from_moving_by_a_character() {
        // Four separate actions, and they are ten bits apart rather than
        // adjacent -- the by-word pair was added later, which is why upstream
        // could not put them beside their by-character partners. A port
        // guessing at the numbering would have paired them.
        use SemanticsAction::*;
        assert_eq!(MoveCursorForwardByCharacter as i32, 1 << 9);
        assert_eq!(MoveCursorBackwardByCharacter as i32, 1 << 10);
        assert_eq!(MoveCursorForwardByWord as i32, 1 << 19);
        assert_eq!(MoveCursorBackwardByWord as i32, 1 << 20);
        assert_ne!(
            MoveCursorForwardByWord as i32,
            (MoveCursorForwardByCharacter as i32) << 1
        );
    }

    #[test]
    fn scrolling_to_an_offset_is_not_a_fifth_direction() {
        // The four directions are a nudge -- move by about a screenful -- and
        // this one carries a destination. A reader dragging a scrollbar sends
        // it; one pressing a page key sends `ScrollDown`.
        assert_ne!(
            SemanticsAction::ScrollToOffset as i32,
            SemanticsAction::ScrollDown as i32
        );
        // And it is not in the vertical-scroll pair, which is the engine's
        // one bundling of these bits.
        assert_eq!(
            SemanticsAction::VERTICAL_SCROLL,
            SemanticsAction::ScrollUp as i32 | SemanticsAction::ScrollDown as i32
        );
        assert_eq!(
            SemanticsAction::VERTICAL_SCROLL & SemanticsAction::ScrollToOffset as i32,
            0
        );
        // A node that scrolls vertically offers both directions: offering one
        // alone would be a list you can go down and not back up.
        assert_ne!(
            SemanticsAction::VERTICAL_SCROLL,
            SemanticsAction::ScrollDown as i32
        );
    }

    #[test]
    fn the_custom_action_bit_does_not_say_which_custom_action() {
        // The only bit that does not name what it does: it says "one of the
        // application's own", and which one arrives in a separate integer. A
        // bridge treating it like the others has thrown away the only part
        // that carried the meaning.
        assert_eq!(SemanticsAction::CustomAction as i32, 1 << 17);
        assert_eq!(
            SemanticsAction::from_bits(1 << 17),
            Some(SemanticsAction::CustomAction)
        );
    }

    /// Lays out a tree, paints it, and returns what it says about itself.
    ///
    /// The paint is here because a real frame paints -- and because a walk that
    /// still worked when the drawing had been skipped is the whole point.
    fn describe_tree(widget: AnyWidget, size: Size) -> Vec<SemanticsNode> {
        describe_tree_keeping_root(widget, size).0
    }

    /// The same, handing back the render tree as well, for the tests that ask
    /// it something after the frame -- an action arrives long after the frame
    /// that drew the thing it names.
    fn describe_tree_keeping_root(
        widget: AnyWidget,
        size: Size,
    ) -> (Vec<SemanticsNode>, crate::render::BoxedRender) {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a tree was mounted");
        // Loose, so a child that asked for a size keeps it: a tight box
        // would stretch the very thing whose rectangle is under test.
        root.layout(BoxConstraints::loose(size.width, size.height));
        let mut layers = crate::engine::LayerTree::new(size.width as i32, size.height as i32);
        {
            let mut context = PaintContext::new(&mut layers, size);
            root.paint(&mut context, Offset::ZERO);
        }
        flush(size, &root);
        (tree_or_fail(), root)
    }

    #[test]
    fn the_walk_reports_what_a_paragraph_says_and_not_what_it_paints() {
        // A mutation making `describe_semantics` read the painted string
        // survived: the tests for the two strings checked `spoken()` directly
        // and never ran the walk. Checked here, where it runs.
        set_enabled(true);
        let nodes = describe_tree(
            leaf(|| {
                crate::render::RenderParagraph::rich_spans(vec![
                    crate::widgets::TextSpan::new("Costs ", crate::engine::TextStyle::default()),
                    crate::widgets::TextSpan::new("$$", crate::engine::TextStyle::default())
                        .spoken_as("Double dollars"),
                ])
            }),
            Size::new(400.0, 100.0),
        );
        set_enabled(false);

        let labels: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .collect();
        assert!(
            labels.iter().any(|label| *label == "Costs Double dollars"),
            "a reader hears the words, not the glyphs: {labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label.contains("$$")),
            "and never hears the glyphs: {labels:?}"
        );
    }

    #[test]
    fn the_walk_does_not_invent_an_index_in_parent() {
        // It cannot know one: by the time a node reaches the walk its dropped
        // siblings are gone, and their positions with them. Upstream sets
        // `indexInParent` in the render layer -- `RenderIndexedSemantics` and
        // `RenderTable` put it on the config -- where the full list is still
        // there to count.
        //
        // The walk now *carries* an index that a render object declared (see
        // [`crate::render::RenderIndexedSemanticsBox`] and
        // `the_last_of_five_is_still_the_fifth_when_two_were_dropped`), which
        // makes this test the other half of that rule rather than a record of
        // a gap: nobody in this tree declared one, so nobody gets one. Carrying
        // and inventing are the two things it is easy to confuse.
        //
        // Checked on nodes the collector produced, not on one built by hand:
        // a mutation making the walk fill in `Some(0)` survived a test that
        // constructed its own node, because that test never ran the walk.
        set_enabled(true);
        let nodes = describe_tree(
            single(
                semantics(
                    7,
                    SemanticsProperties::button("Increment"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ),
                |child| RenderPadding::new(EdgeInsets::all(10.0), child),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);
        assert!(nodes.len() >= 2, "the view's node and the annotation in it");
        for node in &nodes {
            assert_eq!(
                node.index_in_parent, None,
                "node {} was given an index the walk cannot know",
                node.id
            );
        }
    }

    /// What the platform is holding, which had better be something.
    fn tree_or_fail() -> Vec<SemanticsNode> {
        let nodes = tree();
        assert!(
            !nodes.is_empty(),
            "semantics are on but nothing was collected"
        );
        nodes
    }

    #[test]
    fn an_excluded_subtree_says_nothing_and_still_draws() {
        // `ExcludeSemantics` had no render object until tick 347: the widget
        // was a bare struct with a flag, and `render_semantics.rs` held a
        // model nothing built. So a subtree "excluded" from a screen reader
        // was read out in full.
        set_enabled(true);
        let laid = |widget| {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("mounted");
            crate::render::RenderBox::layout(
                &mut root,
                crate::render::BoxConstraints::loose(200.0, 100.0),
            );
            root
        };

        let plain = laid(leaf(|| crate::widgets::Text::new("read me")));
        crate::semantics::mark_needs_update();
        let heard = flush(Size::new(200.0, 100.0), &plain).expect("somebody asked");
        assert!(
            heard
                .iter()
                .any(|node| node.properties.label.contains("read me")),
            "the text speaks for itself when nothing hides it"
        );

        let hidden = laid(leaf(|| {
            crate::render::RenderExcludeSemanticsBox::new(
                true,
                crate::widgets::Text::new("read me"),
            )
        }));
        crate::semantics::mark_needs_update();
        let silence = flush(Size::new(200.0, 100.0), &hidden);
        assert!(
            silence
                .iter()
                .flatten()
                .all(|node| !node.properties.label.contains("read me")),
            "the walk never asked the text what it would have said"
        );

        // And `excluding: false` is the same widget doing nothing, which is
        // what makes the flag rather than the wrapper the thing that hides.
        let showing = laid(leaf(|| {
            crate::render::RenderExcludeSemanticsBox::new(
                false,
                crate::widgets::Text::new("read me"),
            )
        }));
        crate::semantics::mark_needs_update();
        let audible = flush(Size::new(200.0, 100.0), &showing).expect("somebody asked");
        assert!(
            audible
                .iter()
                .any(|node| node.properties.label.contains("read me")),
            "not excluding hides nothing"
        );

        // The subtree is hidden from the reader and from nobody else: it
        // still lays out, which is the difference between this and not
        // building the child at all.
        assert_eq!(
            crate::render::RenderBox::size(&hidden),
            crate::render::RenderBox::size(&plain),
            "excluded, not removed"
        );

        // And the ordinary walk still finds the child. Only the semantics
        // walk turns back, which is what makes this different from a widget
        // that does not build its child: anything walking the tree for
        // layout, paint or debugging still sees everything.
        let excluded = crate::render::RenderExcludeSemanticsBox::new(
            true,
            crate::widgets::Text::new("read me"),
        );
        let mut ordinary = 0;
        crate::render::RenderBox::visit_children(&excluded, &mut |_, _| ordinary += 1);
        let mut for_semantics = 0;
        crate::render::RenderBox::visit_children_for_semantics(&excluded, &mut |_, _| {
            for_semantics += 1
        });
        assert_eq!(ordinary, 1, "the child is still there");
        assert_eq!(for_semantics, 0, "and only the reader is turned away");
        set_enabled(false);
    }

    #[test]
    fn a_dialog_takes_the_page_under_it_out_of_the_reading() {
        // `BlockSemantics` is the other half of upstream's interesting pair,
        // and it runs the opposite way from exclusion: it says nothing about
        // its own subtree and takes away the siblings **painted before it**.
        // Until tick 348 the widget had no render object, so a modal left the
        // page behind it fully readable.
        set_enabled(true);
        let laid = |widget| {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("mounted");
            crate::render::RenderBox::layout(
                &mut root,
                crate::render::BoxConstraints::loose(200.0, 100.0),
            );
            root
        };
        let labels = |root: &crate::render::BoxedRender| {
            crate::semantics::mark_needs_update();
            flush(Size::new(200.0, 100.0), root)
                .unwrap_or_default()
                .iter()
                .map(|node| node.properties.label.clone())
                .filter(|label| !label.is_empty())
                .collect::<Vec<_>>()
        };

        // A page, then a dialog painted over it.
        let page_then_dialog = |blocking| {
            leaf(move || {
                crate::widgets::Column::new()
                    .push(crate::widgets::Text::new("the page"))
                    .push(crate::render::RenderBlockSemanticsBox::new(
                        blocking,
                        crate::widgets::Text::new("the dialog"),
                    ))
            })
        };

        let open = laid(page_then_dialog(true));
        let heard = labels(&open);
        assert!(
            heard.iter().any(|label| label.contains("the dialog")),
            "the dialog speaks for itself: {heard:?}"
        );
        assert!(
            !heard.iter().any(|label| label.contains("the page")),
            "and the page under it is gone: {heard:?}"
        );

        // The same tree with the block off is the control: both are read.
        let shut = laid(page_then_dialog(false));
        let both = labels(&shut);
        assert!(both.iter().any(|label| label.contains("the dialog")));
        assert!(
            both.iter().any(|label| label.contains("the page")),
            "nothing blocking, nothing hidden: {both:?}"
        );
    }

    #[test]
    fn a_merged_button_is_one_stop_that_says_both_its_words() {
        // The third shape in upstream's family. Exclusion does not ask;
        // blocking takes back what was asked; merging asks and **keeps** the
        // words, in one node instead of several -- an icon and a label that
        // would otherwise be two stops for a reader become one thing saying
        // both.
        set_enabled(true);
        let laid = |widget| {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("mounted");
            crate::render::RenderBox::layout(
                &mut root,
                crate::render::BoxConstraints::loose(200.0, 100.0),
            );
            root
        };
        let spoken = |root: &crate::render::BoxedRender| {
            crate::semantics::mark_needs_update();
            flush(Size::new(200.0, 100.0), root)
                .unwrap_or_default()
                .iter()
                .map(|node| node.properties.label.clone())
                .filter(|label| !label.is_empty())
                .collect::<Vec<_>>()
        };

        let apart = laid(leaf(|| {
            crate::widgets::Column::new()
                .push(crate::widgets::Text::new("Save"))
                .push(crate::widgets::Text::new("Ctrl S"))
        }));
        assert_eq!(
            spoken(&apart),
            vec!["Save".to_string(), "Ctrl S".to_string()],
            "two stops when nothing merges them"
        );

        let together = laid(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(
                crate::widgets::Column::new()
                    .push(crate::widgets::Text::new("Save"))
                    .push(crate::widgets::Text::new("Ctrl S")),
            )
        }));
        assert_eq!(
            spoken(&together),
            vec!["Save\nCtrl S".to_string()],
            "one stop, both words, joined the way `absorb` joins them"
        );
        set_enabled(false);
    }

    #[test]
    fn merging_keeps_the_order_the_reader_would_have_heard() {
        // Paint order is reading order, and folding must not reshuffle it:
        // "Save / Ctrl S" and "Ctrl S / Save" are the same two words and a
        // different sentence.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(
                crate::widgets::Column::new()
                    .push(crate::widgets::Text::new("first"))
                    .push(crate::widgets::Text::new("second"))
                    .push(crate::widgets::Text::new("third")),
            )
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
        let labels: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(labels, vec!["first\nsecond\nthird"]);
        set_enabled(false);
    }

    /// Lays a tree out, flushes semantics, and returns the nodes.
    fn spoken_nodes(widget: crate::framework::AnyWidget) -> Vec<SemanticsNode> {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        flush(Size::new(200.0, 200.0), &root).expect("somebody asked")
    }

    #[test]
    fn a_stop_nobody_can_point_at_does_not_cross_to_the_platform() {
        // Upstream's `shouldDrop`, which is `isInvisible`, which is an empty
        // rect. The words are not the whole of a semantics node: a reader
        // focuses it, the platform draws a highlight around it, and a touch
        // explorer finds it by where it is. A node with no rect has none of
        // that, so upstream drops it and everything that came with it.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .push(RenderSemantics::new(
                    901,
                    SemanticsProperties::label("nowhere"),
                    crate::widgets::SizedBox::new(0.0, 0.0),
                ))
                .push(crate::widgets::Text::new("somewhere"))
        }));
        let heard: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(
            heard,
            vec!["somewhere"],
            "the sized one is read and the sizeless one is gone"
        );
        assert!(
            !nodes.iter().any(|node| node.id == 901),
            "and it is not in the tree under any other name"
        );
        // The parent must not be left pointing at a node that is not there.
        let ids: Vec<i32> = nodes.iter().map(|node| node.id).collect();
        for node in &nodes {
            for child in &node.children {
                assert!(ids.contains(child), "dangling child {child} on {}", node.id);
            }
        }
        set_enabled(false);
    }

    #[test]
    fn the_same_empty_rect_is_dropped_whether_or_not_a_clip_made_it_empty() {
        // The bug the drop fixes was not "empty rects are kept" -- it was that
        // an empty rect inside a clip was dropped and the identical empty rect
        // outside one was shipped. One rule, one answer, however the rect got
        // that way.
        set_enabled(true);
        fn sizeless() -> RenderSemantics {
            RenderSemantics::new(
                902,
                SemanticsProperties::label("nowhere"),
                crate::widgets::SizedBox::new(0.0, 0.0),
            )
        }
        // The visible sibling differs between the two trees on purpose. With
        // the sizeless node dropped they would otherwise flush to identical
        // node sets, and the second `flush` would answer `None` for "nothing
        // changed since the last walk" -- gate three -- rather than for
        // anything this test is asking about.
        let bare = spoken_nodes(leaf(|| {
            crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .push(sizeless())
                .push(crate::widgets::Text::new("bare"))
        }));
        let clipped = spoken_nodes(leaf(|| {
            crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .push(crate::render::RenderClipRect::new(sizeless()))
                .push(crate::widgets::Text::new("clipped"))
        }));
        let said =
            |nodes: &[SemanticsNode]| nodes.iter().any(|node| node.properties.label == "nowhere");
        assert!(
            bare.iter().any(|node| node.properties.label == "bare"),
            "the control is there, so the walk really ran"
        );
        assert!(
            clipped
                .iter()
                .any(|node| node.properties.label == "clipped")
        );
        assert!(!said(&bare), "unclipped and empty: dropped");
        assert!(!said(&clipped), "clipped and empty: dropped, as before");
        set_enabled(false);
    }

    /// A viewport onto content of `content`, scrolled to `offset`, and what
    /// the walk says about it. The viewport fills the 200x200 the harness lays
    /// out into, so the room to scroll is `content` less 200 along the axis.
    fn scroll_node(axis: crate::render::Axis, content: Size, offset: f32) -> SemanticsNode {
        let nodes = spoken_nodes(leaf(move || {
            crate::render::RenderViewport::new(
                axis,
                crate::widgets::SizedBox::new(content.width, content.height),
            )
            .with_offset(offset)
        }));
        nodes
            .iter()
            .find(|node| !node.properties.scroll_position.is_nan())
            .cloned()
            .expect("a list said it was one")
    }

    #[test]
    fn a_list_tells_a_reader_that_it_is_a_list_and_where_in_it_they_are() {
        // What was missing was not the field but the speaker: all three scroll
        // numbers already crossed the FFI, and `SemanticsProperties::scrollable`
        // was built by one test and by nothing else. So every list in this port
        // reached a screen reader as a plain box -- no announcement that it
        // scrolls, no sense of position in it, no gesture offered.
        set_enabled(true);
        let node = scroll_node(
            crate::render::Axis::Vertical,
            Size::new(100.0, 500.0),
            120.0,
        );
        assert_eq!(node.properties.scroll_position, 120.0);
        assert_eq!(node.properties.scroll_extent_min, 0.0);
        assert_eq!(
            node.properties.scroll_extent_max, 300.0,
            "the content beyond the window: 500 of content in 200 of glass"
        );
        assert!(node.properties.has(SemanticsAction::ScrollUp));
        assert!(node.properties.has(SemanticsAction::ScrollDown));
        set_enabled(false);
    }

    /// A list of `count` rows of 50, each carrying its index, in a viewport
    /// scrolled to `offset` and clipped to the 200 the harness lays out into.
    fn scrolled_rows(count: i32, offset: f32) -> SemanticsNode {
        let nodes = spoken_nodes(leaf(move || {
            let mut column =
                crate::widgets::Column::new().with_main_axis_size(crate::render::MainAxisSize::Min);
            for index in 0..count {
                column = column.push(crate::render::RenderIndexedSemanticsBox::new(
                    index as i64,
                    RenderSemantics::new(
                        700 + index,
                        SemanticsProperties::label(format!("row {index}")),
                        crate::widgets::SizedBox::new(100.0, 50.0),
                    ),
                ));
            }
            crate::render::RenderViewport::new(crate::render::Axis::Vertical, column)
                .with_offset(offset)
        }));
        nodes
            .iter()
            .find(|node| !node.properties.scroll_position.is_nan())
            .cloned()
            .expect("a list said it was one")
    }

    #[test]
    fn a_scrolled_list_says_which_row_is_showing() {
        // The end of the chain that runs back through three rounds: a box
        // declares an index (349's family, wired in 351), a node that is not on
        // the glass is dropped rather than shipped (350), and the list itself
        // finally speaks (352). Here they meet -- the list reports the index of
        // the first row that survived, which is upstream's
        // `firstVisibleIndex ??= child.indexInParent` over the children that
        // are not hidden.
        //
        // Eight rows of 50 in a 200 window, so 400 of content and 200 of room
        // to scroll. Scrolled by 0 the first showing row is 0; by 120 the first
        // two rows are wholly above the clip, so it is row 2 -- and *not* row
        // 0, which is what counting the survivors from zero would have said.
        // At the bottom of the list it is row 4, still counted in the list
        // rather than in what is left of it.
        set_enabled(true);
        assert_eq!(scrolled_rows(8, 0.0).properties.scroll_index, Some(0));
        assert_eq!(
            scrolled_rows(8, 120.0).properties.scroll_index,
            Some(2),
            "two rows above the clip, so the third is the one showing"
        );
        assert_eq!(
            scrolled_rows(8, 200.0).properties.scroll_index,
            Some(4),
            "scrolled to the end: rows 4 to 7 are the window"
        );
        set_enabled(false);
    }

    #[test]
    fn a_list_counts_only_the_rows_inside_it() {
        // The search starts below the list's own node and not at the top of
        // the tree. The walk pushes nodes in the order it opens them, so
        // everything painted *before* the list is already sitting in the same
        // array -- and an indexed thing among them would be read as the list's
        // first showing row, which is a row number belonging to something that
        // is not in the list.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                // Painted first, indexed, and nothing to do with the list.
                .push(crate::render::RenderIndexedSemanticsBox::new(
                    99,
                    crate::widgets::Text::new("a chip above the list"),
                ))
                .push(
                    crate::render::RenderViewport::new(
                        crate::render::Axis::Vertical,
                        crate::render::RenderIndexedSemanticsBox::new(
                            4,
                            RenderSemantics::new(
                                710,
                                SemanticsProperties::label("the row"),
                                crate::widgets::SizedBox::new(100.0, 400.0),
                            ),
                        ),
                    )
                    .with_offset(0.0),
                )
        }));
        let list = nodes
            .iter()
            .find(|node| !node.properties.scroll_position.is_nan())
            .expect("a list said it was one");
        assert_eq!(
            list.properties.scroll_index,
            Some(4),
            "the row inside it, not the chip painted before it"
        );
        set_enabled(false);
    }

    #[test]
    fn a_list_showing_a_different_row_is_something_new_to_say() {
        // Both gates that decide whether a frame reaches the platform compare
        // properties -- `RenderSemantics::update_from` and `flush`'s third gate
        // -- so a field left out of the comparison is a field whose changes are
        // silently swallowed. Two lists identical in every other way, scrolled
        // to the same pixel, showing rows numbered differently: a reader being
        // told "row 4" instead of "row 104" is not the same announcement.
        set_enabled(true);
        let laid = |base: i32| {
            let nodes = spoken_nodes(leaf(move || {
                let mut column = crate::widgets::Column::new()
                    .with_main_axis_size(crate::render::MainAxisSize::Min);
                for step in 0..8 {
                    column = column.push(crate::render::RenderIndexedSemanticsBox::new(
                        (base + step) as i64,
                        RenderSemantics::new(
                            800 + step,
                            // The same words in both, so the row number is the
                            // only difference the comparison can see.
                            SemanticsProperties::label("a row"),
                            crate::widgets::SizedBox::new(100.0, 50.0),
                        ),
                    ));
                }
                crate::render::RenderViewport::new(crate::render::Axis::Vertical, column)
                    .with_offset(120.0)
            }));
            nodes
                .iter()
                .find(|node| !node.properties.scroll_position.is_nan())
                .cloned()
                .expect("a list said it was one")
        };

        let here = laid(0);
        let further = laid(100);
        assert_eq!(here.properties.scroll_index, Some(2));
        assert_eq!(further.properties.scroll_index, Some(102));
        assert_eq!(
            here.properties.scroll_position, further.properties.scroll_position,
            "same pixel, so the floats cannot be what tells them apart"
        );
        assert_ne!(
            here.properties, further.properties,
            "a changed row number has to survive the comparison"
        );
        set_enabled(false);
    }

    #[test]
    fn the_engine_has_no_null_so_minus_one_is_the_one_we_send() {
        use crate::app::pack_scroll_count;
        // Zero is unavailable as a null: `scrollIndex` and `scrollChildren`
        // are plain `int32_t` on the engine's node and default to 0, and row 0
        // of a list is a real answer. Sending 0 for "not a list" would make
        // every plain box claim to be showing the first row of itself.
        assert_eq!(pack_scroll_count(Some(0)), 0, "row zero is an answer");
        assert_eq!(pack_scroll_count(Some(41)), 41);
        assert_eq!(pack_scroll_count(None), -1, "and this is the absence");
    }

    #[test]
    fn a_list_that_knows_how_long_it_is_says_so() {
        // The count is the declared half, and nothing in this port declares one
        // yet: `RenderViewport` shows one child of whatever size and honestly
        // passes `None` (see
        // `a_list_that_never_said_how_long_it_is_says_nothing_about_it`). So
        // the parameter is exercised here directly rather than through a walk
        // -- a lazy list that knows its length is what will pass a number, and
        // when one does this is the behaviour it will get.
        let counted = SemanticsProperties::scrollable(
            0.0,
            0.0,
            100.0,
            crate::render::AxisDirection::Down,
            Some(40),
        );
        assert_eq!(counted.scroll_child_count, Some(40));
        assert_eq!(
            counted.scroll_index, None,
            "and the other half is still the walk's to find"
        );
        let uncounted = SemanticsProperties::scrollable(
            0.0,
            0.0,
            100.0,
            crate::render::AxisDirection::Down,
            None,
        );
        assert_eq!(uncounted.scroll_child_count, None);
    }

    #[test]
    fn a_list_that_never_said_how_long_it_is_says_nothing_about_it() {
        // Two different questions with two different answers. The count is
        // *declared* by whoever built the list -- upstream's
        // `semanticChildCount`, which a plain scrolling box has no value for --
        // while the index is *discovered* by the walk. A viewport onto one
        // child of some size knows the second and not the first, and saying
        // "row 3 of 0" would be worse than saying nothing.
        set_enabled(true);
        let node = scrolled_rows(8, 120.0);
        assert_eq!(node.properties.scroll_index, Some(2), "discovered");
        assert_eq!(node.properties.scroll_child_count, None, "never declared");
        set_enabled(false);
    }

    #[test]
    fn a_box_that_does_not_scroll_is_not_given_a_row_number() {
        // The index is only looked for under a node that said it scrolls.
        // Otherwise every ancestor of an indexed box -- the view's own node
        // included -- would take the first index it found underneath it and
        // report itself as a list showing row 3.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::render::RenderIndexedSemanticsBox::new(
                3,
                crate::widgets::Text::new("not in a list"),
            )
        }));
        for node in &nodes {
            assert_eq!(
                node.properties.scroll_index, None,
                "node {} claimed to be a list",
                node.id
            );
        }
        set_enabled(false);
    }

    #[test]
    fn a_list_that_fits_is_a_list_but_not_a_scrollable_one() {
        // Upstream's two conditions on two lines: the extents are written
        // whenever the position `haveDimensions`, the action only when
        // `maxScrollExtent > minScrollExtent`. Offering a scroll gesture that
        // cannot move anything tells a reader the page has more on it.
        set_enabled(true);
        let node = scroll_node(crate::render::Axis::Vertical, Size::new(100.0, 60.0), 0.0);
        assert_eq!(node.properties.scroll_extent_max, 0.0, "nowhere to go");
        assert_eq!(
            node.properties.scroll_position, 0.0,
            "and it still says it is a list, sitting at the top of itself"
        );
        assert!(!node.properties.has(SemanticsAction::ScrollUp));
        assert!(!node.properties.has(SemanticsAction::ScrollDown));
        set_enabled(false);
    }

    #[test]
    fn a_sideways_list_offers_the_sideways_gestures() {
        // The axis picks the pair. A reader flicking up on a row of cards
        // would be reaching for a gesture the list never claimed to have.
        set_enabled(true);
        let node = scroll_node(
            crate::render::Axis::Horizontal,
            Size::new(500.0, 100.0),
            0.0,
        );
        assert_eq!(node.properties.scroll_extent_max, 300.0);
        // At the left edge, and the axis picks the pair: the gesture that
        // reveals what is further right is offered, the one that would go back
        // past the start is not.
        assert!(node.properties.has(SemanticsAction::ScrollLeft));
        assert!(!node.properties.has(SemanticsAction::ScrollRight));
        assert!(!node.properties.has(SemanticsAction::ScrollUp));
        assert!(!node.properties.has(SemanticsAction::ScrollDown));
        set_enabled(false);
    }

    /// The nodes a laid-out image produces, through the real walk. Built by a
    /// closure because `leaf` may call it more than once and a `RenderImage`
    /// is not `Clone`.
    fn image_nodes(
        build: impl Fn(crate::render::RenderImage) -> crate::render::RenderImage + 'static,
    ) -> Vec<SemanticsNode> {
        spoken_nodes(leaf(move || {
            let handle = std::rc::Rc::new(
                crate::painting::Image::from_pixels(&[0u8; 4], 1, 1).expect("the stub allocates"),
            );
            build(crate::widgets::ImageView::new(handle))
        }))
    }

    #[test]
    fn a_named_picture_tells_a_reader_it_is_a_picture() {
        // Nothing in this port raised `is_image` -- the bit has crossed the
        // FFI since it was written and no widget ever set it -- so a
        // photograph arrived as an unnamed box. Upstream's `Image` wraps
        // itself in `Semantics(image: true, label: semanticLabel ?? '')`.
        set_enabled(true);
        let nodes = image_nodes(|image| image.with_semantic_label("Sunset over the bay"));
        let picture = nodes
            .iter()
            .find(|node| node.properties.flags.is_image)
            .expect("a picture said it was one");
        assert_eq!(picture.properties.label, "Sunset over the bay");
        set_enabled(false);
    }

    #[test]
    fn an_unnamed_picture_makes_no_stop_of_its_own() {
        // Upstream marks `image: true` either way and uses
        // `container: semanticLabel != null` to decide whether it becomes a
        // stop. This walk turns every annotation into a node, so annotating an
        // unlabelled picture would add a stop with **no words** to every piece
        // of decoration -- louder than upstream rather than more faithful.
        set_enabled(true);
        let nodes = image_nodes(|image| image);
        assert!(
            !nodes.iter().any(|node| node.properties.flags.is_image),
            "no words, so no stop"
        );
        set_enabled(false);
    }

    #[test]
    fn a_picture_excluded_from_semantics_says_nothing_even_when_named() {
        // `excludeFromSemantics` outranks the label: it is the parameter for a
        // picture whose meaning is already carried by the text beside it,
        // where even the word "image" is one word too many.
        set_enabled(true);
        let nodes = image_nodes(|image| {
            image
                .with_semantic_label("Sunset over the bay")
                .with_exclude_from_semantics(true)
        });
        assert!(
            !nodes
                .iter()
                .any(|node| node.properties.label.contains("Sunset")),
            "excluded, so not read"
        );
        assert!(!nodes.iter().any(|node| node.properties.flags.is_image));
        set_enabled(false);
    }

    #[test]
    fn a_reader_at_the_top_of_a_list_is_not_offered_the_way_back_up() {
        // Upstream's `_updateSemanticActions`, whose own doc gives the reason:
        // "If the scroll view has been scrolled all the way to the top, the
        // action to scroll further up needs to be removed as the scroll view
        // cannot be scrolled in that direction anymore."
        //
        // This port used to offer both directions whenever there was any room
        // at all, so a reader at the top of a list was handed a gesture that
        // does nothing -- and at the bottom, another one.
        set_enabled(true);
        let content = Size::new(100.0, 500.0);

        let top = scroll_node(crate::render::Axis::Vertical, content, 0.0);
        assert!(
            top.properties.has(SemanticsAction::ScrollUp),
            "there is more list below, and the gesture for it is `ScrollUp`"
        );
        assert!(
            !top.properties.has(SemanticsAction::ScrollDown),
            "and nothing above to go back to"
        );

        let middle = scroll_node(crate::render::Axis::Vertical, content, 150.0);
        assert!(
            middle.properties.has(SemanticsAction::ScrollUp),
            "both ways"
        );
        assert!(middle.properties.has(SemanticsAction::ScrollDown));

        let bottom = scroll_node(crate::render::Axis::Vertical, content, 300.0);
        assert!(
            !bottom.properties.has(SemanticsAction::ScrollUp),
            "nothing further down"
        );
        assert!(bottom.properties.has(SemanticsAction::ScrollDown));
        set_enabled(false);
    }

    #[test]
    fn a_list_that_runs_the_other_way_swaps_the_two_gestures() {
        // A chat pinned to the bottom is `AxisDirection::Up`, and upstream
        // swaps the pair for it. Taking the *axis* rather than the direction
        // would give a reader at the start of such a list the one gesture that
        // does nothing -- which is the same bug as the one above, wearing a
        // different hat.
        use crate::render::AxisDirection;
        let at_start = SemanticsProperties::scroll_actions(AxisDirection::Up, 0.0, 0.0, 300.0);
        assert_eq!(
            at_start,
            SemanticsAction::ScrollDown as i32,
            "an upward list moves on with `ScrollDown`"
        );
        let downward = SemanticsProperties::scroll_actions(AxisDirection::Down, 0.0, 0.0, 300.0);
        assert_eq!(
            downward,
            SemanticsAction::ScrollUp as i32,
            "and a downward one with `ScrollUp`, which is the opposite gesture"
        );
    }

    #[test]
    fn a_list_with_nowhere_to_go_offers_nothing() {
        // The case round 352 got right and this rule has to keep: a list that
        // fits on screen still says it is a list, and offers no gesture. Here
        // it falls out of the same two comparisons rather than needing a
        // guard of its own -- with `min == max == pixels`, neither holds.
        use crate::render::AxisDirection;
        assert_eq!(
            SemanticsProperties::scroll_actions(AxisDirection::Down, 0.0, 0.0, 0.0),
            0
        );
    }

    #[test]
    fn the_last_of_five_is_still_the_fifth_when_two_were_dropped() {
        // Upstream's own example, run end to end: "a scrollable with five
        // children whose first two are not visible has three nodes, and the
        // last of them still has index 4". Counting the survivors would say
        // "item 3 of 3" and quietly tell the reader the list is shorter than
        // it is -- which is the whole reason the index is written down by
        // whoever built the list instead of worked out by the walk.
        //
        // This test could not have been written before nodes were dropped at
        // all; until then the survivors and the full list were the same thing,
        // which is the case a careless test picks.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            let mut stack = crate::render::RenderStack::new();
            for index in 0..5 {
                stack = stack.push_positioned(
                    crate::render::RenderIndexedSemanticsBox::new(
                        index,
                        RenderSemantics::new(
                            600 + index as i32,
                            SemanticsProperties::label(format!("row {index}")),
                            crate::widgets::SizedBox::new(40.0, 20.0),
                        ),
                    ),
                    crate::render::StackPosition {
                        left: Some(0.0),
                        // The first two sit above the clip and never arrive.
                        top: Some(index as f32 * 20.0 - 40.0),
                        ..Default::default()
                    },
                );
            }
            crate::render::RenderClipRect::new(stack)
        }));
        let rows: Vec<(i32, Option<i32>)> = nodes
            .iter()
            .filter(|node| node.properties.label.starts_with("row "))
            .map(|node| (node.id, node.index_in_parent))
            .collect();
        assert_eq!(
            rows,
            vec![(602, Some(2)), (603, Some(3)), (604, Some(4))],
            "three nodes, and the last of them is still the fifth"
        );
        set_enabled(false);
    }

    #[test]
    fn one_offer_labels_one_node_and_not_everything_under_it() {
        // The offer is taken, not copied. An item is usually more than one box
        // deep, and if the index stuck to every node inside it a reader would
        // be told that each part of row 3 is itself row 3 -- the position of
        // the item, repeated onto its contents.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::render::RenderIndexedSemanticsBox::new(
                3,
                crate::widgets::Column::new()
                    .with_main_axis_size(crate::render::MainAxisSize::Min)
                    .push(crate::widgets::Text::new("first inside"))
                    .push(crate::widgets::Text::new("second inside")),
            )
        }));
        let inside: Vec<(&str, Option<i32>)> = nodes
            .iter()
            .filter(|node| node.properties.label.ends_with("inside"))
            .map(|node| (node.properties.label.as_str(), node.index_in_parent))
            .collect();
        assert_eq!(
            inside,
            vec![("first inside", Some(3)), ("second inside", None)],
            "the offer was claimed once"
        );
        set_enabled(false);
    }

    #[test]
    fn an_inner_list_does_not_swallow_the_outer_list_s_offer() {
        // A cell inside a row: two indexed boxes, one inside the other. The
        // inner one's offer covers its own subtree and no more, so what the
        // outer one offered is still on the table afterwards. Clearing instead
        // of restoring would drop the outer index silently -- the reader would
        // simply never be told where the row sits.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::render::RenderIndexedSemanticsBox::new(
                7,
                crate::widgets::Column::new()
                    .with_main_axis_size(crate::render::MainAxisSize::Min)
                    .push(crate::render::RenderIndexedSemanticsBox::new(
                        2,
                        crate::widgets::Text::new("cell"),
                    ))
                    .push(crate::widgets::Text::new("after the cell")),
            )
        }));
        let seen: Vec<(&str, Option<i32>)> = nodes
            .iter()
            .filter(|node| !node.properties.label.is_empty())
            .map(|node| (node.properties.label.as_str(), node.index_in_parent))
            .collect();
        assert_eq!(
            seen,
            vec![("cell", Some(2)), ("after the cell", Some(7))],
            "the inner offer was spent inside, the outer one survived it"
        );
        set_enabled(false);
    }

    #[test]
    fn an_index_offered_to_a_subtree_that_was_dropped_is_not_taken_by_a_sibling() {
        // The offer is put back on every way out of the walk, including the two
        // that abandon a subtree. Leave it lying and the next node to open --
        // a *sibling*, nothing to do with the indexed box -- picks it up, and
        // a reader is told the wrong position for the wrong row.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .push(crate::render::RenderIndexedSemanticsBox::new(
                    9,
                    // Sizeless, so the walk drops it and never opens a node
                    // that could claim the 9.
                    RenderSemantics::new(
                        610,
                        SemanticsProperties::label("dropped"),
                        crate::widgets::SizedBox::new(0.0, 0.0),
                    ),
                ))
                .push(crate::widgets::Text::new("innocent"))
        }));
        let bystander = nodes
            .iter()
            .find(|node| node.properties.label == "innocent")
            .expect("the sibling is read");
        assert_eq!(
            bystander.index_in_parent, None,
            "a sibling took an index offered to a subtree that was thrown away"
        );
        set_enabled(false);
    }

    #[test]
    fn what_was_under_a_dropped_stop_leaves_with_it() {
        // Upstream drops a node from its parent's child list
        // (`children.removeWhere(shouldDrop)`), and a child list is the only
        // way a node's subtree is reachable -- so the descendants are not
        // orphaned onto the grandparent, they are gone. Getting this wrong
        // would be worse than keeping the empty node: a reader would meet the
        // inner stop with no idea what it belonged to.
        //
        // `RenderSizedOverflowBox` is the case in one box: it takes the size it
        // was asked for and lays its child against the constraints that came
        // in, so the annotated box around it has no rect while the box inside
        // it has a real one.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            RenderSemantics::new(
                903,
                SemanticsProperties::label("the empty one"),
                crate::render::RenderSizedOverflowBox::new(
                    Size::ZERO,
                    RenderSemantics::new(
                        904,
                        SemanticsProperties::label("the one inside"),
                        crate::widgets::SizedBox::new(40.0, 20.0),
                    ),
                ),
            )
        }));
        let outer = nodes.iter().find(|node| node.id == 903);
        assert!(outer.is_none(), "the sizeless stop is dropped");
        assert!(
            nodes.iter().all(|node| node.id != 904),
            "and the stop under it went with it rather than moving up a level"
        );
        set_enabled(false);
    }

    #[test]
    fn a_merged_stop_past_the_clip_is_not_read_either() {
        // The merging branch opens a node of its own, so it has to ask the same
        // question every other opener asks. It did not, for one round: a folded
        // button scrolled off the end of a list would still have been read out,
        // at coordinates off the glass.
        set_enabled(true);
        let clipped_at = |top: f32| {
            spoken_nodes(leaf(move || {
                crate::render::RenderClipRect::new(
                    crate::render::RenderStack::new().push_positioned(
                        crate::render::RenderMergeSemanticsBox::new(
                            crate::widgets::Column::new()
                                .with_main_axis_size(crate::render::MainAxisSize::Min)
                                .push(crate::widgets::Text::new("Save"))
                                .push(crate::widgets::Text::new("Ctrl S")),
                        ),
                        crate::render::StackPosition {
                            left: Some(10.0),
                            top: Some(top),
                            ..Default::default()
                        },
                    ),
                )
            }))
        };
        let heard = |nodes: &[SemanticsNode]| {
            nodes
                .iter()
                .any(|node| node.properties.label.contains("Save"))
        };
        assert!(heard(&clipped_at(10.0)), "inside the clip: read");

        // The label alone would not catch this. A folded descendant is dropped
        // by its *own* clip check on the way in, so the words go missing
        // whether or not the merging box asks anything -- what leaks is the
        // merging box's own node, sitting on the tree at coordinates off the
        // glass with nothing in it. So the assertion is about where the nodes
        // are, not about what they say.
        let past = clipped_at(400.0);
        assert!(!heard(&past), "wholly past the clip: not read");
        let stray: Vec<(i32, f32)> = past
            .iter()
            .filter(|node| node.top >= 200.0)
            .map(|node| (node.id, node.top))
            .collect();
        assert!(
            stray.is_empty(),
            "a node was reported off the glass: {stray:?}"
        );
        set_enabled(false);
    }

    #[test]
    fn the_merge_lets_go_of_everything_painted_after_it() {
        // The fold has an end, and the end is the box. A sibling painted after
        // the merging box is nobody's descendant, so it stays a stop of its
        // own; swallowing it would make "merge these two things" mean "merge
        // the rest of the screen", which is a different and much worse widget.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::widgets::Column::new()
                .push(crate::render::RenderMergeSemanticsBox::new(
                    crate::widgets::Column::new()
                        .with_main_axis_size(crate::render::MainAxisSize::Min)
                        .push(crate::widgets::Text::new("Save"))
                        .push(crate::widgets::Text::new("Ctrl S")),
                ))
                .push(crate::widgets::Text::new("after"))
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 200.0), &root).expect("somebody asked");
        let labels: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(labels, vec!["Save\nCtrl S", "after"]);
        set_enabled(false);
    }

    #[test]
    fn a_label_folded_into_a_merge_says_its_words_once() {
        // A `Label` is an annotation with words and a `Text` underneath saying
        // the same thing; ordinarily the text yields to the label above it.
        // Folded into a merging box the annotation becomes no node at all, so
        // for a while nothing was left for the text to yield to and a banner
        // said "You are offline" twice.
        //
        // The rule that fixes it is the one that was always meant: **words
        // speak for what is under them**, whether they become a node or are
        // folded into one.
        set_enabled(true);
        let nodes = spoken_nodes(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(RenderSemantics::new(
                720,
                SemanticsProperties::label("You are offline"),
                crate::widgets::Text::new("You are offline"),
            ))
        }));
        let heard: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(heard, vec!["You are offline"], "once, not twice");
        set_enabled(false);
    }

    #[test]
    fn a_folded_button_leaves_a_stop_that_can_still_be_pressed() {
        // The gap round 388 found and this round closed. The walk's fold
        // carried a descendant's label, then its tooltip, then its role, and
        // **never its flags** -- so a button folded into a row left a stop
        // that said the button's words and nothing about being pressable, and
        // a checkbox folded into one stopped saying whether it was ticked.
        // Upstream unions them in `SemanticsNode.updateWith`:
        // `flags = flags.merge(node._flags)`.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(RenderSemantics::new(
                7,
                SemanticsProperties {
                    flags: SemanticsFlags {
                        is_button: true,
                        checked: SemanticsCheckState::Checked,
                        ..SemanticsFlags::default()
                    },
                    ..SemanticsProperties::label("Save")
                },
                crate::widgets::Text::new("Save"),
            ))
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
        let folded = nodes
            .iter()
            .find(|node| node.properties.label == "Save")
            .expect("the folded stop");
        assert!(
            folded.properties.flags.is_button,
            "it stopped being a button"
        );
        assert_eq!(
            folded.properties.flags.checked,
            SemanticsCheckState::Checked
        );
        set_enabled(false);
    }

    #[test]
    fn a_fold_adds_to_what_it_already_claimed_rather_than_replacing_it() {
        // The merging node has flags of its own -- a banner's live region is
        // the everyday case -- and a descendant's have to be **added** to
        // them. Written as an assignment the two agree in every test where the
        // merging node starts empty, and then the day a live region folds a
        // button, the announcement stops being an announcement.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(RenderSemantics::new(
                8,
                SemanticsProperties {
                    flags: SemanticsFlags {
                        is_button: true,
                        ..SemanticsFlags::default()
                    },
                    ..SemanticsProperties::label("Undo")
                },
                crate::widgets::Text::new("Undo"),
            ))
            .with_properties(SemanticsProperties {
                flags: SemanticsFlags {
                    is_live_region: true,
                    ..SemanticsFlags::default()
                },
                ..SemanticsProperties::label("")
            })
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
        let folded = nodes
            .iter()
            .find(|node| node.properties.label == "Undo")
            .expect("the folded stop");
        assert!(
            folded.properties.flags.is_live_region,
            "the fold overwrote what the merging node already claimed"
        );
        assert!(folded.properties.flags.is_button, "and it kept the button");
        set_enabled(false);
    }

    #[test]
    fn a_fold_over_nothing_in_particular_claims_nothing() {
        // The other half: a merge over plain text picks up no flags, so
        // ordinary rows do not start announcing themselves as controls. This
        // is why the census came out **byte for byte identical** when the fold
        // landed -- everything it folds is text.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(
                crate::widgets::Column::new()
                    .push(crate::widgets::Text::new("Inbox"))
                    .push(crate::widgets::Text::new("12 unread")),
            )
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
        let folded = nodes
            .iter()
            .find(|node| node.properties.label.starts_with("Inbox"))
            .expect("the folded stop");
        assert_eq!(folded.properties.flags, SemanticsFlags::default());
        set_enabled(false);
    }

    #[test]
    fn a_merge_inside_a_merge_is_still_one_stop() {
        // The outer merge has already claimed everything below it, so the inner
        // one has nothing left to claim: it opens no node of its own (`open`
        // hands back `None` while merging), falls through to the ordinary path,
        // and its children keep folding into the outer node. One stop, not two
        // -- which is what a reader wants, and it falls out of the rule rather
        // than needing a case of its own.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(
                crate::widgets::Column::new()
                    .push(crate::widgets::Text::new("outer"))
                    .push(crate::render::RenderMergeSemanticsBox::new(
                        crate::widgets::Text::new("inner"),
                    )),
            )
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
        let labels: Vec<&str> = nodes
            .iter()
            .map(|node| node.properties.label.as_str())
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(labels, vec!["outer\ninner"]);
        set_enabled(false);
    }

    #[test]
    fn the_merged_node_stands_where_the_merging_box_stands() {
        // The fold has to produce a node a reader can *point at*, and the only
        // rect that covers all the folded pieces is the merging box's own. Take
        // it from one of the children and the highlight lands on half the
        // button. The control is the same tree unmerged: the union of the two
        // stops it produces is the rect the one merged stop has to have.
        set_enabled(true);
        // Shrink-wrapped on purpose: a `MainAxisSize::Max` column would stand
        // taller than its two rows, and then "the box's rect" and "the union of
        // the rows" would be two different answers and the assertion could not
        // tell which one the code gave.
        fn column() -> crate::render::RenderFlex {
            crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .push(crate::widgets::Text::new("top"))
                .push(crate::widgets::Text::new("bottom"))
        }
        let spans = |widget| {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("mounted");
            crate::render::RenderBox::layout(
                &mut root,
                crate::render::BoxConstraints::loose(200.0, 100.0),
            );
            crate::semantics::mark_needs_update();
            let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
            let spoken: Vec<(f32, f32)> = nodes
                .iter()
                .filter(|node| !node.properties.label.is_empty())
                .map(|node| (node.top, node.bottom))
                .collect();
            spoken
        };

        let apart = spans(leaf(column));
        assert_eq!(apart.len(), 2, "two stops unmerged: {apart:?}");
        let union = (
            apart.iter().map(|it| it.0).fold(f32::MAX, f32::min),
            apart.iter().map(|it| it.1).fold(f32::MIN, f32::max),
        );

        let together = spans(leaf(|| {
            crate::render::RenderMergeSemanticsBox::new(column())
        }));
        assert_eq!(together.len(), 1, "one stop merged: {together:?}");
        assert_eq!(
            together[0], union,
            "the folded node covers both rows, not one"
        );
        assert!(
            union.1 - union.0 > 0.0,
            "and the two rows are not the same row: {union:?}"
        );
        set_enabled(false);
    }

    #[test]
    fn blocking_takes_what_came_before_and_not_what_comes_after() {
        // Paint order is the whole rule. A sibling painted *after* the
        // blocker is still read -- otherwise this would be "hide everything
        // else", which is a different widget.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            crate::widgets::Column::new()
                .push(crate::widgets::Text::new("before"))
                .push(crate::render::RenderBlockSemanticsBox::new(
                    true,
                    crate::widgets::Text::new("blocker"),
                ))
                .push(crate::widgets::Text::new("after"))
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = flush(Size::new(200.0, 100.0), &root).expect("somebody asked");
        let heard: Vec<String> = nodes
            .iter()
            .map(|node| node.properties.label.clone())
            .filter(|label| !label.is_empty())
            .collect();

        assert!(!heard.iter().any(|l| l.contains("before")), "{heard:?}");
        assert!(heard.iter().any(|l| l.contains("blocker")), "{heard:?}");
        assert!(
            heard.iter().any(|l| l.contains("after")),
            "painted later, so untouched: {heard:?}"
        );

        // The tree the reader is handed has to be *whole*, not merely short of
        // the blocked label: the node the survivors hang from is still there,
        // and it lists exactly them. Dropping the nodes without mending the
        // child list leaves ids pointing at things that no longer exist, and
        // taking the parent as well leaves the survivors hanging from nothing
        // -- neither shows up in a list of labels.
        let root_node = nodes.first().expect("a root node survived the blocking");
        let ids: Vec<i32> = nodes.iter().skip(1).map(|node| node.id).collect();
        assert_eq!(
            root_node.children, ids,
            "every child id still names a node that is there"
        );
        set_enabled(false);
    }

    #[test]
    fn nothing_is_collected_until_something_asks() {
        set_enabled(false);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| crate::widgets::Text::new("unread")));
        let root = tree.build_render_tree().expect("mounted");
        let collected = flush(Size::new(200.0, 100.0), &root);
        assert!(
            collected.is_none(),
            "a tree nobody reads should not be built"
        );

        // And the same tree is built once somebody asks. Without this the
        // claim above is satisfied by a `flush` that never builds anything --
        // which would be a screen reader that hears silence from every
        // application.
        set_enabled(true);
        let heard = flush(Size::new(200.0, 100.0), &root);
        assert!(heard.is_some(), "somebody asked");
        set_enabled(false);
    }

    #[test]
    fn an_annotation_reports_where_its_child_ended_up() {
        set_enabled(true);
        let nodes = describe_tree(
            single(
                semantics(
                    7,
                    SemanticsProperties::button("Increment"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ),
                |child| RenderPadding::new(EdgeInsets::all(10.0), child),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        // The view's own node, and the annotation inside it.
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id, ROOT_ID);
        let node = &nodes[1];
        assert_eq!(node.id, 7);
        assert_eq!(node.properties.label, "Increment");
        assert!(node.properties.flags.is_button);
        assert!(node.properties.has(SemanticsAction::Tap));
        // In root coordinates, past the padding: what the platform asks for is
        // where on the glass this is, not where inside its parent.
        assert_eq!((node.left, node.top), (10.0, 10.0));
        assert_eq!((node.width(), node.height()), (80.0, 40.0));
    }

    #[test]
    fn a_node_under_a_boundary_still_says_where_it_is() {
        set_enabled(true);
        let nodes = describe_tree(
            single(
                crate::widgets::repaint_boundary(semantics(
                    7,
                    SemanticsProperties::button("Increment"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                )),
                |child| RenderPadding::new(EdgeInsets::all(10.0), child),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);
        let node = nodes
            .iter()
            .find(|n| n.id == 7)
            .expect("the button is read");
        assert_eq!(
            (node.left, node.top),
            (10.0, 10.0),
            "reported somewhere else"
        );
    }

    #[test]
    fn a_node_under_a_layer_still_says_where_it_is() {
        // A repaint boundary is not the only thing that puts the offset in a
        // layer and paints its child at the origin -- opacity and transform do
        // it too, because a layer carries its own position. Anything reading a
        // node's rectangle out of the paint walk read the position *inside* the
        // layer, and a partly faded subtree is an ordinary thing to have.
        use crate::render::{RenderOpacity, RenderTransform};

        set_enabled(true);
        for (what, wrap) in [("opacity", 0), ("transform", 1)] {
            let inner = semantics(
                8,
                SemanticsProperties::button("Increment"),
                leaf(|| SizedBox::new(80.0, 40.0)),
            );
            let wrapped = match wrap {
                0 => single(inner, |child| RenderOpacity::new(0.5, child)),
                _ => single(inner, |child| {
                    RenderTransform::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], child)
                }),
            };
            let nodes = describe_tree(
                single(wrapped, |child| {
                    RenderPadding::new(EdgeInsets::all(10.0), child)
                }),
                Size::new(200.0, 100.0),
            );
            let node = nodes
                .iter()
                .find(|n| n.id == 8)
                .expect("the button is read");
            assert_eq!((node.left, node.top), (10.0, 10.0), "under {what}");
        }
        set_enabled(false);
    }

    #[test]
    fn a_viewport_clips_its_rows_to_the_window() {
        // The walk used to report a scrolled-out row at its place in the
        // content -- the gallery's home had "Tooltips" at y=3651 under a window
        // a tenth that tall, and the bridges passed it on. Upstream cuts every
        // node's rect against the clips its ancestors describe
        // (`_SemanticsGeometry.computeChildGeometry`); this is that, with a
        // viewport contributing both of its rects.
        //
        // Four rows of 100 in a 200-tall window scrolled 150: row one is wholly
        // above the window, rows two and four straddle its edges, row three is
        // entirely inside.
        use crate::render::{Axis, MainAxisSize, RenderFlex, RenderViewport};

        set_enabled(true);
        let row = |id: i32, said: &str| {
            semantics_with_action(
                id,
                SemanticsProperties::label(said),
                leaf(|| SizedBox::new(200.0, 100.0)),
                |_| {},
            )
        };
        let (nodes, root) = describe_tree_keeping_root(
            single(
                many(
                    vec![
                        row(1, "first"),
                        row(2, "second"),
                        row(3, "third"),
                        row(4, "fourth"),
                    ],
                    |children| {
                        let mut column =
                            RenderFlex::column().with_main_axis_size(MainAxisSize::Min);
                        for child in children {
                            column = column.push(child);
                        }
                        column
                    },
                ),
                |column| RenderViewport::new(Axis::Vertical, column).with_offset(150.0),
            ),
            Size::new(200.0, 200.0),
        );
        set_enabled(false);

        // Wholly above the window: not in the tree at all. Upstream drops it
        // as `isInvisible`, and its subtree goes with it.
        assert!(
            nodes.iter().all(|node| node.id != 1),
            "a row off the window was reported"
        );

        let by_id = |id: i32| nodes.iter().find(|node| node.id == id).unwrap();
        // The second row is content 100..200, absolute -50..50: the window
        // keeps 0..50 of it.
        assert_eq!(
            (by_id(2).top, by_id(2).bottom),
            (0.0, 50.0),
            "cut to the part that shows"
        );
        // The third is content 200..300, absolute 50..150: inside entire.
        assert_eq!(
            (by_id(3).top, by_id(3).bottom),
            (50.0, 150.0),
            "a held row keeps its rect"
        );
        // The fourth straddles the far edge: 150..250 becomes 150..200.
        assert_eq!(
            (by_id(4).top, by_id(4).bottom),
            (150.0, 200.0),
            "cut at the far edge"
        );
        // Nothing reports outside the window, which is the whole complaint.
        for node in &nodes {
            assert!(
                node.left >= 0.0 && node.top >= 0.0 && node.right <= 200.0 && node.bottom <= 200.0,
                "{node:?} is outside the window"
            );
        }

        // And the dropping reaches actions: a row the reader was never told
        // about is not a row a reader can name, so nothing takes an action for
        // it either.
        assert!(
            !perform_action(&root, 1, SemanticsAction::Tap),
            "an off-window row acted"
        );
        assert!(perform_action(&root, 3, SemanticsAction::Tap));
    }

    #[test]
    fn a_carousel_card_beyond_the_window_is_not_reported() {
        // The other half of the same complaint: the gallery's carousel in a
        // window 690 wide reported a card at x=1318. Three cards of 690 in a
        // window of 690, scrolled 62, puts the third card's left edge exactly
        // there -- past the window and past the cache band, so it is gone
        // rather than clipped.
        use crate::render::{Axis, MainAxisSize, RenderFlex, RenderViewport};

        set_enabled(true);
        let nodes = describe_tree(
            single(
                many(
                    vec![
                        semantics(
                            1,
                            SemanticsProperties::label("Rally"),
                            leaf(|| SizedBox::new(690.0, 100.0)),
                        ),
                        semantics(
                            2,
                            SemanticsProperties::label("Shrine"),
                            leaf(|| SizedBox::new(690.0, 100.0)),
                        ),
                        semantics(
                            3,
                            SemanticsProperties::label("Fortnightly"),
                            leaf(|| SizedBox::new(690.0, 100.0)),
                        ),
                    ],
                    |children| {
                        let mut row = RenderFlex::row().with_main_axis_size(MainAxisSize::Min);
                        for child in children {
                            row = row.push(child);
                        }
                        row
                    },
                ),
                |row| RenderViewport::new(Axis::Horizontal, row).with_offset(62.0),
            ),
            Size::new(690.0, 100.0),
        );
        set_enabled(false);

        assert!(
            nodes.iter().all(|node| node.id != 3),
            "the card at x=1318 was reported"
        );
        let by_id = |id: i32| nodes.iter().find(|node| node.id == id).unwrap();
        // The first card has scrolled 62 off the leading edge, the second runs
        // off the trailing one, and neither reports past the window.
        assert_eq!(
            (by_id(1).left, by_id(1).right),
            (0.0, 628.0),
            "cut at the leading edge"
        );
        assert_eq!(
            (by_id(2).left, by_id(2).right),
            (628.0, 690.0),
            "cut at the trailing edge"
        );
    }

    #[test]
    fn a_node_overhanging_a_clip_is_held_to_it() {
        // A `ClipRect` paints through its own bounds, and a reader is told
        // about the part of a child that is inside them -- upstream's
        // `RenderClipRect.describeApproximatePaintClip`. The stack is the
        // window's size with the badge pinned 10 before its bottom edge, so 10
        // of the badge hang past it; the second badge is wholly outside.
        use crate::render::{RenderClipRect, RenderStack, StackPosition};

        set_enabled(true);
        let nodes = describe_tree(
            single(
                many(
                    vec![
                        semantics(
                            6,
                            SemanticsProperties::label("badge"),
                            leaf(|| SizedBox::new(40.0, 20.0)),
                        ),
                        semantics(
                            7,
                            SemanticsProperties::label("gone"),
                            leaf(|| SizedBox::new(40.0, 20.0)),
                        ),
                    ],
                    |children| {
                        let positions = [
                            StackPosition {
                                left: Some(10.0),
                                top: Some(90.0),
                                ..Default::default()
                            },
                            StackPosition {
                                left: Some(10.0),
                                top: Some(120.0),
                                ..Default::default()
                            },
                        ];
                        let mut stack = RenderStack::new();
                        for (child, position) in children.into_iter().zip(positions) {
                            stack = stack.push_positioned(child, position);
                        }
                        stack
                    },
                ),
                |stack| RenderClipRect::new(stack),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let badge = nodes
            .iter()
            .find(|node| node.id == 6)
            .expect("the badge is read");
        assert_eq!(
            (badge.left, badge.top, badge.right, badge.bottom),
            (10.0, 90.0, 50.0, 100.0),
            "held to the clip it is painted through"
        );
        // Wholly past the clip, and with no viewport above to give it a cache
        // band: nothing of it is on the glass, so it is not in the tree.
        assert!(
            nodes.iter().all(|node| node.id != 7),
            "a node past the clip was reported"
        );
    }

    #[test]
    fn a_reader_no_longer_costs_the_screen_its_retained_layers() {
        // What collecting on the paint walk used to cost. A boundary could not
        // hand back the layer it kept, because the subtree behind that layer
        // was where the semantics came from -- so opening a screen reader threw
        // away every retained layer on the screen, for as long as it stayed
        // open. The walk is its own now, so the layer and the reader are no
        // longer in each other's way.
        use crate::engine_test_stubs::{layer_calls, reset_layer_calls};
        use crate::widgets::repaint_boundary;

        set_enabled(true);
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(repaint_boundary(semantics(
            11,
            SemanticsProperties::button("Increment"),
            leaf(|| SizedBox::new(80.0, 40.0)),
        )));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));

        let frame = |root: &mut crate::render::BoxedRender| {
            reset_layer_calls();
            let mut layers = crate::engine::LayerTree::new(200, 100);
            {
                let mut context = PaintContext::new(&mut layers, size);
                root.paint(&mut context, Offset::ZERO);
            }
            flush(size, root);
            (layer_calls(), tree_or_fail())
        };

        let (first, said) = frame(&mut root);
        assert_eq!(
            (first.retainable, first.retained),
            (1, 0),
            "the first frame draws"
        );
        assert!(said.iter().any(|node| node.id == 11), "and is read");

        let (quiet, said) = frame(&mut root);
        assert_eq!(
            (quiet.retainable, quiet.retained),
            (0, 1),
            "drawn again for a reader"
        );
        assert!(
            said.iter().any(|node| node.id == 11),
            "the node stopped being reported"
        );
        set_enabled(false);
    }

    #[test]
    fn nesting_follows_the_paint() {
        set_enabled(true);
        let nodes = describe_tree(
            semantics(
                1,
                SemanticsProperties::label("a list"),
                many(
                    vec![
                        semantics(
                            2,
                            SemanticsProperties::label("first"),
                            leaf(|| SizedBox::new(50.0, 20.0)),
                        ),
                        semantics(
                            3,
                            SemanticsProperties::label("second"),
                            leaf(|| SizedBox::new(50.0, 20.0)),
                        ),
                    ],
                    |children| {
                        let mut flex = RenderFlex::column();
                        for child in children {
                            flex = flex.push(child);
                        }
                        flex
                    },
                ),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].children, vec![1], "the view's node holds the tree");
        assert_eq!(nodes[1].id, 1);
        assert_eq!(
            nodes[1].children,
            vec![2, 3],
            "reading order is paint order"
        );
        assert!(nodes[2].children.is_empty());
        // The second row is below the first, which is the whole reason a
        // rectangle is worth carrying: a finger dragged down the glass meets
        // them in this order.
        assert!(nodes[3].top >= nodes[2].bottom);
    }

    #[test]
    fn an_action_reaches_the_widget_that_offered_it() {
        set_enabled(true);
        let taps = Rc::new(Cell::new(0));
        let counted = Rc::clone(&taps);
        let (_, root) = describe_tree_keeping_root(
            semantics_with_action(
                4,
                SemanticsProperties::button("Increment"),
                leaf(|| SizedBox::new(50.0, 20.0)),
                move |action| {
                    if action == SemanticsAction::Tap {
                        counted.set(counted.get() + 1);
                    }
                },
            ),
            Size::new(200.0, 100.0),
        );

        assert!(perform_action(&root, 4, SemanticsAction::Tap));
        assert_eq!(taps.get(), 1);
        assert!(
            !perform_action(&root, 99, SemanticsAction::Tap),
            "no such node"
        );
        set_enabled(false);
    }

    #[test]
    fn a_toggle_says_which_way_it_is() {
        let off = SemanticsProperties::toggle("Notifications", false);
        let on = SemanticsProperties::toggle("Notifications", true);
        // Having the state is what makes "off" sayable at all; a node without
        // it is just a label, and a reader is told nothing.
        //
        // This asserted `checked` and passed, because `toggle` set the
        // checked flag -- so a switch announced as a checkbox and the test
        // said it was right to. A reader says "on" for one and "checked" for
        // the other, and upstream's `Switch` raises `toggled` while its
        // `Checkbox` raises `checked`.
        assert_eq!(off.flags.toggled, SemanticsTristate::False);
        assert_eq!(on.flags.toggled, SemanticsTristate::True);
        assert_eq!(
            on.flags.checked,
            SemanticsCheckState::None,
            "a switch is not a checkbox"
        );
        assert!(off.has(SemanticsAction::Tap));
    }

    #[test]
    fn a_scrollable_says_how_far_down_it_is() {
        let scroller = SemanticsProperties::scrollable(
            120.0,
            0.0,
            900.0,
            crate::render::AxisDirection::Down,
            None,
        );
        assert!(scroller.has(SemanticsAction::ScrollUp));
        assert!(scroller.has(SemanticsAction::ScrollDown));
        assert!(!scroller.has(SemanticsAction::ScrollLeft));
        assert_eq!(scroller.scroll_position, 120.0);
    }

    #[test]
    fn a_screen_of_ordinary_components_describes_itself() {
        // Nothing below asks for semantics. The point of wiring the built-in
        // components is that an application gets this without knowing it did.
        use crate::components::{Button, Label, Switch, stack_column};
        use crate::framework::component;

        set_enabled(true);
        let nodes = describe_tree(
            stack_column(
                vec![
                    component(Label::title("Settings")),
                    component(Label::new("Notifications are on")),
                    component(Switch::new(2, true)),
                    component(Button::new(3, "Save")),
                    component(Button::new(4, "Delete").with_enabled(false)),
                ],
                8.0,
            ),
            Size::new(300.0, 400.0),
        );
        set_enabled(false);

        let says = |text: &str| nodes.iter().find(|n| n.properties.label == text);

        let title = says("Settings").expect("the title is read");
        assert!(
            title.properties.flags.is_header,
            "a title is a heading to jump to"
        );

        assert!(says("Notifications are on").is_some(), "body text is read");

        let switch = nodes
            .iter()
            .find(|n| n.id == node_id_for(2))
            .expect("the switch is there");
        // A switch, so the *toggled* flag: this asserted `checked` and passed,
        // which is what "the switch announced as a checkbox" looked like from
        // inside the suite.
        assert!(switch.properties.flags.toggled.is_set());
        assert_eq!(
            switch.properties.flags.toggled,
            SemanticsTristate::True,
            "and it is on"
        );
        assert_eq!(
            switch.properties.flags.checked,
            SemanticsCheckState::None,
            "and it is not a checkbox"
        );

        let save = says("Save").expect("the button is read");
        assert_eq!(
            save.id,
            node_id_for(3),
            "its semantics id is its hit-test id"
        );
        assert!(save.properties.flags.is_button);
        assert!(save.properties.has(SemanticsAction::Tap));
        assert!(save.properties.flags.is_enabled);

        let delete = says("Delete").expect("a disabled button is still read");
        assert!(delete.properties.flags.has_enabled_state && !delete.properties.flags.is_enabled);
        assert!(
            !delete.properties.has(SemanticsAction::Tap),
            "and offers nothing a reader could do with it"
        );

        // Every node has somewhere to be, and the ones whose size does not
        // depend on shaping have real area. A rectangle is what makes touch
        // exploration possible at all -- a node with none is, to a finger
        // dragged across the glass, not there. (The text nodes measure zero
        // here because the engine every unit test shapes against reports zero
        // for every metric; on a device they are the size of their glyphs.)
        for node in &nodes {
            assert!(node.left.is_finite() && node.top.is_finite(), "{node:?}");
            assert!(node.width() >= 0.0 && node.height() >= 0.0, "{node:?}");
        }
        assert!(
            switch.width() > 0.0 && switch.height() > 0.0,
            "the switch is a target"
        );
        assert!(
            save.width() > 0.0 && save.height() > 0.0,
            "so is the button"
        );
    }

    #[test]
    fn tapping_a_button_through_semantics_does_what_the_finger_does() {
        use crate::components::Button;
        use crate::framework::component;
        use crate::gestures::PointerHandlers;

        set_enabled(true);
        let saves = Rc::new(Cell::new(0));
        let counted = Rc::clone(&saves);
        let (_, root) = describe_tree_keeping_root(
            component(Button::new(9, "Save").with_handlers(
                PointerHandlers::new().with_tap(move |_| counted.set(counted.get() + 1)),
            )),
            Size::new(300.0, 100.0),
        );

        assert!(perform_action(&root, node_id_for(9), SemanticsAction::Tap));
        assert_eq!(
            saves.get(),
            1,
            "the same closure a finger would have called"
        );
        set_enabled(false);
    }

    #[test]
    fn text_nobody_annotated_is_still_read() {
        // A raw `Text`, with nothing asking for accessibility anywhere. It is
        // the most common thing on a screen and the most important thing to
        // read; upstream `Text` describes itself for the same reason.
        set_enabled(true);
        let nodes = describe_tree(
            leaf(|| crate::widgets::Text::new("Rendered by Rust")),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].properties.label, "Rendered by Rust");
    }

    #[test]
    fn a_label_under_an_rtl_directionality_says_which_way_it_runs() {
        // Upstream's `Semantics` widget defaults the configuration's
        // textDirection to `Directionality.maybeOf(context)`, and the node
        // carries the result to the embedder -- a reader that is not told
        // which way "حفظ" runs reads it back to front. The direction is
        // taken where the annotation is built, inside the render walk that
        // pushes the ambient direction around the subtree.
        use crate::direction::{TextDirection, directionality};

        set_enabled(true);
        let nodes = describe_tree(
            directionality(
                TextDirection::Rtl,
                semantics(
                    7,
                    SemanticsProperties::button("حفظ"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let node = nodes
            .iter()
            .find(|n| n.id == 7)
            .expect("the button is read");
        assert_eq!(node.properties.text_direction, Some(TextDirection::Rtl));
        // The view's own node says nothing, so it carries no direction: only
        // words have one.
        assert_eq!(nodes[0].properties.text_direction, None);
    }

    #[test]
    fn a_label_without_a_directionality_runs_left_to_right() {
        // Left to right is what a tree with no `directionality` in it gets,
        // from the ambient direction's own fallback; a label still carries
        // it rather than nothing, because upstream's `SemanticsData` demands
        // a direction of everything it can read aloud.
        use crate::direction::TextDirection;

        set_enabled(true);
        let nodes = describe_tree(
            semantics(
                3,
                SemanticsProperties::label("plain"),
                leaf(|| SizedBox::new(50.0, 20.0)),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let node = nodes.iter().find(|n| n.id == 3).expect("the label is read");
        assert_eq!(node.properties.text_direction, Some(TextDirection::Ltr));
    }

    #[test]
    fn a_node_with_nothing_to_read_carries_no_direction() {
        // A node that offers an action but says nothing: upstream's assert
        // demands a textDirection of the read-aloud fields and none other,
        // so this one crosses as "unknown" rather than guessing.
        set_enabled(true);
        let nodes = describe_tree(
            semantics_with_action(
                5,
                SemanticsProperties::default(),
                leaf(|| SizedBox::new(50.0, 20.0)),
                |_| {},
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let node = nodes.iter().find(|n| n.id == 5).expect("the node is read");
        assert_eq!(node.properties.label, "");
        assert_eq!(
            node.properties.text_direction, None,
            "no words, no direction"
        );
    }

    #[test]
    fn text_takes_the_direction_of_its_context() {
        // A paragraph's annotation takes the ambient direction with the
        // words, standing in for the paragraph's own until the render side
        // captures one; `with_text_direction` is where that lands, and a
        // render object that already knows can say so through it today.
        use crate::direction::{TextDirection, with_direction};

        let plain = SemanticsAnnotation::text(1, "plain");
        assert_eq!(plain.properties.text_direction, Some(TextDirection::Ltr));

        let rtl = with_direction(TextDirection::Rtl, || SemanticsAnnotation::text(2, "مرحبا"));
        assert_eq!(rtl.properties.text_direction, Some(TextDirection::Rtl));

        let known = SemanticsAnnotation::text(3, "mixed").with_text_direction(TextDirection::Rtl);
        assert_eq!(known.properties.text_direction, Some(TextDirection::Rtl));
    }

    #[test]
    fn a_changed_direction_is_news_to_a_reader() {
        // A direction participates in the sameness the walk and the update
        // compare -- upstream's `_isDifferentFromCurrentSemanticAnnotation`
        // compares `textDirection` beside the label -- so a subtree whose
        // directionality flipped is re-sent rather than read in the old one.
        use crate::direction::TextDirection;

        let ltr = SemanticsProperties {
            text_direction: Some(TextDirection::Ltr),
            ..SemanticsProperties::label("same words")
        };
        let rtl = SemanticsProperties {
            text_direction: Some(TextDirection::Rtl),
            ..SemanticsProperties::label("same words")
        };
        assert_eq!(ltr, ltr.clone());
        assert_ne!(ltr, rtl, "the same words run differently");
    }

    #[test]
    fn a_label_speaks_for_the_text_inside_it() {
        // The button says "Save" and its child text says "Save". Read as two
        // nodes a reader hears it twice, which is worse than hearing it once
        // in the wrong voice. Upstream's `excludeSemantics` is the same rule.
        use crate::components::Button;
        use crate::framework::component;

        set_enabled(true);
        let nodes = describe_tree(component(Button::new(5, "Save")), Size::new(200.0, 100.0));
        set_enabled(false);

        let said: Vec<&str> = nodes
            .iter()
            .map(|n| n.properties.label.as_str())
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(said, vec!["Save"], "heard once, from the button");
    }

    #[test]
    fn a_paragraph_keeps_its_node_across_a_frame() {
        // The identity a screen reader keys on. It is stable because the
        // render object is: see the persistence work in section eighteen.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| crate::widgets::Text::new("unchanging")));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 100.0));

        // Forced, because the point under test is what the walk finds, and a
        // rebuild that changed nothing is exactly the frame the walk is now
        // allowed to skip.
        let describe_once = |root: &mut crate::render::BoxedRender| {
            let mut layers = crate::engine::LayerTree::new(200, 100);
            {
                let mut context = PaintContext::new(&mut layers, Size::new(200.0, 100.0));
                root.paint(&mut context, Offset::ZERO);
            }
            mark_needs_update();
            flush(Size::new(200.0, 100.0), root);
            tree_or_fail()
        };

        let first = describe_once(&mut root);
        tree.rebuild_dirty();
        let mut again = tree.build_render_tree().expect("still mounted");
        let second = describe_once(&mut again);
        set_enabled(false);

        assert_eq!(first[1].id, second[1].id, "the same text became a new node");
    }

    #[test]
    fn the_view_is_one_node_with_everything_under_it() {
        // A reader is handed a tree, not a heap. Upstream's `RenderView` node
        // is what makes that true there, and it is what makes reading *order*
        // survive the crossing here: the platform is handed a map, so the only
        // place a sequence can live is a parent's list of children.
        use crate::components::Label;
        use crate::components::stack_column;
        use crate::framework::component;

        set_enabled(true);
        let nodes = describe_tree(
            stack_column(
                vec![
                    component(Label::new("first")),
                    component(Label::new("second")),
                    component(Label::new("third")),
                ],
                4.0,
            ),
            Size::new(300.0, 400.0),
        );
        set_enabled(false);

        assert_eq!(nodes[0].id, ROOT_ID);
        assert_eq!((nodes[0].width(), nodes[0].height()), (300.0, 400.0));
        // Everything else is somebody's child, so nothing is loose.
        let claimed: Vec<i32> = nodes.iter().flat_map(|n| n.children.clone()).collect();
        for node in &nodes[1..] {
            assert!(claimed.contains(&node.id), "{node:?} hangs from nothing");
        }
        let said: Vec<&str> = nodes[0]
            .children
            .iter()
            .map(|id| {
                nodes
                    .iter()
                    .find(|n| n.id == *id)
                    .map(|n| n.properties.label.as_str())
                    .unwrap_or("")
            })
            .collect();
        assert_eq!(said, vec!["first", "second", "third"], "top to bottom");
    }

    #[test]
    fn every_action_bit_round_trips() {
        for action in [
            SemanticsAction::Tap,
            SemanticsAction::LongPress,
            SemanticsAction::ScrollUp,
            SemanticsAction::Increase,
            SemanticsAction::Dismiss,
            SemanticsAction::Focus,
        ] {
            assert_eq!(SemanticsAction::from_bits(action as i32), Some(action));
        }
        assert_eq!(
            SemanticsAction::from_bits(1 << 30),
            None,
            "a bit we have no name for"
        );
    }

    #[test]
    fn a_new_label_under_a_boundary_is_still_read_out() {
        // Written when semantics rode on the paint walk, where a repaint
        // boundary handing back a kept layer meant a subtree that said nothing
        // about itself -- a reader would lose every row of a list after the
        // first frame. The walk is its own now, so that is no longer how this
        // passes; it passes because `RenderSemantics::update_from` marks when
        // its label changed, and because the two frames in the middle -- which
        // change nothing -- are allowed to skip the walk entirely and leave
        // last frame's answer standing, which is still the right answer.
        //
        // Three things at once, then: a label that changed is read, a label
        // that did not is not re-derived, and neither depends on the drawing.
        use crate::framework::{BuildContext, StateHandle, StatefulComponent, stateful};
        use crate::widgets::repaint_boundary;
        use std::cell::RefCell;
        use std::rc::Rc;

        #[derive(Default)]
        struct Which {
            second: bool,
        }

        struct Label {
            sink: Rc<RefCell<Option<StateHandle<Which>>>>,
        }

        impl StatefulComponent for Label {
            type State = Which;

            fn build(
                &self,
                state: &Which,
                handle: StateHandle<Which>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                *self.sink.borrow_mut() = Some(handle);
                let said = if state.second { "after" } else { "before" };
                repaint_boundary(semantics(
                    9,
                    SemanticsProperties::label(said),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ))
            }
        }

        set_enabled(true);
        let sink: Rc<RefCell<Option<StateHandle<Which>>>> = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Label { sink: sink.clone() }));

        let size = Size::new(200.0, 100.0);
        let frame = |tree: &mut ElementTree| {
            let mut root = tree.build_render_tree().expect("mounted");
            root.layout(BoxConstraints::loose(size.width, size.height));
            let mut layers = crate::engine::LayerTree::new(size.width as i32, size.height as i32);
            {
                let mut context = PaintContext::new(&mut layers, size);
                root.paint(&mut context, Offset::ZERO);
            }
            flush(size, &root);
            tree_or_fail()
        };

        let said = |nodes: &[SemanticsNode]| {
            nodes
                .iter()
                .find(|n| n.id == 9)
                .map(|n| n.properties.label.clone())
                .unwrap_or_default()
        };

        assert_eq!(said(&frame(&mut tree)), "before");
        // Painted once already, so the boundary is holding a layer.
        assert_eq!(
            said(&frame(&mut tree)),
            "before",
            "the node stopped being reported"
        );

        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.second = true);
        tree.rebuild_dirty();
        assert_eq!(
            said(&frame(&mut tree)),
            "after",
            "a reader was told last frame's label"
        );
        set_enabled(false);
    }

    /// A box that counts how often it has been asked what it says, and can be
    /// made to answer differently every time it is asked.
    ///
    /// Counting the question rather than the answer is the only way to tell
    /// the second gate from the third: both end in `flush` returning `None`,
    /// and they differ in whether the walk happened at all.
    struct Counted {
        asked: Rc<Cell<u32>>,
        /// Whether the answer changes with the count. A box that says
        /// something new every time makes a walk visible in what is sent; one
        /// that says the same thing makes the *absence* of a send visible even
        /// though the walk ran.
        chatty: bool,
        size: Size,
    }

    impl RenderBox for Counted {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.size = Size::new(constraints.max_width, constraints.max_height);
            self.size
        }
        fn size(&self) -> Size {
            self.size
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
        fn describe_semantics(&self) -> Option<SemanticsAnnotation> {
            self.asked.set(self.asked.get() + 1);
            let label = if self.chatty {
                format!("asked {} times", self.asked.get())
            } else {
                "the same as ever".to_string()
            };
            Some(SemanticsAnnotation::new(
                21,
                SemanticsProperties::label(label),
                None,
            ))
        }
    }

    fn counting(asked: &Rc<Cell<u32>>, chatty: bool) -> AnyWidget {
        let asked = Rc::clone(asked);
        leaf(move || Counted {
            asked: Rc::clone(&asked),
            chatty,
            size: Size::ZERO,
        })
    }

    #[test]
    fn a_frame_that_changed_nothing_is_not_walked() {
        // The second gate, and the reason this work was done. Upstream's
        // `flushSemantics` visits what is in
        // `PipelineOwner._nodesNeedingSemanticsUpdate` and nothing else; on a
        // frame where nothing put anything there, no render object is asked
        // what it says. The box below would answer differently every time it
        // were asked, so if the walk ran the tree would change and something
        // would be sent -- which makes "the walk did not run" a thing a test
        // can see rather than a thing a comment claims.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, true));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));

        assert!(
            flush(size, &root).is_some(),
            "the first frame has everything to say"
        );
        assert_eq!(asked.get(), 1);

        // Frames two and three change nothing: no rebuild, no layout, nothing
        // marked. Upstream would visit an empty dirty set; this returns before
        // the walk.
        assert!(flush(size, &root).is_none(), "a quiet frame sent something");
        assert!(flush(size, &root).is_none());
        assert_eq!(asked.get(), 1, "a quiet frame asked the tree what it says");

        // And it is not stuck: whatever marks, walks.
        mark_needs_update();
        assert!(flush(size, &root).is_some(), "a marked frame said nothing");
        assert_eq!(asked.get(), 2);
        set_enabled(false);
    }

    #[test]
    fn a_walk_that_came_out_the_same_sends_nothing() {
        // The third gate. Upstream's `sendSemanticsUpdate` opens with
        // `if (_dirtyNodes.isEmpty) return;` and puts only changed nodes on
        // the wire; here the walk ran -- `asked` proves it -- and produced the
        // tree the platform is already holding, so nothing crosses.
        //
        // This is the ordinary case for anything that animates without
        // speaking: a ripple, a colour tween, a scroll that has come to rest.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, false));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));

        assert!(
            flush(size, &root).is_some(),
            "the first frame has everything to say"
        );
        assert_eq!(asked.get(), 1);

        mark_needs_update();
        assert!(flush(size, &root).is_none(), "the same tree was sent twice");
        assert_eq!(asked.get(), 2, "the walk was supposed to run");
        // What the platform holds is unchanged, not cleared.
        assert!(tree_or_fail().iter().any(|node| node.id == 21));
        set_enabled(false);
    }

    #[test]
    fn laying_out_is_what_marks_the_ordinary_frame() {
        // Upstream calls `markNeedsSemanticsUpdate` from inside
        // `RenderObject.layout`, on the line after `performLayout`, and that
        // single call is what covers nearly everything: a scroll, a rebuild
        // that changed a size, a row that appeared. Here it is
        // `RenderRef::layout`, past the early return -- so a re-layout at the
        // same constraints on a clean tree marks nothing, and a real one does.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, true));
        let mut root = tree.build_render_tree().expect("mounted");

        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_some());

        // Same constraints, clean tree: the early return, and nothing marked.
        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(
            flush(size, &root).is_none(),
            "an unchanged layout marked semantics"
        );

        // A different size is a real layout, and everything that moved has
        // something new to say about where it is.
        root.layout(BoxConstraints::loose(180.0, 90.0));
        assert!(
            flush(Size::new(180.0, 90.0), &root).is_some(),
            "a re-layout said nothing"
        );
        set_enabled(false);
    }

    #[test]
    fn an_action_reaches_the_handler_no_frame_carried() {
        // The cost of skipping walks, paid where it cannot be felt. A rebuild
        // that replaces only a closure changes nothing measured and nothing
        // drawn, so nothing marks itself and no walk happens -- and a handler
        // remembered by the last walk would be the wrong one by exactly one
        // build. `perform_action` asks the live object instead.
        use crate::framework::{BuildContext, StateHandle, StatefulComponent, stateful};

        #[derive(Default)]
        struct Round {
            second: bool,
        }

        struct Chooser {
            sink: Rc<RefCell<Option<StateHandle<Round>>>>,
            called: Rc<Cell<&'static str>>,
        }

        impl StatefulComponent for Chooser {
            type State = Round;

            fn build(
                &self,
                state: &Round,
                handle: StateHandle<Round>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                *self.sink.borrow_mut() = Some(handle);
                let which = if state.second { "second" } else { "first" };
                let called = Rc::clone(&self.called);
                semantics_with_action(
                    12,
                    // Deliberately the same label both times: if the
                    // annotation itself changed, `update_from` would mark and
                    // a walk would happen, and the point is the frame where
                    // one does not.
                    SemanticsProperties::button("Act"),
                    leaf(|| SizedBox::new(50.0, 20.0)),
                    move |_| called.set(which),
                )
            }
        }

        set_enabled(true);
        let size = Size::new(200.0, 100.0);
        let sink: Rc<RefCell<Option<StateHandle<Round>>>> = Rc::new(RefCell::new(None));
        let called = Rc::new(Cell::new("nobody"));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Chooser {
            sink: sink.clone(),
            called: Rc::clone(&called),
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_some());
        assert!(perform_action(&root, 12, SemanticsAction::Tap));
        assert_eq!(called.get(), "first");

        sink.borrow()
            .as_ref()
            .expect("built")
            .set_state(|state| state.second = true);
        tree.rebuild_dirty();
        let root = tree.build_render_tree().expect("still mounted");
        // No layout and no walk: nothing about this frame is worth either.
        assert!(
            flush(size, &root).is_none(),
            "a new closure should not be news"
        );
        assert!(perform_action(&root, 12, SemanticsAction::Tap));
        assert_eq!(
            called.get(),
            "second",
            "the reader called last build's closure"
        );
        set_enabled(false);
    }

    #[test]
    fn a_reader_arriving_is_told_everything() {
        // Upstream's `scheduleInitialSemantics`: the tree a reader has never
        // been shown is entirely news, however quiet the frame is otherwise.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, false));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_some());
        assert!(flush(size, &root).is_none(), "quiet");

        // The reader leaves and another arrives. Nothing on screen moved.
        set_enabled(false);
        assert!(
            super::tree().is_empty(),
            "a tree nobody holds should not be kept"
        );
        set_enabled(true);
        let sent = flush(size, &root).expect("the new reader was told nothing");
        assert!(sent.iter().any(|node| node.id == 21));
        set_enabled(false);
    }

    #[test]
    fn a_fade_to_nothing_marks_but_a_fade_between_two_visible_values_does_not() {
        // Upstream's `RenderOpacity.set opacity` marks semantics on
        // `wasVisible != isVisible` and on nothing else. It is the one place
        // in this framework where a repaint alone changes what a reader would
        // hear, because a fully transparent subtree is one
        // `visit_children_for_semantics` refuses to enter.
        use crate::render::{RenderOpacity, UpdateEffect};

        set_enabled(true);
        let mut faded = RenderOpacity::new(1.0, SizedBox::new(10.0, 10.0));
        let step = |faded: &mut RenderOpacity, to: f32| {
            let mut fresh = RenderOpacity::new(to, SizedBox::new(10.0, 10.0));
            NEEDS_UPDATE.with(|needs| needs.set(false));
            let effect = faded.update_from(&mut fresh);
            (effect, NEEDS_UPDATE.with(|needs| needs.get()))
        };

        let (effect, marked) = step(&mut faded, 0.5);
        assert_eq!(
            effect,
            Some(UpdateEffect::Relayout),
            "the child is a new object"
        );
        assert!(
            !marked,
            "half way is still visible, and says the same thing"
        );

        let (_, marked) = step(&mut faded, 0.0);
        assert!(
            marked,
            "a subtree that stopped being drawn stopped being described"
        );

        let (_, marked) = step(&mut faded, 0.25);
        assert!(marked, "and it is describable again");
        set_enabled(false);
    }

    // -- AttributedString ---------------------------------------------------------

    #[test]
    fn joining_two_attributed_strings_moves_the_right_ones_ranges() {
        // The whole of what concatenation has to do: every attribute past the
        // seam would otherwise point at the wrong letters.
        let left = AttributedString::with_attributes(
            "hello ",
            vec![StringAttribute::SpellOut {
                range: TextRange::new(0, 5),
            }],
        );
        let right = AttributedString::with_attributes(
            "world",
            vec![StringAttribute::Locale {
                range: TextRange::new(0, 5),
                locale: "fr".to_string(),
            }],
        );

        let joined = &left + &right;
        assert_eq!(joined.string(), "hello world");
        assert_eq!(joined.attributes().len(), 2);
        assert_eq!(
            joined.attributes()[0].range(),
            TextRange::new(0, 5),
            "the left one did not move"
        );
        assert_eq!(
            joined.attributes()[1].range(),
            TextRange::new(6, 11),
            "the right one moved by the left string's length"
        );
        assert_eq!(
            &joined.string()[6..11],
            "world",
            "and the moved range still names the words it was on"
        );
    }

    #[test]
    fn an_attribute_keeps_its_kind_when_its_range_moves() {
        let right = AttributedString::with_attributes(
            "bonjour",
            vec![StringAttribute::Locale {
                range: TextRange::new(0, 7),
                locale: "fr".to_string(),
            }],
        );
        let joined = &AttributedString::new("say ") + &right;
        assert_eq!(
            joined.attributes()[0],
            StringAttribute::Locale {
                range: TextRange::new(4, 11),
                locale: "fr".to_string()
            },
            "still French, just further along"
        );
    }

    #[test]
    fn joining_with_an_empty_string_hands_back_the_other_one_whole() {
        let text = AttributedString::with_attributes(
            "code",
            vec![StringAttribute::SpellOut {
                range: TextRange::new(0, 4),
            }],
        );
        let empty = AttributedString::new("");

        assert_eq!(&empty + &text, text);
        assert_eq!(&text + &empty, text);
        assert_eq!(
            (&text + &empty).attributes().len(),
            1,
            "and the attributes survived"
        );
    }

    #[test]
    fn an_empty_string_carries_no_attributes_which_is_what_makes_the_shortcut_safe() {
        // The early returns in `concat` hand back one operand whole. That is
        // only lossless because an empty string has nothing to hand over -- the
        // invariant the constructor asserts. Without it the shortcut would drop
        // attributes the general path would have kept.
        //
        // Checked by removing the shortcut and watching the tests above stay
        // green: they cannot tell the two paths apart, because there is nothing
        // to tell apart. This is the assertion that actually holds the claim up.
        let empty = AttributedString::new("");
        assert!(empty.attributes().is_empty());
        assert!(empty.is_empty());

        // And the general path agrees with the shortcut, which is the whole
        // reason the shortcut is allowed to exist.
        let text = AttributedString::with_attributes(
            "code",
            vec![StringAttribute::SpellOut {
                range: TextRange::new(0, 4),
            }],
        );
        let the_long_way = AttributedString::with_attributes(
            format!("{}{}", empty.string(), text.string()),
            text.attributes().to_vec(),
        );
        assert_eq!(&empty + &text, the_long_way);
    }

    #[test]
    fn two_attributed_strings_are_equal_when_their_attributes_are_too() {
        let spell = |s: &str| {
            AttributedString::with_attributes(
                s,
                vec![StringAttribute::SpellOut {
                    range: TextRange::new(0, s.len() as isize),
                }],
            )
        };
        assert_eq!(spell("abc"), spell("abc"));
        assert_ne!(
            spell("abc"),
            AttributedString::new("abc"),
            "same letters, different instructions for reading them"
        );
    }

    #[test]
    fn an_attributed_string_property_hides_itself_when_there_is_nothing_to_say() {
        assert!(!AttributedStringProperty::new("label", None).is_interesting());
        assert!(
            !AttributedStringProperty::new("label", Some(AttributedString::new("")))
                .is_interesting()
        );
        assert!(
            AttributedStringProperty::new("label", Some(AttributedString::new("Increment")))
                .is_interesting()
        );
    }

    #[test]
    fn an_attributed_string_property_prints_attributes_only_when_there_are_some() {
        let plain = AttributedStringProperty::new("label", Some(AttributedString::new("abc")));
        assert_eq!(plain.value_to_string(), "\"abc\"");

        let attributed = AttributedStringProperty::new(
            "label",
            Some(AttributedString::with_attributes(
                "abc",
                vec![StringAttribute::SpellOut {
                    range: TextRange::new(0, 3),
                }],
            )),
        );
        assert!(attributed.value_to_string().starts_with("\"abc\" "));
        assert!(attributed.value_to_string().contains("SpellOut"));
    }

    // -- SemanticsTag -------------------------------------------------------------

    #[test]
    fn a_tags_name_is_not_its_identity() {
        // Upstream's own emphasis: the name is for debugging, and two tags made
        // with `new` and the same name are not the same tag. A tag compared by
        // name would make two unrelated subsystems that picked the same word
        // interfere.
        let mine = SemanticsTag::new("selected");
        let theirs = SemanticsTag::new("selected");
        assert_eq!(mine.name(), theirs.name());
        assert_ne!(mine, theirs, "same word, different tags");
        assert_eq!(mine, mine.clone(), "and a copy is the same tag");
    }

    #[test]
    fn a_tag_shared_from_one_place_is_one_tag() {
        // The Rust way to get upstream's `const` behaviour: declare it once.
        let shared = SemanticsTag::new("scrolled into view");
        let a = shared.clone();
        let b = shared.clone();
        assert_eq!(a, b);

        // And it works as a key, which is what marking a node is for.
        let mut marked = std::collections::HashSet::new();
        marked.insert(a);
        assert!(marked.contains(&b));
        assert!(!marked.contains(&SemanticsTag::new("scrolled into view")));
    }

    // -- SemanticsHintOverrides ---------------------------------------------------

    #[test]
    fn a_hint_override_says_what_happens_and_not_how_to_do_it() {
        // Upstream's rule, as its own examples: "show movies", not "double tap
        // to show movies". The platform already tells the reader which gesture
        // its own device wants.
        let hints = SemanticsHintOverrides::new()
            .with_tap_hint("show movies")
            .with_long_press_hint("show tooltip");
        assert_eq!(hints.on_tap_hint(), Some("show movies"));
        assert_eq!(hints.on_long_press_hint(), Some("show tooltip"));
        assert!(hints.is_not_empty());
    }

    #[test]
    fn no_hint_and_an_empty_hint_are_different_things() {
        // Absent means "use the standard phrasing"; empty would mean "say
        // nothing", which hides what the control does. Upstream asserts against
        // the second, which is why only the first is reachable here.
        let none = SemanticsHintOverrides::new();
        assert!(!none.is_not_empty());
        assert_eq!(none.on_tap_hint(), None);

        let one = SemanticsHintOverrides::new().with_tap_hint("open");
        assert!(one.is_not_empty());
        assert_eq!(one.on_long_press_hint(), None, "the other stays absent");
    }

    // -- CustomSemanticsAction ----------------------------------------------------

    #[test]
    fn a_custom_action_is_either_a_new_one_or_an_override_and_never_both() {
        let new_one = CustomSemanticsAction::labelled("Add to favourites");
        assert_eq!(new_one.label(), Some("Add to favourites"));
        assert_eq!(new_one.hint(), None);
        assert_eq!(new_one.action(), None);

        let override_one = CustomSemanticsAction::overriding("show movies", SemanticsAction::Tap);
        assert_eq!(override_one.label(), None);
        assert_eq!(override_one.hint(), Some("show movies"));
        assert_eq!(override_one.action(), Some(SemanticsAction::Tap));
    }

    #[test]
    fn a_custom_actions_identifier_is_stable_and_keyed_on_its_value() {
        // Unlike a tag, whose whole point is the opposite: two nodes offering
        // the same label are offering the same action and share an id.
        CustomSemanticsAction::reset_for_tests();
        let first = CustomSemanticsAction::labelled("Archive");
        let same = CustomSemanticsAction::labelled("Archive");
        let other = CustomSemanticsAction::labelled("Delete");

        let id = CustomSemanticsAction::identifier(&first);
        assert_eq!(
            CustomSemanticsAction::identifier(&same),
            id,
            "the same action, however it was built"
        );
        assert_eq!(
            CustomSemanticsAction::identifier(&first),
            id,
            "and asking twice does not hand out a second id"
        );
        assert_ne!(CustomSemanticsAction::identifier(&other), id);

        assert_eq!(CustomSemanticsAction::from_identifier(id), Some(first));
        assert_eq!(CustomSemanticsAction::from_identifier(9999), None);
        CustomSemanticsAction::reset_for_tests();
    }

    #[test]
    fn an_overriding_action_is_not_the_same_action_as_a_label_that_reads_alike() {
        CustomSemanticsAction::reset_for_tests();
        let labelled = CustomSemanticsAction::labelled("open");
        let overriding = CustomSemanticsAction::overriding("open", SemanticsAction::Tap);
        assert_ne!(
            CustomSemanticsAction::identifier(&labelled),
            CustomSemanticsAction::identifier(&overriding)
        );
        CustomSemanticsAction::reset_for_tests();
    }

    #[test]
    fn resetting_the_registry_starts_the_ids_over() {
        // It exists because the registry outlives any one test, so one test's
        // actions would otherwise decide the next one's ids.
        CustomSemanticsAction::reset_for_tests();
        let first = CustomSemanticsAction::identifier(&CustomSemanticsAction::labelled("a"));
        CustomSemanticsAction::reset_for_tests();
        let again = CustomSemanticsAction::identifier(&CustomSemanticsAction::labelled("b"));
        assert_eq!(first, again, "a different action, the same first id");
    }

    // -- Sort keys ----------------------------------------------------------------

    #[test]
    fn a_lower_ordinal_is_read_first() {
        let first = OrdinalSortKey::new(1.0);
        let second = OrdinalSortKey::new(2.0);
        assert!(first < second);
        assert_eq!(first.compare(&OrdinalSortKey::new(1.0)), Ordering::Equal);
    }

    #[test]
    fn keys_with_no_name_are_read_before_keys_with_one() {
        // Upstream: "Keys that don't have a name are sorted together and come
        // before those with a name."
        let unnamed = OrdinalSortKey::new(100.0);
        let named = OrdinalSortKey::new(1.0).with_name("toolbar");
        assert!(
            unnamed < named,
            "the unnamed one goes first even though its order is far higher"
        );
    }

    #[test]
    fn the_name_is_a_grouping_and_it_wins_over_the_order() {
        // Two keys with different names are ordered by their names whatever
        // their numbers say -- so a name is not a label, it decides the
        // sequence.
        let early_name_late_order = OrdinalSortKey::new(999.0).with_name("aaa");
        let late_name_early_order = OrdinalSortKey::new(1.0).with_name("zzz");
        assert!(early_name_late_order < late_name_early_order);
    }

    #[test]
    fn keys_in_the_same_group_fall_back_to_their_order() {
        let first = OrdinalSortKey::new(1.0).with_name("toolbar");
        let second = OrdinalSortKey::new(2.0).with_name("toolbar");
        assert!(first < second);
    }

    #[test]
    fn a_list_of_keys_sorts_into_the_order_a_reader_walks() {
        let mut keys = vec![
            OrdinalSortKey::new(2.0).with_name("body"),
            OrdinalSortKey::new(5.0),
            OrdinalSortKey::new(1.0).with_name("body"),
            OrdinalSortKey::new(1.0),
            OrdinalSortKey::new(1.0).with_name("aside"),
        ];
        keys.sort();
        let described: Vec<(Option<&str>, f64)> =
            keys.iter().map(|k| (k.name(), k.order())).collect();
        assert_eq!(
            described,
            vec![
                (None, 1.0),
                (None, 5.0),
                (Some("aside"), 1.0),
                (Some("body"), 1.0),
                (Some("body"), 2.0),
            ],
            "unnamed first by order, then each group by name, then by order"
        );
    }

    // -- SemanticsLabelBuilder ----------------------------------------------------

    #[test]
    fn joining_two_parts_puts_the_separator_between_them() {
        // Upstream's own first example.
        let mut builder = SemanticsLabelBuilder::new();
        builder.add_part("Hello");
        builder.add_part("world");
        assert_eq!(builder.build(), "Hello world");
    }

    #[test]
    fn an_empty_part_is_dropped_rather_than_leaving_a_doubled_separator() {
        let mut builder = SemanticsLabelBuilder::new();
        builder.add_part("Hello");
        builder.add_part("");
        builder.add_part("world");
        assert_eq!(builder.len(), 2, "the empty one was never kept");
        assert_eq!(builder.build(), "Hello world");
    }

    #[test]
    fn no_parts_is_an_empty_label_and_one_part_is_itself() {
        let mut builder = SemanticsLabelBuilder::new();
        assert!(builder.is_empty());
        assert_eq!(builder.build(), "");

        builder.add_part("Increment");
        assert_eq!(builder.build(), "Increment", "no separator, nothing added");
    }

    #[test]
    fn a_part_in_the_other_direction_is_wrapped_in_embedding_marks() {
        // Upstream's second example: a left-to-right label with an Arabic part
        // in it. Without the marks the reader runs the two together in
        // whichever direction it guessed.
        let mut builder = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        builder.add_part_in("Welcome", TextDirection::Ltr);
        builder.add_part_in("\u{645}\u{631}\u{62d}\u{628}\u{627}", TextDirection::Rtl);

        let label = builder.build();
        assert_eq!(
            label,
            format!(
                "Welcome {}{}{}",
                Unicode::RLE,
                "\u{645}\u{631}\u{62d}\u{628}\u{627}",
                Unicode::PDF
            )
        );
    }

    #[test]
    fn a_part_in_the_same_direction_is_left_alone() {
        let mut builder = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        builder.add_part_in("Welcome", TextDirection::Ltr);
        builder.add_part_in("back", TextDirection::Ltr);
        assert_eq!(builder.build(), "Welcome back", "no marks to add");
    }

    #[test]
    fn a_part_that_names_no_direction_inherits_the_builders_and_so_never_differs() {
        // The second rule. Only an explicitly contrary part gets marks -- an
        // unnamed one takes the builder's direction and therefore cannot differ
        // from it.
        let mut builder = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        builder.add_part("Welcome");
        builder.add_part("\u{645}\u{631}\u{62d}\u{628}\u{627}");
        assert_eq!(
            builder.build(),
            "Welcome \u{645}\u{631}\u{62d}\u{628}\u{627}",
            "Arabic text, no marks, because nobody said it was Arabic"
        );
    }

    #[test]
    fn a_builder_with_no_direction_of_its_own_wraps_nothing() {
        // A part can only differ from a direction that exists.
        let mut builder = SemanticsLabelBuilder::new();
        builder.add_part_in("Welcome", TextDirection::Ltr);
        builder.add_part_in("\u{645}\u{631}\u{62d}\u{628}\u{627}", TextDirection::Rtl);
        assert_eq!(
            builder.build(),
            "Welcome \u{645}\u{631}\u{62d}\u{628}\u{627}"
        );
    }

    #[test]
    fn the_first_part_is_never_wrapped_however_contrary_it_is() {
        // Upstream's third rule, and it looks like an oversight: the first part
        // is written to the buffer before the direction-checking loop starts.
        // A label whose first piece is the contrary one gets no marks on the
        // piece that most needs them.
        //
        // Ported as-is because an application built against upstream will have
        // been laid out around this, and a port that quietly did better would
        // be the odd one out.
        let mut builder = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        builder.add_part_in("\u{645}\u{631}\u{62d}\u{628}\u{627}", TextDirection::Rtl);
        builder.add_part_in("Welcome", TextDirection::Ltr);

        let label = builder.build();
        assert_eq!(
            label, "\u{645}\u{631}\u{62d}\u{628}\u{627} Welcome",
            "the contrary first part is bare"
        );
        assert!(
            !label.contains(Unicode::RLE),
            "and no embedding mark anywhere"
        );

        // The same two parts the other way round *are* marked, which is what
        // makes this a rule about position rather than about content.
        let mut reversed = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        reversed.add_part_in("Welcome", TextDirection::Ltr);
        reversed.add_part_in("\u{645}\u{631}\u{62d}\u{628}\u{627}", TextDirection::Rtl);
        assert!(reversed.build().contains(Unicode::RLE));
    }

    #[test]
    fn a_single_contrary_part_is_returned_bare() {
        // The third rule reached the other way. Note this does *not* test the
        // single-part early return: the general path would answer the same,
        // because it leaves the first part unprocessed too.
        let mut builder = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        builder.add_part_in("\u{645}\u{631}\u{62d}\u{628}\u{627}", TextDirection::Rtl);
        assert_eq!(builder.build(), "\u{645}\u{631}\u{62d}\u{628}\u{627}");
    }

    #[test]
    fn an_empty_separator_leaves_only_the_marks_between_parts() {
        let mut builder = SemanticsLabelBuilder::new()
            .with_separator("")
            .with_text_direction(TextDirection::Ltr);
        builder.add_part_in("a", TextDirection::Ltr);
        builder.add_part_in("b", TextDirection::Rtl);
        assert_eq!(
            builder.build(),
            format!("a{}b{}", Unicode::RLE, Unicode::PDF)
        );
    }

    #[test]
    fn a_custom_separator_is_used_between_every_pair() {
        let mut builder = SemanticsLabelBuilder::new().with_separator(", ");
        builder.add_part("one");
        builder.add_part("two");
        builder.add_part("three");
        assert_eq!(builder.build(), "one, two, three");
    }

    #[test]
    fn clearing_lets_one_builder_make_a_second_label() {
        let mut builder = SemanticsLabelBuilder::new();
        builder.add_part("first");
        assert_eq!(builder.build(), "first");

        builder.clear();
        assert!(builder.is_empty());
        builder.add_part("second");
        assert_eq!(
            builder.build(),
            "second",
            "nothing left over from the first"
        );
    }

    #[test]
    fn the_embedding_marks_are_the_ones_unicode_names() {
        // RLE and LRE open an embedding and PDF closes it. Getting one wrong
        // leaves the reader in that direction for the rest of the label.
        assert_eq!(Unicode::RLE, '\u{202B}');
        assert_eq!(Unicode::LRE, '\u{202A}');
        assert_eq!(Unicode::PDF, '\u{202C}');
    }

    // -- Who is asking for semantics ----------------------------------------------

    #[test]
    fn semantics_is_on_while_a_handle_is_alive_and_off_when_it_drops() {
        SemanticsBinding::reset_for_tests();
        assert!(!enabled());
        assert_eq!(SemanticsBinding::outstanding_handles(), 0);

        {
            let _handle = SemanticsBinding::ensure_semantics();
            assert!(enabled());
            assert_eq!(SemanticsBinding::outstanding_handles(), 1);
        }

        assert!(!enabled(), "the handle went out of scope");
        assert_eq!(SemanticsBinding::outstanding_handles(), 0);
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn the_count_and_the_flag_agree_the_way_upstream_asserts_they_do() {
        // Upstream reads `assert(_semanticsEnabled.value == (_outstandingHandles
        // > 0))` on every access. Here that is a test rather than an assert,
        // because there is nowhere to put an assert that runs on every read of
        // a plain function.
        SemanticsBinding::reset_for_tests();
        let a = SemanticsBinding::ensure_semantics();
        assert_eq!(enabled(), SemanticsBinding::outstanding_handles() > 0);
        let b = SemanticsBinding::ensure_semantics();
        assert_eq!(enabled(), SemanticsBinding::outstanding_handles() > 0);
        drop(a);
        assert_eq!(enabled(), SemanticsBinding::outstanding_handles() > 0);
        drop(b);
        assert_eq!(enabled(), SemanticsBinding::outstanding_handles() > 0);
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn a_second_client_keeps_semantics_on_after_the_first_lets_go() {
        // The whole reason this is a count and not a boolean.
        SemanticsBinding::reset_for_tests();
        let inspector = SemanticsBinding::ensure_semantics();
        let other = SemanticsBinding::ensure_semantics();
        assert_eq!(SemanticsBinding::outstanding_handles(), 2);

        drop(other);
        assert!(enabled(), "the inspector is still looking");
        drop(inspector);
        assert!(!enabled());
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn the_platform_letting_go_does_not_switch_off_a_client_that_asked() {
        // The case a boolean gets wrong: an inspector that turned semantics on
        // to read the tree used to be switched off again the next time the
        // platform said no reader was attached, halfway through the inspection.
        SemanticsBinding::reset_for_tests();
        set_enabled(true);
        let inspector = SemanticsBinding::ensure_semantics();
        assert_eq!(SemanticsBinding::outstanding_handles(), 2);

        set_enabled(false);
        assert!(
            enabled(),
            "the platform released its handle; the inspector's still stands"
        );
        assert_eq!(SemanticsBinding::outstanding_handles(), 1);

        drop(inspector);
        assert!(!enabled());
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn the_platform_reports_its_state_rather_than_a_change_so_saying_it_twice_is_free() {
        // The shell calls this with whatever the platform currently says, which
        // may be what it said last time.
        SemanticsBinding::reset_for_tests();
        set_enabled(true);
        set_enabled(true);
        set_enabled(true);
        assert_eq!(
            SemanticsBinding::outstanding_handles(),
            1,
            "one platform handle, however many times it was announced"
        );

        set_enabled(false);
        set_enabled(false);
        assert_eq!(SemanticsBinding::outstanding_handles(), 0);
        assert!(!enabled());
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn disposing_a_handle_twice_releases_it_once() {
        SemanticsBinding::reset_for_tests();
        let keep = SemanticsBinding::ensure_semantics();
        let mut handle = SemanticsBinding::ensure_semantics();
        assert_eq!(SemanticsBinding::outstanding_handles(), 2);

        handle.dispose();
        handle.dispose();
        assert_eq!(
            SemanticsBinding::outstanding_handles(),
            1,
            "and the drop that follows does nothing either"
        );
        drop(handle);
        assert_eq!(SemanticsBinding::outstanding_handles(), 1);

        drop(keep);
        assert_eq!(SemanticsBinding::outstanding_handles(), 0);
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn turning_semantics_off_clears_what_the_platform_was_holding() {
        // The behaviour `set_enabled` had before the handles went in, checked
        // through the new path: a reader that left leaves an empty tree behind,
        // so the next one is not compared against a stale one.
        SemanticsBinding::reset_for_tests();
        set_enabled(true);
        assert!(enabled());
        set_enabled(false);
        assert!(tree().is_empty());
        SemanticsBinding::reset_for_tests();
    }

    // -- Listeners ------------------------------------------------------------------

    #[test]
    fn a_listener_hears_the_edges_and_not_every_call() {
        SemanticsBinding::reset_for_tests();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&heard);
        let token =
            SemanticsBinding::add_enabled_listener(move |on| recorder.borrow_mut().push(on));

        let first = SemanticsBinding::ensure_semantics();
        let second = SemanticsBinding::ensure_semantics();
        drop(second);
        drop(first);

        assert_eq!(
            *heard.borrow(),
            vec![true, false],
            "on at the first handle and off at the last, nothing in between"
        );
        SemanticsBinding::remove_enabled_listener(token);
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn a_removed_listener_stops_hearing() {
        SemanticsBinding::reset_for_tests();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&heard);
        let token =
            SemanticsBinding::add_enabled_listener(move |on| recorder.borrow_mut().push(on));

        drop(SemanticsBinding::ensure_semantics());
        assert_eq!(heard.borrow().len(), 2);

        assert!(SemanticsBinding::remove_enabled_listener(token));
        assert!(
            !SemanticsBinding::remove_enabled_listener(token),
            "removing it again finds nothing"
        );
        drop(SemanticsBinding::ensure_semantics());
        assert_eq!(heard.borrow().len(), 2, "nothing new was heard");
        SemanticsBinding::reset_for_tests();
    }

    #[test]
    fn a_token_still_names_its_own_listener_after_an_earlier_one_is_removed() {
        // The token is an index into the list, so removing an entry has to
        // leave a hole rather than close the gap -- otherwise every token after
        // it silently comes to mean the next listener along.
        //
        // Three listeners, and the first is removed before the *third* is:
        // with a shifting removal the third's token would be out of range by
        // then, so it would survive and be heard.
        SemanticsBinding::reset_for_tests();
        let heard = Rc::new(RefCell::new(Vec::new()));
        let (one, two, three) = (Rc::clone(&heard), Rc::clone(&heard), Rc::clone(&heard));
        let a = SemanticsBinding::add_enabled_listener(move |_| one.borrow_mut().push("a"));
        let b = SemanticsBinding::add_enabled_listener(move |_| two.borrow_mut().push("b"));
        let c = SemanticsBinding::add_enabled_listener(move |_| three.borrow_mut().push("c"));

        assert!(SemanticsBinding::remove_enabled_listener(a));
        assert!(
            SemanticsBinding::remove_enabled_listener(c),
            "c's token still finds c"
        );

        drop(SemanticsBinding::ensure_semantics());
        assert_eq!(
            *heard.borrow(),
            vec!["b", "b"],
            "only b is left, and it heard both edges"
        );
        SemanticsBinding::remove_enabled_listener(b);
        SemanticsBinding::reset_for_tests();
    }

    // -- SemanticsConfiguration: the merge ----------------------------------------

    fn said(label: &str) -> SemanticsConfiguration {
        SemanticsConfiguration {
            label: AttributedString::new(label),
            has_been_annotated: true,
            ..SemanticsConfiguration::new()
        }
    }

    #[test]
    fn absorbing_an_unannotated_child_changes_nothing() {
        // There is nothing in it to take. Upstream's first line.
        let mut parent = said("Submit");
        let before = parent.label.string().to_string();
        parent.absorb(&SemanticsConfiguration::new());
        assert_eq!(parent.label.string(), before);
    }

    #[test]
    fn labels_are_joined_rather_than_chosen_between() {
        // Two things merged into one node have two things to say, and a reader
        // should hear both. The separator is a newline, which is upstream's.
        let mut parent = said("Submit");
        parent.absorb(&said("disabled"));
        assert_eq!(parent.label.string(), "Submit\ndisabled");
    }

    #[test]
    fn a_value_is_taken_only_when_the_parent_has_none() {
        // A node has one value; the parent asked first.
        let mut parent = said("Volume");
        parent.value = AttributedString::new("7");
        let mut child = said("");
        child.value = AttributedString::new("9");
        parent.absorb(&child);
        assert_eq!(parent.value.string(), "7", "the parent's own stands");

        let mut empty_parent = said("Volume");
        empty_parent.absorb(&child);
        assert_eq!(empty_parent.value.string(), "9", "and is taken when absent");
    }

    #[test]
    fn a_singular_number_is_first_wins() {
        let mut parent = said("List");
        parent.scroll_index = Some(3);
        let mut child = said("Row");
        child.scroll_index = Some(9);
        child.scroll_child_count = Some(40);

        parent.absorb(&child);
        assert_eq!(parent.scroll_index, Some(3), "the parent's own");
        assert_eq!(
            parent.scroll_child_count,
            Some(40),
            "and the child's where the parent had none"
        );
    }

    #[test]
    fn an_identifiers_unset_is_the_empty_string_and_not_an_absence() {
        // Upstream's field is a plain String, so the merge tests it against
        // `""` rather than null -- which means an identifier deliberately set
        // to empty cannot be told from one never set.
        let mut parent = said("a");
        let mut child = said("b");
        child.identifier = "row-3".to_string();
        parent.absorb(&child);
        assert_eq!(parent.identifier, "row-3");

        let mut named = said("a");
        named.identifier = "mine".to_string();
        named.absorb(&child);
        assert_eq!(named.identifier, "mine");
    }

    #[test]
    fn the_direction_is_settled_before_the_labels_are_joined() {
        // Upstream's line order, and it matters: the child's label is wrapped
        // against the direction the *result* will have. Taking the child's
        // direction after joining would compare its label against a direction
        // that was about to become its own, and leave the marks off.
        let mut parent = said("");
        // Parent has no direction of its own; the child's becomes the node's.
        let mut child = said("hello");
        child.text_direction = Some(TextDirection::Rtl);

        parent.absorb(&child);
        assert_eq!(parent.text_direction, Some(TextDirection::Rtl));
        assert_eq!(
            parent.label.string(),
            "hello",
            "no marks: by the time the labels joined, the directions agreed"
        );
    }

    #[test]
    fn a_child_that_reads_the_other_way_is_wrapped_into_the_label() {
        let mut parent = said("Welcome");
        parent.text_direction = Some(TextDirection::Ltr);
        let mut child = said("\u{645}\u{631}\u{62d}\u{628}\u{627}");
        child.text_direction = Some(TextDirection::Rtl);

        parent.absorb(&child);
        assert!(parent.label.string().contains(Unicode::RLE));
        assert!(parent.label.string().contains(Unicode::PDF));
        assert!(parent.label.string().starts_with("Welcome\n"));
    }

    #[test]
    fn flags_merge_and_actions_union() {
        let mut parent = said("Row");
        parent.actions.push(SemanticsAction::Tap);

        let mut child = said("Check");
        child.flags.checked = SemanticsCheckState::Checked;
        child.actions.push(SemanticsAction::LongPress);
        child.actions.push(SemanticsAction::Tap);

        parent.absorb(&child);
        // The parent had no check state, so it takes the child's outright --
        // which is the arm `||` used to get right by accident and would have
        // got wrong the moment the parent was unchecked.
        assert_eq!(parent.flags.checked, SemanticsCheckState::Checked);
        assert_eq!(parent.actions.len(), 2, "Tap was not added twice");
        assert!(parent.actions.contains(&SemanticsAction::LongPress));
    }

    #[test]
    fn a_blocked_child_hands_over_only_its_focus_notifications() {
        // Blocking is about refusing to *act*, not about refusing to know where
        // the reader is -- so the two accessibility-focus actions survive and
        // nothing else does.
        let mut parent = said("Behind a barrier");
        let mut child = said("Button");
        child.is_blocking_user_actions = true;
        child.actions.push(SemanticsAction::Tap);
        child
            .actions
            .push(SemanticsAction::DidGainAccessibilityFocus);
        child
            .actions
            .push(SemanticsAction::DidLoseAccessibilityFocus);

        parent.absorb(&child);
        assert!(!parent.actions.contains(&SemanticsAction::Tap), "refused");
        assert!(
            parent
                .actions
                .contains(&SemanticsAction::DidGainAccessibilityFocus)
        );
        assert!(
            parent
                .actions
                .contains(&SemanticsAction::DidLoseAccessibilityFocus)
        );
    }

    // -- SemanticsConfiguration: compatibility ------------------------------------

    #[test]
    fn anything_is_compatible_with_nothing() {
        let config = said("a");
        assert!(config.is_compatible_with(None));
        assert!(config.is_compatible_with(Some(&SemanticsConfiguration::new())));
        assert!(SemanticsConfiguration::new().is_compatible_with(Some(&config)));
    }

    #[test]
    fn two_labels_may_merge_and_two_values_may_not() {
        // The distinction the whole check is built on: labels concatenate, so
        // two of them are fine; a node has one value, and a reader would hear
        // only one of two.
        assert!(said("a").is_compatible_with(Some(&said("b"))));

        let mut one = said("a");
        one.value = AttributedString::new("7");
        let mut two = said("b");
        two.value = AttributedString::new("9");
        assert!(!one.is_compatible_with(Some(&two)));
    }

    #[test]
    fn two_handlers_for_one_gesture_cannot_merge() {
        let mut one = said("a");
        one.actions.push(SemanticsAction::Tap);
        let mut two = said("b");
        two.actions.push(SemanticsAction::Tap);
        assert!(!one.is_compatible_with(Some(&two)));

        two.actions = vec![SemanticsAction::LongPress];
        assert!(
            one.is_compatible_with(Some(&two)),
            "different gestures are fine"
        );
    }

    #[test]
    fn a_button_and_a_text_field_may_merge_because_a_button_is_not_a_role() {
        // The plausible reading is that they cannot, and that is what I wrote
        // first. Upstream's `hasConflictingFlags` tests the *same* flag on both
        // sides, and `isButton` is absent from `_hasExplicitRole` -- a button
        // is a trait a node can have alongside being something else.
        let mut button = said("Send");
        button.flags.is_button = true;
        let mut field = said("Message");
        field.flags.is_text_field = true;
        assert!(button.is_compatible_with(Some(&field)));
        assert!(!button.has_explicit_role(), "a button claims no role");
        assert!(field.has_explicit_role());
    }

    #[test]
    fn two_of_the_same_kind_cannot_merge() {
        let mut one = said("Send");
        one.flags.is_button = true;
        let mut two = said("Cancel");
        two.flags.is_button = true;
        assert!(
            !one.is_compatible_with(Some(&two)),
            "one thing to press where there were two"
        );
    }

    #[test]
    fn two_things_that_each_claim_a_role_cannot_merge_even_when_the_roles_differ() {
        // This is the check that does the work the flag conflict is often
        // assumed to do.
        let mut field = said("Message");
        field.flags.is_text_field = true;
        let mut slider = said("Volume");
        slider.flags.is_slider = true;
        assert!(
            !field.flags.conflicts_with(&slider.flags),
            "different flags"
        );
        assert!(
            !field.is_compatible_with(Some(&slider)),
            "but both claim a role"
        );
    }

    #[test]
    fn state_flags_do_not_conflict_the_way_kind_flags_do() {
        // One node contributing "checkable" and another "focused" describes a
        // thing that is both, which is a real thing.
        let mut checkable = said("a");
        checkable.flags.checked = SemanticsCheckState::Unchecked;
        let mut focused = said("b");
        focused.flags.focused = SemanticsTristate::True;
        assert!(checkable.is_compatible_with(Some(&focused)));
        assert!(!checkable.flags.conflicts_with(&focused.flags));
    }

    #[test]
    fn two_positions_in_one_parent_merge_and_the_outer_one_wins() {
        // This test used to assert the opposite, against a rule this port had
        // and upstream does not: `isCompatibleWith` asks about
        // `platformViewId`, `maxValueLength`, `currentValueLength`,
        // `attributedValue`, `minValue` and `maxValue`, and about neither
        // `indexInParent` nor `hintOverrides`.
        //
        // What upstream does instead is let the merge happen and drop the
        // child's by first-wins, which is the same rule every other singular
        // slot follows. Refusing it split one node into two.
        let mut one = said("a");
        one.index_in_parent = Some(1);
        let mut two = said("b");
        two.index_in_parent = Some(2);
        assert!(one.is_compatible_with(Some(&two)));
        one.absorb(&two);
        assert_eq!(one.index_in_parent, Some(1), "the parent asked first");
    }

    #[test]
    fn two_sets_of_hint_overrides_merge_the_same_way() {
        // The other rule that was here and is not upstream's. Same shape, and
        // worth its own test because the two were removed together and a
        // single test would let one of them come back unnoticed.
        let mut one = said("a");
        one.hint_overrides = Some(SemanticsHintOverrides::new().with_tap_hint("open"));
        let mut two = said("b");
        two.hint_overrides = Some(SemanticsHintOverrides::new().with_tap_hint("close"));
        assert!(one.is_compatible_with(Some(&two)));
        one.absorb(&two);
        assert_eq!(
            one.hint_overrides.as_ref().and_then(|h| h.on_tap_hint()),
            Some("open")
        );
    }

    #[test]
    fn blocking_survives_a_merge_that_the_parent_did_not_ask_for() {
        // The one strongest-wins rule in the class. A parent that blocks
        // nothing swallows a child that blocks its subtree, and the merged node
        // blocks the subtree: the child's promise was about what a reader must
        // not reach, and after the merge every path that reached the child
        // reaches this node.
        let mut parent = said("a");
        let mut child = said("b");
        child.accessibility_focus_block_type = AccessibilityFocusBlockType::BlockSubtree;
        parent.absorb(&child);
        assert_eq!(
            parent.accessibility_focus_block_type,
            AccessibilityFocusBlockType::BlockSubtree
        );

        // And it does not weaken, which is the half a first-wins spelling
        // would get right by accident.
        let mut strong = said("a");
        strong.accessibility_focus_block_type = AccessibilityFocusBlockType::BlockSubtree;
        let mut weak = said("b");
        weak.accessibility_focus_block_type = AccessibilityFocusBlockType::BlockNode;
        strong.absorb(&weak);
        assert_eq!(
            strong.accessibility_focus_block_type,
            AccessibilityFocusBlockType::BlockSubtree
        );

        // The case first-wins gets *wrong*: the parent's is the weaker one.
        let mut outer = said("a");
        outer.accessibility_focus_block_type = AccessibilityFocusBlockType::BlockNode;
        let mut inner = said("b");
        inner.accessibility_focus_block_type = AccessibilityFocusBlockType::BlockSubtree;
        outer.absorb(&inner);
        assert_eq!(
            outer.accessibility_focus_block_type,
            AccessibilityFocusBlockType::BlockSubtree,
            "first-wins would have kept BlockNode here"
        );
    }

    // -- concat_attributed_string, against its sibling -----------------------------

    #[test]
    fn joining_labels_uses_a_newline_where_the_label_builder_uses_a_space() {
        // Two functions in the same upstream file that both join text with
        // direction marks, and they disagree. Reaching for the wrong one gives
        // a label that is subtly misread.
        let joined = concat_attributed_string(
            &AttributedString::new("one"),
            None,
            &AttributedString::new("two"),
            None,
        );
        assert_eq!(joined.string(), "one\ntwo");

        let mut builder = SemanticsLabelBuilder::new();
        builder.add_part("one");
        builder.add_part("two");
        assert_eq!(builder.build(), "one two");
    }

    #[test]
    fn a_known_direction_joined_onto_an_unknown_one_counts_as_differing() {
        // The row that bites. The label builder needs *both* directions known
        // before it wraps; this one wraps whenever they differ and the other's
        // is known -- and `None != Some(Ltr)`.
        let joined = concat_attributed_string(
            &AttributedString::new("one"),
            None,
            &AttributedString::new("two"),
            Some(TextDirection::Ltr),
        );
        assert!(
            joined.string().contains(Unicode::LRE),
            "{}",
            joined.string()
        );

        let mut builder = SemanticsLabelBuilder::new();
        builder.add_part("one");
        builder.add_part_in("two", TextDirection::Ltr);
        assert!(
            !builder.build().contains(Unicode::LRE),
            "the builder leaves it bare in the same situation"
        );
    }

    #[test]
    fn a_lone_contrary_string_is_wrapped_here_and_bare_in_the_builder() {
        // Upstream wraps `other` *before* testing whether `this` is empty, so
        // an empty `this` returns the already-wrapped `other`. The builder
        // returns its single part untouched.
        let joined = concat_attributed_string(
            &AttributedString::new(""),
            Some(TextDirection::Ltr),
            &AttributedString::new("\u{645}"),
            Some(TextDirection::Rtl),
        );
        assert_eq!(
            joined.string(),
            format!("{}{}{}", Unicode::RLE, "\u{645}", Unicode::PDF)
        );

        let mut builder = SemanticsLabelBuilder::new().with_text_direction(TextDirection::Ltr);
        builder.add_part_in("\u{645}", TextDirection::Rtl);
        assert_eq!(builder.build(), "\u{645}");
    }

    #[test]
    fn joining_an_empty_other_hands_back_this_one() {
        let joined = concat_attributed_string(
            &AttributedString::new("one"),
            None,
            &AttributedString::new(""),
            Some(TextDirection::Rtl),
        );
        assert_eq!(joined.string(), "one", "no separator, no marks");
    }

    // -- The seam to the collector -------------------------------------------------

    #[test]
    fn a_config_becomes_the_properties_the_collector_already_speaks() {
        let mut config = said("Volume");
        config.value = AttributedString::new("7");
        config.actions.push(SemanticsAction::Increase);
        config.actions.push(SemanticsAction::Decrease);
        config.flags.is_slider = true;
        config.scroll_position = Some(12.0);

        let properties = config.to_properties();
        assert_eq!(properties.label, "Volume");
        assert_eq!(properties.value, "7");
        assert!(properties.flags.is_slider);
        assert_eq!(
            properties.actions,
            SemanticsAction::Increase as i32 | SemanticsAction::Decrease as i32
        );
        assert_eq!(properties.scroll_position, 12.0);
    }

    #[test]
    fn a_config_that_says_nothing_about_scrolling_reports_the_not_an_answer() {
        // `NaN` is what this crate's `SemanticsProperties` uses for "does not
        // scroll", so an absent position has to become one rather than a zero
        // that reads as "scrolled to the top".
        let properties = said("Button").to_properties();
        assert!(properties.scroll_position.is_nan());
        assert!(properties.scroll_extent_max.is_nan());
    }

    // -- ChildSemanticsConfigurationsResult ---------------------------------------

    fn config(label: &str) -> Rc<SemanticsConfiguration> {
        Rc::new(said(label))
    }

    #[test]
    fn a_builder_keeps_the_two_arrangements_apart() {
        let mut builder = ChildSemanticsConfigurationsResultBuilder::new();
        let title = config("Title");
        let subtitle = config("Subtitle");
        let badge = config("3 unread");

        builder.mark_as_merge_up(Rc::clone(&title));
        builder.mark_as_merge_up(Rc::clone(&subtitle));
        builder.mark_as_sibling_merge_group(vec![Rc::clone(&badge)]);

        let result = builder.build();
        assert_eq!(result.merge_up.len(), 2);
        assert_eq!(result.sibling_merge_groups.len(), 1);
        assert_eq!(result.sibling_merge_groups[0].len(), 1);
    }

    #[test]
    fn all_walks_the_merge_ups_first_then_each_group() {
        let mut builder = ChildSemanticsConfigurationsResultBuilder::new();
        builder.mark_as_merge_up(config("up"));
        builder.mark_as_sibling_merge_group(vec![config("a"), config("b")]);
        builder.mark_as_sibling_merge_group(vec![config("c")]);

        let all = builder.build().all();
        let labels: Vec<&str> = all.iter().map(|c| c.label.string()).collect();
        assert_eq!(labels, vec!["up", "a", "b", "c"]);
    }

    #[test]
    fn an_empty_result_names_nothing() {
        let result = ChildSemanticsConfigurationsResultBuilder::new().build();
        assert!(result.all().is_empty());
        assert!(result.merge_up.is_empty());
        assert!(result.sibling_merge_groups.is_empty());
    }

    #[test]
    fn two_children_that_say_the_same_thing_are_still_two_children() {
        // The identity rule. Upstream's duplicate check is a set under Dart's
        // default identity equality -- the same *object* twice. Two configs
        // that describe themselves alike are two separate children and both
        // belong in the list.
        let mut builder = ChildSemanticsConfigurationsResultBuilder::new();
        builder.mark_as_merge_up(config("Delete"));
        builder.mark_as_merge_up(config("Delete"));

        let result = builder.build();
        assert_eq!(result.merge_up.len(), 2, "two rows, both called Delete");
    }

    #[test]
    fn the_same_config_marked_twice_is_the_thing_being_guarded_against() {
        // Checked through `is_marked` rather than by tripping the assert, since
        // a debug assertion cannot be observed without unwinding.
        let mut builder = ChildSemanticsConfigurationsResultBuilder::new();
        let shared = config("Row");
        builder.mark_as_merge_up(Rc::clone(&shared));

        assert!(builder.is_marked(&shared), "already spoken for");
        assert!(
            !builder.is_marked(&config("Row")),
            "and a different config that reads the same is not"
        );
    }

    #[test]
    fn a_config_in_a_sibling_group_counts_as_marked_too() {
        // The assert spans both lists: upstream's message names them together.
        let mut builder = ChildSemanticsConfigurationsResultBuilder::new();
        let shared = config("Badge");
        builder.mark_as_sibling_merge_group(vec![Rc::clone(&shared)]);
        assert!(builder.is_marked(&shared));
    }

    #[test]
    fn a_config_marked_in_two_places_is_caught() {
        // The duplicate check itself, exercised directly -- `build`'s assert
        // runs on exactly this list.
        let shared = config("Row");
        let other = config("Other");
        assert!(!has_duplicate(&[Rc::clone(&shared), Rc::clone(&other)]));
        assert!(has_duplicate(&[
            Rc::clone(&shared),
            Rc::clone(&other),
            Rc::clone(&shared)
        ]));
    }

    #[test]
    fn the_duplicate_check_compares_handles_and_not_contents() {
        // Two configs built from the same text are different objects, so the
        // check must not fire on them.
        assert!(!has_duplicate(&[config("same"), config("same")]));

        let shared = config("same");
        assert!(has_duplicate(&[Rc::clone(&shared), shared]));
    }

    #[test]
    fn a_group_may_hold_configs_that_will_not_actually_merge() {
        // Neither list is a promise: a config in one that conflicts with the
        // others is pulled back out by the walk and gets a node of its own.
        // The builder does not check, and upstream's does not either -- it is
        // `is_compatible_with` that decides, later.
        let mut one = said("Send");
        one.flags.is_button = true;
        let mut two = said("Cancel");
        two.flags.is_button = true;
        assert!(!one.is_compatible_with(Some(&two)), "these cannot merge");

        let mut builder = ChildSemanticsConfigurationsResultBuilder::new();
        builder.mark_as_sibling_merge_group(vec![Rc::new(one), Rc::new(two)]);
        assert_eq!(
            builder.build().sibling_merge_groups[0].len(),
            2,
            "grouped anyway; the walk sorts it out"
        );
    }

    // -- SemanticsData: the assertions are the content -----------------------------

    fn snapshot() -> SemanticsData {
        SemanticsData {
            text_direction: Some(TextDirection::Ltr),
            ..SemanticsData::default()
        }
    }

    #[test]
    fn an_empty_snapshot_needs_no_direction() {
        // Every one of upstream's six assertions is "empty, or a direction".
        // Nothing to read means nothing to read in a direction.
        assert_eq!(SemanticsData::default().check(), None);
    }

    #[test]
    fn text_without_a_direction_is_caught_for_every_field_upstream_names() {
        // Six fields, six assertions. The reason is downstream: with no
        // direction to hand over, a reader guesses from the characters, which
        // is right for text that is all one script and silently wrong for
        // everything else.
        let cases: Vec<(&str, fn(&mut SemanticsData))> = vec![
            ("tooltip", |d| d.tooltip = "Create".to_string()),
            ("label", |d| d.label = AttributedString::new("Send")),
            ("value", |d| d.value = AttributedString::new("7")),
            ("decreasedValue", |d| {
                d.decreased_value = AttributedString::new("6")
            }),
            ("increasedValue", |d| {
                d.increased_value = AttributedString::new("8")
            }),
            ("hint", |d| d.hint = AttributedString::new("double tap")),
        ];
        for (name, set) in cases {
            let mut data = SemanticsData::default();
            set(&mut data);
            let complaint = data
                .check()
                .unwrap_or_else(|| panic!("{name} went unchecked"));
            assert!(complaint.contains(name), "{complaint}");
            assert!(complaint.contains("null textDirection"), "{complaint}");

            // And the same field with a direction is fine.
            let mut with_direction = data.clone();
            with_direction.text_direction = Some(TextDirection::Ltr);
            assert_eq!(with_direction.check(), None, "{name}");
        }
    }

    #[test]
    fn a_heading_level_runs_from_zero_to_six() {
        // Zero is "not a heading"; one to six is what `aria-level` and the
        // platform bridges accept.
        for level in 0..=6u8 {
            let data = SemanticsData {
                heading_level: level,
                ..snapshot()
            };
            assert_eq!(data.check(), None, "level {level}");
        }
        let too_deep = SemanticsData {
            heading_level: 7,
            ..snapshot()
        };
        assert!(too_deep.check().is_some());
    }

    #[test]
    fn check_reports_the_first_thing_wrong_and_not_a_list() {
        // Upstream's constructor throws on the first assertion that trips, so a
        // snapshot with two problems reports the earlier one.
        let data = SemanticsData {
            tooltip: "Create".to_string(),
            label: AttributedString::new("Send"),
            ..SemanticsData::default()
        };
        let complaint = data.check().expect("two problems, one report");
        assert!(complaint.contains("tooltip"), "{complaint}");
    }

    #[test]
    fn actions_are_a_bit_set_because_that_is_what_goes_over_the_wire() {
        let data = SemanticsData {
            actions: SemanticsAction::Tap as i32 | SemanticsAction::Increase as i32,
            ..snapshot()
        };
        assert!(data.has_action(SemanticsAction::Tap));
        assert!(data.has_action(SemanticsAction::Increase));
        assert!(!data.has_action(SemanticsAction::Decrease));
    }

    #[test]
    fn a_tag_is_recognised_by_identity_and_not_by_name() {
        let mine = SemanticsTag::new("row");
        let theirs = SemanticsTag::new("row");
        let data = SemanticsData {
            tags: vec![mine.clone()],
            ..snapshot()
        };
        assert!(data.is_tagged(&mine));
        assert!(
            !data.is_tagged(&theirs),
            "same word, different tag -- two subsystems must not collide"
        );
    }

    #[test]
    fn a_configuration_becomes_a_snapshot_with_the_rectangle_the_walk_found() {
        let mut config = said("Volume");
        config.value = AttributedString::new("7");
        config.text_direction = Some(TextDirection::Ltr);
        config.actions.push(SemanticsAction::Increase);
        config.scroll_index = Some(3);

        let rect = Rect::ltrb(10.0, 20.0, 110.0, 60.0);
        let data = SemanticsData::from_configuration(&config, rect);
        assert_eq!(data.label.string(), "Volume");
        assert_eq!(data.value.string(), "7");
        assert!(data.has_action(SemanticsAction::Increase));
        assert_eq!(data.scroll_index, Some(3));
        assert_eq!(data.rect, rect);
        assert_eq!(data.check(), None, "and it is a sound snapshot");
    }

    #[test]
    fn a_configs_child_tags_do_not_land_on_its_own_snapshot() {
        // `tagsForChildren` goes on the children, not on the node that named
        // them -- so the node's own snapshot starts untagged.
        let mut config = said("List");
        config.text_direction = Some(TextDirection::Ltr);
        config.add_tag_for_children(SemanticsTag::new("row"));

        let data = SemanticsData::from_configuration(&config, Rect::ltrb(0.0, 0.0, 1.0, 1.0));
        assert!(data.tags.is_empty());
        assert_eq!(config.tags_for_children.len(), 1, "the config still has it");
    }

    #[test]
    fn a_config_with_text_and_no_direction_makes_a_snapshot_that_fails_its_check() {
        // The seam works both ways: the config does not require a direction,
        // and the snapshot does. That is where the requirement is enforced,
        // because it is the snapshot the platform is handed.
        let config = said("Send");
        assert!(config.text_direction.is_none());
        let data = SemanticsData::from_configuration(&config, Rect::ltrb(0.0, 0.0, 1.0, 1.0));
        assert!(data.check().is_some());
    }

    // -- SemanticsOwner ------------------------------------------------------------

    fn node_at(id: i32, rect: (f32, f32, f32, f32), children: Vec<i32>) -> SemanticsNode {
        SemanticsNode {
            id,
            properties: SemanticsProperties::label(""),
            left: rect.0,
            top: rect.1,
            right: rect.2,
            bottom: rect.3,
            children,
            is_merged_into_parent: false,
            index_in_parent: None,
        }
    }

    #[test]
    fn an_owner_answers_for_its_root_and_by_id() {
        let owner = SemanticsOwner::new(vec![
            node_at(ROOT_ID, (0.0, 0.0, 800.0, 600.0), vec![7]),
            node_at(7, (0.0, 0.0, 100.0, 40.0), vec![]),
        ]);
        assert_eq!(owner.root().map(|n| n.id), Some(ROOT_ID));
        assert_eq!(owner.node(7).map(|n| n.id), Some(7));
        assert!(owner.node(99).is_none());
    }

    #[test]
    fn an_empty_owner_has_no_root_and_nothing_to_complain_about() {
        let owner = SemanticsOwner::new(Vec::new());
        assert!(owner.root().is_none());
        assert!(owner.invisible_nodes().is_empty());
    }

    #[test]
    fn a_node_with_no_size_is_a_node_the_reader_cannot_land_on() {
        // Upstream asserts against these before every send. What goes wrong is
        // that the reader hears something announced and there is nothing on the
        // glass -- worse than the thing being absent, because they now believe
        // they missed it.
        let owner = SemanticsOwner::new(vec![
            node_at(ROOT_ID, (0.0, 0.0, 800.0, 600.0), vec![7]),
            node_at(7, (10.0, 10.0, 10.0, 10.0), vec![]),
        ]);
        assert_eq!(owner.invisible_nodes(), vec![7]);
    }

    #[test]
    fn either_extent_collapsing_is_enough() {
        // `dart:ui`'s `Rect.isEmpty` is `left >= right || top >= bottom`. A
        // node one pixel wide and zero tall is exactly as unreachable as one
        // that is zero by zero, and an `&&` here would let it through.
        for rect in [
            (10.0, 10.0, 11.0, 10.0),
            (10.0, 10.0, 10.0, 11.0),
            (10.0, 10.0, 9.0, 20.0),
        ] {
            let owner = SemanticsOwner::new(vec![
                node_at(ROOT_ID, (0.0, 0.0, 800.0, 600.0), vec![7]),
                node_at(7, rect, vec![]),
            ]);
            assert_eq!(owner.invisible_nodes(), vec![7], "{rect:?}");
        }
    }

    #[test]
    fn a_root_with_no_children_is_allowed_to_be_invisible() {
        // An application that has not laid out yet: zero-size root, nothing
        // under it. A state to pass through, not a bug.
        let owner = SemanticsOwner::new(vec![node_at(ROOT_ID, (0.0, 0.0, 0.0, 0.0), vec![])]);
        assert!(owner.invisible_nodes().is_empty());
    }

    #[test]
    fn a_root_with_children_is_not() {
        // Something is claiming to be on screen underneath a root that is not.
        let owner = SemanticsOwner::new(vec![
            node_at(ROOT_ID, (0.0, 0.0, 0.0, 0.0), vec![7]),
            node_at(7, (0.0, 0.0, 100.0, 40.0), vec![]),
        ]);
        assert_eq!(owner.invisible_nodes(), vec![ROOT_ID]);
    }

    #[test]
    fn the_walk_stops_at_the_first_invisible_node_on_a_branch() {
        // Upstream adds it and does not descend. Everything under an invisible
        // node is invisible for the same reason, and reporting the lot would
        // bury the one that has to be fixed.
        let owner = SemanticsOwner::new(vec![
            node_at(ROOT_ID, (0.0, 0.0, 800.0, 600.0), vec![7]),
            node_at(7, (0.0, 0.0, 0.0, 0.0), vec![8]),
            node_at(8, (0.0, 0.0, 0.0, 0.0), vec![]),
        ]);
        assert_eq!(owner.invisible_nodes(), vec![7], "not 7 and 8");
    }

    #[test]
    fn a_sound_tree_reports_nothing() {
        let owner = SemanticsOwner::new(vec![
            node_at(ROOT_ID, (0.0, 0.0, 800.0, 600.0), vec![7, 8]),
            node_at(7, (0.0, 0.0, 100.0, 40.0), vec![9]),
            node_at(8, (0.0, 50.0, 100.0, 90.0), vec![]),
            node_at(9, (5.0, 5.0, 95.0, 35.0), vec![]),
        ]);
        assert!(owner.invisible_nodes().is_empty());
    }

    #[test]
    fn every_bad_branch_is_reported_and_not_only_the_first() {
        let owner = SemanticsOwner::new(vec![
            node_at(ROOT_ID, (0.0, 0.0, 800.0, 600.0), vec![7, 8]),
            node_at(7, (0.0, 0.0, 0.0, 0.0), vec![]),
            node_at(8, (0.0, 0.0, 0.0, 0.0), vec![]),
        ]);
        assert_eq!(owner.invisible_nodes(), vec![7, 8]);
    }

    #[test]
    fn disposing_an_owner_leaves_it_holding_nothing() {
        let mut owner = SemanticsOwner::new(vec![node_at(ROOT_ID, (0.0, 0.0, 8.0, 6.0), vec![])]);
        assert!(owner.root().is_some());
        owner.dispose();
        assert!(owner.root().is_none());
        assert!(owner.nodes().is_empty());
    }
}

// -- The tool on the other end of the wire -------------------------------------

/// What a service extension answered with. Upstream returns a
/// `Map<String, Object?>`, and these are the shapes it puts in it.
#[derive(Clone, Debug, PartialEq)]
pub enum InspectorResponse {
    /// Upstream's `{}` -- did what you asked, nothing to say.
    Done,
    /// Upstream's `{'error': ...}`.
    Error(String),
    /// Upstream's `{'error': ..., 'needsFrame': true}`.
    ///
    /// A separate case because it means something different to the caller: not
    /// "this cannot work" but "ask again after a frame". Upstream also asks for
    /// that frame itself before answering, so the retry has something to find.
    NeedsFrame(String),
    /// Upstream's `{'data': {id: node, ...}}`.
    Tree(Vec<(i32, SemanticsNode)>),
}

/// Upstream `AccessibilityInspector`: the semantics tree, on demand, for a tool
/// outside the process.
///
/// # It is a singleton because what it holds must be
///
/// The handle it keeps is what *keeps semantics turned on*. Semantics are off
/// until something asks, because building the tree costs work on every frame; a
/// tool that wants to inspect them asks once and the tree stays alive until it
/// says it is done. Two inspectors would mean two handles, and one of them
/// turning semantics off while the other was still reading.
///
/// The three extensions are the whole interface, and they are three rather than
/// one because turning semantics on and reading the tree are different moments:
/// a tool holds the first open across many of the second.
#[derive(Default)]
pub struct AccessibilityInspector {
    handle: RefCell<Option<SemanticsHandle>>,
}

thread_local! {
    static INSPECTOR: AccessibilityInspector = AccessibilityInspector::default();
}

impl AccessibilityInspector {
    /// Upstream's `AccessibilityInspector.instance`, with its private
    /// constructor: there is one, and a caller cannot make another.
    pub fn with_instance<R>(body: impl FnOnce(&AccessibilityInspector) -> R) -> R {
        INSPECTOR.with(body)
    }

    /// Upstream's `_enableSemantics`.
    ///
    /// Upstream's `??=` is there to stop a tool that asks twice from taking a
    /// second handle and leaking the first, because upstream's handle is
    /// released by hand. **Here it cannot be observed**: [`SemanticsHandle`]
    /// releases on drop, so assigning over one would release it in the same
    /// breath and the count would be unchanged. Kept because it is upstream's
    /// line and says what it means; the invariant that makes it unnecessary is
    /// the one under test.
    pub fn enable_semantics(&self) -> InspectorResponse {
        let mut handle = self.handle.borrow_mut();
        if handle.is_none() {
            *handle = Some(SemanticsBinding::ensure_semantics());
        }
        InspectorResponse::Done
    }

    /// Upstream's `_disposeSemantics`, which is `resetAllState`.
    pub fn dispose_semantics(&self) -> InspectorResponse {
        self.reset_all_state();
        InspectorResponse::Done
    }

    /// Upstream's `resetAllState`: drop the handle, which is what lets
    /// semantics switch off again once nothing else wants them.
    pub fn reset_all_state(&self) {
        if let Some(mut handle) = self.handle.borrow_mut().take() {
            handle.dispose();
        }
    }

    pub fn is_holding_semantics(&self) -> bool {
        self.handle.borrow().is_some()
    }

    /// Upstream's `_getSemanticsTree`.
    ///
    /// The three failures are distinct, and upstream keeps them apart:
    ///
    /// * semantics are not on at all -- the tool has to enable them first;
    /// * they are on but nothing owns a tree -- nothing to read;
    /// * there is an owner but no root **yet**, which is not a failure but a
    ///   timing problem. Upstream asks for a frame and tells the caller to come
    ///   back, rather than reporting an error a tool would give up on.
    ///
    /// The walk is a stack -- upstream's `removeLast` -- with a visited set, so
    /// a tree that is a graph does not loop.
    ///
    /// **Upstream pushes each node's children twice**, once in traversal order
    /// and once in inverse hit-test order. Both orders are over the same
    /// children, so with the visited set in place the second pass cannot reach
    /// a node the first did not; what it changes is the order things come off
    /// the stack, and the result is keyed by id, so it does not change that
    /// either. This port pushes once. Recorded rather than copied, because
    /// copying it would look like it was doing something.
    pub fn semantics_tree(&self, owner: Option<&SemanticsOwner>) -> InspectorResponse {
        if !enabled() {
            return InspectorResponse::Error("Semantics not enabled.".to_string());
        }
        let Some(owner) = owner else {
            return InspectorResponse::Error(
                "No PipelineOwner with SemanticsOwner found".to_string(),
            );
        };
        let Some(root) = owner.root() else {
            return InspectorResponse::NeedsFrame("rootSemanticsNode is null".to_string());
        };

        let mut nodes: Vec<(i32, SemanticsNode)> = Vec::new();
        let mut visited: Vec<i32> = Vec::new();
        let mut stack: Vec<i32> = vec![root.id];
        while let Some(id) = stack.pop() {
            if visited.contains(&id) {
                continue;
            }
            let Some(node) = owner.node(id) else {
                continue;
            };
            visited.push(id);
            nodes.push((id, node.clone()));
            for child in node.children.iter().rev() {
                if !visited.contains(child) {
                    stack.push(*child);
                }
            }
        }
        InspectorResponse::Tree(nodes)
    }
}

#[cfg(test)]
mod inspector_tests {
    use super::*;

    fn node(id: i32, children: Vec<i32>) -> SemanticsNode {
        SemanticsNode {
            id,
            properties: SemanticsProperties::label(""),
            left: 0.0,
            top: 0.0,
            right: 10.0,
            bottom: 10.0,
            children,
            is_merged_into_parent: false,
            index_in_parent: None,
        }
    }

    #[test]
    fn the_handle_is_taken_once_however_often_it_is_asked_for() {
        // Upstream's `??=`. A tool that reconnects must not leak a handle, or
        // semantics stay on for the life of the process.
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            assert!(!inspector.is_holding_semantics());

            assert_eq!(inspector.enable_semantics(), InspectorResponse::Done);
            assert!(inspector.is_holding_semantics());
            assert!(enabled(), "and semantics are on");

            inspector.enable_semantics();
            inspector.enable_semantics();
            assert_eq!(inspector.dispose_semantics(), InspectorResponse::Done);
            assert!(
                !inspector.is_holding_semantics(),
                "one dispose is enough, because only one handle was taken"
            );
            assert!(!enabled(), "and semantics go off again");
        });
    }

    /// Something that toggles semantics from its own drop -- which is what an
    /// inspector handle in a thread-local is.
    struct TogglesOnDrop;

    impl Drop for TogglesOnDrop {
        fn drop(&mut self) {
            apply_enabled(false);
        }
    }

    thread_local! {
        static TOGGLES: TogglesOnDrop = const { TogglesOnDrop };
    }

    #[test]
    fn toggling_from_a_drop_survives_a_half_destroyed_thread() {
        // Thread-locals are destroyed in reverse order of initialisation, so
        // touching the collector, then this dropper, then the listener list
        // guarantees the drop runs with the collector still alive and the
        // listener list already gone. Both halves have to be able to cope: a
        // guard on the collector alone would pass and then fall into the dead
        // list.
        let thread = std::thread::spawn(|| {
            let _ = enabled();
            TOGGLES.with(|_| {});
            SemanticsBinding::add_enabled_listener(|_| {});
        });
        assert!(thread.join().is_ok());
    }

    #[test]
    fn a_handle_outliving_its_thread_does_not_abort_the_process() {
        // The inspector's handle is released from a thread-local's drop, which
        // runs during teardown -- after the collector it clears may already be
        // gone. Reading that with `with` panics, and a panic inside a drop
        // inside teardown aborts the whole process rather than failing a test.
        let thread = std::thread::spawn(|| {
            AccessibilityInspector::with_instance(|inspector| {
                inspector.enable_semantics();
            });
            // Deliberately left held.
        });
        assert!(thread.join().is_ok());
    }

    #[test]
    fn a_handle_releases_itself_when_it_is_dropped() {
        // Which is what makes upstream's `??=` unobservable here: assigning
        // over a handle would release it in the same breath.
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            assert!(!enabled());
            {
                let _handle = SemanticsBinding::ensure_semantics();
                assert!(enabled());
            }
            assert!(!enabled(), "released without anyone calling dispose");
        });
    }

    #[test]
    fn resetting_twice_is_not_an_error() {
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            inspector.reset_all_state();
            assert!(!inspector.is_holding_semantics());
        });
    }

    #[test]
    fn semantics_off_is_told_apart_from_nothing_to_read() {
        // Three different failures, and a tool does different things about
        // each -- which is why upstream keeps them apart.
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            let owner = SemanticsOwner::new(vec![node(ROOT_ID, Vec::new())]);
            assert_eq!(
                inspector.semantics_tree(Some(&owner)),
                InspectorResponse::Error("Semantics not enabled.".to_string()),
                "the tool has to enable them first"
            );

            inspector.enable_semantics();
            assert_eq!(
                inspector.semantics_tree(None),
                InspectorResponse::Error("No PipelineOwner with SemanticsOwner found".to_string()),
                "on, but nothing owns a tree"
            );
            inspector.reset_all_state();
        });
    }

    #[test]
    fn an_owner_with_no_root_yet_is_a_timing_problem_and_not_a_failure() {
        // Upstream asks for a frame and tells the caller to come back, rather
        // than reporting an error a tool would give up on.
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            inspector.enable_semantics();
            let empty = SemanticsOwner::new(Vec::new());
            assert_eq!(
                inspector.semantics_tree(Some(&empty)),
                InspectorResponse::NeedsFrame("rootSemanticsNode is null".to_string())
            );
            inspector.reset_all_state();
        });
    }

    #[test]
    fn the_whole_tree_comes_back_once_each() {
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            inspector.enable_semantics();
            let owner = SemanticsOwner::new(vec![
                node(ROOT_ID, vec![2, 3]),
                node(2, vec![4]),
                node(3, Vec::new()),
                node(4, Vec::new()),
            ]);
            let InspectorResponse::Tree(nodes) = inspector.semantics_tree(Some(&owner)) else {
                panic!("a tree");
            };
            let mut ids: Vec<i32> = nodes.iter().map(|(id, _)| *id).collect();
            ids.sort_unstable();
            assert_eq!(ids, vec![ROOT_ID, 2, 3, 4]);
            inspector.reset_all_state();
        });
    }

    #[test]
    fn a_tree_that_is_a_graph_does_not_loop() {
        // The visited set is what makes the walk terminate, and two parents
        // naming one child is enough to need it.
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            inspector.enable_semantics();
            // Ordered so that one node is on the stack *twice before it is
            // popped* -- root pushes 2, then pushes 3, then 3 pushes 2 again.
            // The check when a node comes off the stack is the only thing that
            // stops it being recorded twice; the check before pushing cannot
            // see a push that has already happened.
            let owner = SemanticsOwner::new(vec![
                node(ROOT_ID, vec![3, 2]),
                node(2, Vec::new()),
                node(3, vec![2]),
            ]);
            let InspectorResponse::Tree(nodes) = inspector.semantics_tree(Some(&owner)) else {
                panic!("a tree");
            };
            let mut ids: Vec<i32> = nodes.iter().map(|(id, _)| *id).collect();
            ids.sort_unstable();
            assert_eq!(ids, vec![ROOT_ID, 2, 3], "each node once");
            inspector.reset_all_state();
        });
    }

    #[test]
    fn a_child_the_owner_does_not_have_is_skipped() {
        // A stale child id after a node was removed: the walk must not stop,
        // because the rest of the tree is still worth reading.
        AccessibilityInspector::with_instance(|inspector| {
            inspector.reset_all_state();
            inspector.enable_semantics();
            let owner = SemanticsOwner::new(vec![node(ROOT_ID, vec![99, 2]), node(2, Vec::new())]);
            let InspectorResponse::Tree(nodes) = inspector.semantics_tree(Some(&owner)) else {
                panic!("a tree");
            };
            let ids: Vec<i32> = nodes.iter().map(|(id, _)| *id).collect();
            assert!(ids.contains(&2));
            assert!(!ids.contains(&99));
            inspector.reset_all_state();
        });
    }
}

#[cfg(test)]
mod placeholder_tag_tests {
    use super::*;

    #[test]
    fn two_tags_with_the_same_index_are_the_same_tag() {
        // Which is the opposite of the base rule, and deliberately so: the
        // paragraph makes these fresh on every layout, and the node from this
        // frame has to be recognised as the node from the last one.
        assert_eq!(
            PlaceholderSpanIndexSemanticsTag::new(3).to_tag(),
            PlaceholderSpanIndexSemanticsTag::new(3).to_tag()
        );
        assert_ne!(
            PlaceholderSpanIndexSemanticsTag::new(3).to_tag(),
            PlaceholderSpanIndexSemanticsTag::new(4).to_tag()
        );
    }

    #[test]
    fn an_ordinary_tag_is_still_compared_by_identity() {
        // The base rule the one above is an exception to.
        assert_ne!(SemanticsTag::new("scrolled"), SemanticsTag::new("scrolled"));
    }

    #[test]
    fn a_derived_id_can_never_collide_with_an_allocated_one() {
        let mut allocated = SemanticsTag::new("a");
        for _ in 0..100 {
            allocated = SemanticsTag::new("a");
        }
        assert!(allocated.id() < PlaceholderSpanIndexSemanticsTag::new(0).to_tag().id());
    }

    #[test]
    fn the_index_reads_back_out() {
        let tag = PlaceholderSpanIndexSemanticsTag::new(7).to_tag();
        assert_eq!(PlaceholderSpanIndexSemanticsTag::index_of(&tag), Some(7));
        assert_eq!(
            PlaceholderSpanIndexSemanticsTag::index_of(&SemanticsTag::new("other")),
            None,
            "and an ordinary tag is not one of these"
        );
    }

    #[test]
    fn the_name_is_what_a_dump_shows() {
        assert_eq!(
            PlaceholderSpanIndexSemanticsTag::new(2).to_tag().name(),
            "PlaceholderSpanIndexSemanticsTag(2)"
        );
    }
}

#[cfg(test)]
mod merge_first_wins_tests {
    use super::*;

    /// A configuration with every scroll field set, so each `or` has something
    /// on both sides and its direction is visible.
    ///
    /// `has_been_annotated` matters: `absorb` returns early without it, so a
    /// child built without it is absorbed as nothing at all -- and a test that
    /// then checked the parent's own values would pass whatever `absorb` did.
    /// The first draft of these tests did exactly that.
    fn filled(base: f32) -> SemanticsConfiguration {
        let mut config = SemanticsConfiguration::new();
        config.has_been_annotated = true;
        config.scroll_position = Some(base);
        config.scroll_extent_max = Some(base + 1.0);
        config.scroll_extent_min = Some(base + 2.0);
        config.scroll_index = Some(base as i32 + 3);
        config.scroll_child_count = Some(base as i32 + 4);
        config.index_in_parent = Some(base as i32 + 5);
        config
    }

    #[test]
    fn the_parent_wins_every_scroll_field_and_not_just_the_indexed_one() {
        // First-wins, one slot at a time: the parent asked first. Two of these
        // six were tested and four were not, because each test set the field on
        // one side only -- which shows that *something* comes through, not
        // which side it came from.
        let mut parent = filled(100.0);
        parent.absorb(&filled(200.0));

        assert_eq!(parent.scroll_position, Some(100.0));
        assert_eq!(parent.scroll_extent_max, Some(101.0));
        assert_eq!(parent.scroll_extent_min, Some(102.0));
        assert_eq!(parent.scroll_index, Some(103));
        assert_eq!(parent.scroll_child_count, Some(104));
        assert_eq!(parent.index_in_parent, Some(105));
    }

    #[test]
    fn a_field_only_the_child_has_still_comes_up() {
        // The other half of "first wins": an empty slot is filled by whoever
        // has something for it.
        let mut parent = SemanticsConfiguration::new();
        parent.scroll_position = Some(1.0);
        parent.absorb(&filled(200.0));

        assert_eq!(parent.scroll_position, Some(1.0), "its own");
        assert_eq!(parent.scroll_extent_max, Some(201.0), "and the child's");
        assert_eq!(parent.index_in_parent, Some(205));
    }
}

#[cfg(test)]
mod semantics_node_visibility_tests {
    use super::*;

    fn node(width: f32, height: f32) -> SemanticsNode {
        SemanticsNode {
            id: 1,
            properties: SemanticsProperties::default(),
            left: 0.0,
            top: 0.0,
            right: width,
            bottom: height,
            children: Vec::new(),
            is_merged_into_parent: false,
            index_in_parent: None,
        }
    }

    #[test]
    fn a_node_with_no_size_is_invisible_and_may_be_dropped() {
        // Which is what the predicate is for: upstream says an invisible node
        // "can be safely dropped from the semantic tree without losing
        // semantic information".
        assert!(node(0.0, 10.0).is_invisible());
        assert!(node(10.0, 0.0).is_invisible());
        assert!(node(0.0, 0.0).is_invisible());
        assert!(!node(10.0, 10.0).is_invisible());
    }

    #[test]
    fn but_a_merged_node_is_never_invisible_however_small_it_is() {
        // A merged node has no geometry of its own to be judged by -- its
        // label is read as part of its parent's -- so an empty rect says
        // nothing about whether it is on screen, and dropping it would lose
        // the words.
        let mut merged = node(0.0, 0.0);
        merged.is_merged_into_parent = true;
        assert!(!merged.is_invisible());

        // The guard is what does it, not the size: the same node unmerged is
        // invisible.
        merged.is_merged_into_parent = false;
        assert!(merged.is_invisible());
    }

    #[test]
    fn and_merging_does_not_make_a_sized_node_anything_else() {
        // The guard only ever turns the answer off, never on.
        let mut merged = node(10.0, 10.0);
        merged.is_merged_into_parent = true;
        assert!(!merged.is_invisible());
        assert!(!node(10.0, 10.0).is_invisible());
    }

    #[test]
    fn an_index_keeps_the_position_it_had_before_its_siblings_were_dropped() {
        // Upstream's own example: five children, the first two not visible,
        // and the last of the three that survive still has index 4.
        assert_eq!(SemanticsNode::indices_in_parent(&[2, 3, 4]), vec![2, 3, 4]);
        assert_ne!(
            SemanticsNode::indices_in_parent(&[2, 3, 4]),
            vec![0, 1, 2],
            "renumbering would say 'item 3 of 3' and shorten the list"
        );
    }

    #[test]
    fn which_is_only_visible_where_something_was_dropped() {
        // With nothing dropped the two rules agree, so a test that only
        // checked this case would pass against either of them.
        assert_eq!(
            SemanticsNode::indices_in_parent(&[0, 1, 2]),
            vec![0, 1, 2],
            "the case that proves nothing"
        );
        assert_eq!(SemanticsNode::indices_in_parent(&[]), Vec::<i32>::new());
    }

    #[test]
    fn a_gap_in_the_middle_survives_too() {
        // Not just a dropped prefix: a scrollable can drop from anywhere.
        assert_eq!(SemanticsNode::indices_in_parent(&[0, 3, 7]), vec![0, 3, 7]);
    }
}

#[cfg(test)]
mod dump_order_tests {
    use super::DebugSemanticsDumpOrder;

    #[test]
    fn the_two_orders_are_reverses_of_one_another() {
        // Painting and hit-testing are reverses, so a dump in the order
        // children are offered a touch is a dump in the reverse of the order
        // a reader navigates them.
        let children = [1, 2, 3, 4];
        let traversal = DebugSemanticsDumpOrder::TraversalOrder.children_of(&children);
        let mut hit_test = DebugSemanticsDumpOrder::InverseHitTest.children_of(&children);
        hit_test.reverse();
        assert_eq!(traversal, hit_test);
    }

    #[test]
    fn traversal_order_is_the_order_they_are_kept_in() {
        let children = [7, 8, 9];
        assert_eq!(
            DebugSemanticsDumpOrder::TraversalOrder.children_of(&children),
            vec![7, 8, 9]
        );
        assert_eq!(
            DebugSemanticsDumpOrder::InverseHitTest.children_of(&children),
            vec![9, 8, 7]
        );
    }

    #[test]
    fn the_last_child_is_offered_the_touch_first() {
        // Later children are painted over earlier ones, so the last is on top
        // and has to be asked first.
        let children = [1, 2, 3];
        let offered = DebugSemanticsDumpOrder::InverseHitTest.children_of(&children);
        assert_eq!(offered.first(), children.last());
        assert_eq!(
            DebugSemanticsDumpOrder::TraversalOrder
                .children_of(&children)
                .first(),
            children.first()
        );
    }

    #[test]
    fn and_neither_order_loses_or_repeats_a_child() {
        let children = [4, 5, 6, 7, 8];
        for order in DebugSemanticsDumpOrder::ALL {
            let mut walked = order.children_of(&children);
            assert_eq!(walked.len(), children.len(), "{order:?}");
            walked.sort_unstable();
            assert_eq!(walked, children.to_vec(), "{order:?}");
        }
    }

    #[test]
    fn a_single_child_reads_the_same_either_way() {
        // Which is why the tests above use four and five: with one child the
        // two orders agree and would prove nothing.
        for order in DebugSemanticsDumpOrder::ALL {
            assert_eq!(order.children_of(&[42]), vec![42]);
            assert!(order.children_of(&[]).is_empty());
        }
        assert_ne!(
            DebugSemanticsDumpOrder::TraversalOrder.children_of(&[1, 2]),
            DebugSemanticsDumpOrder::InverseHitTest.children_of(&[1, 2])
        );
    }

    #[test]
    fn a_dump_reads_in_navigation_order_unless_told_otherwise() {
        assert_eq!(
            DebugSemanticsDumpOrder::default(),
            DebugSemanticsDumpOrder::TraversalOrder
        );
    }
}

#[cfg(test)]
mod focus_block_tests {
    use super::AccessibilityFocusBlockType;

    #[test]
    fn merging_takes_the_stronger_of_the_two() {
        // Upstream's three ifs, said once: blockSubtree beats everything, then
        // blockNode, otherwise both were none.
        for a in AccessibilityFocusBlockType::ALL {
            for b in AccessibilityFocusBlockType::ALL {
                let merged = a.merge(b);
                assert!(merged == a || merged == b, "{a:?} {b:?}");
                assert!(merged.strength() >= a.strength());
                assert!(merged.strength() >= b.strength());
            }
        }
    }

    #[test]
    fn and_it_does_not_matter_which_node_is_asked_first() {
        // Two nodes merging is symmetric; an order-dependent answer would make
        // the result depend on which happened to be the parent.
        for a in AccessibilityFocusBlockType::ALL {
            for b in AccessibilityFocusBlockType::ALL {
                assert_eq!(a.merge(b), b.merge(a), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn not_blocking_is_the_identity_and_blocking_a_subtree_is_the_end() {
        for value in AccessibilityFocusBlockType::ALL {
            assert_eq!(value.merge(AccessibilityFocusBlockType::None), value);
            assert_eq!(
                value.merge(AccessibilityFocusBlockType::BlockSubtree),
                AccessibilityFocusBlockType::BlockSubtree,
                "{value:?} could not undo a blocked subtree"
            );
            assert_eq!(value.merge(value), value, "{value:?}");
        }
    }

    #[test]
    fn blocking_a_node_is_not_blocking_its_children() {
        // The middle rung, and the reason the type has three values rather
        // than two: a container that should not be stopped on, whose contents
        // should still be reachable, is a real thing.
        assert!(
            AccessibilityFocusBlockType::BlockNode.strength()
                < AccessibilityFocusBlockType::BlockSubtree.strength()
        );
        assert_ne!(
            AccessibilityFocusBlockType::BlockNode,
            AccessibilityFocusBlockType::BlockSubtree
        );
        // And merging the two keeps the stronger, so a subtree block anywhere
        // in a merge wins.
        assert_eq!(
            AccessibilityFocusBlockType::BlockNode.merge(AccessibilityFocusBlockType::BlockSubtree),
            AccessibilityFocusBlockType::BlockSubtree
        );
    }

    #[test]
    fn the_three_rungs_are_three_different_heights() {
        let mut strengths: Vec<u8> = AccessibilityFocusBlockType::ALL
            .iter()
            .map(|value| value.strength())
            .collect();
        strengths.sort_unstable();
        strengths.dedup();
        assert_eq!(strengths, vec![0, 1, 2]);
    }

    #[test]
    fn a_node_blocks_nothing_unless_told_to() {
        assert_eq!(
            AccessibilityFocusBlockType::default(),
            AccessibilityFocusBlockType::None
        );
        assert_eq!(AccessibilityFocusBlockType::None.strength(), 0);
    }

    // -- A radio, said the way each platform's screen reader expects ---------

    use crate::editable_text::TargetPlatform;
    use crate::semantics::SemanticsProperties;

    const HINT: &str = "Not selected";

    fn radio(selected: bool, platform: TargetPlatform) -> SemanticsProperties {
        SemanticsProperties::radio("Medium", selected, platform, HINT)
    }

    #[test]
    fn a_radio_is_checkable_and_in_a_group_on_every_platform() {
        // The two together are what a radio *is*. Checkable alone is a
        // checkbox, and a column of radios read as checkboxes says that seven
        // of them being on is a thing that could happen.
        for platform in TargetPlatform::ALL {
            for selected in [false, true] {
                let properties = radio(selected, platform);
                assert!(properties.flags.checked.is_checkable(), "{platform:?}");
                assert_eq!(
                    properties.flags.checked,
                    crate::semantics::SemanticsCheckState::of(Some(selected)),
                    "{platform:?}"
                );
                assert!(
                    properties.flags.is_in_mutually_exclusive_group,
                    "{platform:?}"
                );
            }
        }
    }

    #[test]
    fn but_only_the_apple_platforms_say_it_a_second_time_as_selected() {
        // The same fact in two properties, because the two screen readers read
        // different ones. Setting it everywhere is not neutral: TalkBack would
        // announce a radio as selected *and* checked.
        // These two loops used to make the *same* assertion --
        // `!flags.is_selected` -- about two different situations, because the
        // boolean collapsed them. An unchosen radio on iOS is "not selected";
        // a radio on Android has no opinion about being selected at all, and
        // TalkBack announcing it as both selected and checked is what that
        // silence is protecting against.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                radio(true, platform).flags.selected,
                crate::semantics::SemanticsTristate::True,
                "{platform:?}"
            );
            assert_eq!(
                radio(false, platform).flags.selected,
                crate::semantics::SemanticsTristate::False,
                "{platform:?}: not selected, which is a thing to say"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_eq!(
                radio(true, platform).flags.selected,
                crate::semantics::SemanticsTristate::None,
                "{platform:?}: checked is the whole of it here"
            );
            assert_eq!(
                radio(false, platform).flags.selected,
                crate::semantics::SemanticsTristate::None,
                "{platform:?}: and silence, not a claim of not-selected"
            );
        }
    }

    #[test]
    fn and_the_hint_belongs_to_the_radio_that_is_not_chosen() {
        // Upstream's own comment: the selected state is already announced on
        // iOS through `selected`, so a hint on a selected radio would say it
        // twice. The one that needs telling is the one that is off, where
        // silence is indistinguishable from a control that does nothing.
        assert_eq!(radio(false, TargetPlatform::IOS).hint, HINT);
        assert_eq!(
            radio(true, TargetPlatform::IOS).hint,
            "",
            "a chosen radio has already said so"
        );

        // And nowhere else at all: the hint exists to stand in for a property
        // the other platforms do not read.
        for platform in [TargetPlatform::Android, TargetPlatform::Windows] {
            for selected in [false, true] {
                assert_eq!(radio(selected, platform).hint, "", "{platform:?}");
            }
        }
    }

    #[test]
    fn a_radio_and_a_checkbox_differ_by_exactly_one_flag() {
        // Which is the point of porting the flag at all. Everything else about
        // the two is the same shape, so a reader who has both on a page can
        // only be told them apart by this.
        let radio = SemanticsProperties::radio("Medium", true, TargetPlatform::Android, HINT);
        // This said `toggle` and meant a checkbox, which is the confusion the
        // missing flag caused: the test's own name says checkbox and the
        // helper it called made a switch.
        let checkbox = SemanticsProperties::check("Remember me", Some(true));
        assert_eq!(radio.flags.checked, checkbox.flags.checked);
        assert_ne!(
            radio.flags.is_in_mutually_exclusive_group,
            checkbox.flags.is_in_mutually_exclusive_group
        );
    }

    #[test]
    fn merging_keeps_the_group_claim() {
        // `SemanticsFlags::merge` is a union, and a radio folded into a row
        // that also carries a label must not come out as a checkbox.
        let radio = SemanticsProperties::radio("Medium", false, TargetPlatform::Android, HINT);
        let plain = SemanticsProperties::label("A row");
        assert!(
            radio
                .flags
                .merge(&plain.flags)
                .is_in_mutually_exclusive_group
        );
        assert!(
            plain
                .flags
                .merge(&radio.flags)
                .is_in_mutually_exclusive_group,
            "either way round"
        );
    }
}
