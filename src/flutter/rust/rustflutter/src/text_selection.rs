//! Selecting text with a finger or a mouse -- a port of upstream's
//! `widgets/text_selection.dart`.
//!
//! The two notifiers here are the interesting half, and what makes them
//! interesting is that **they answer a question the framework cannot answer
//! synchronously**. Whether the clipboard has anything pasteable, and whether
//! the platform offers Live Text, are both round trips to the host. A toolbar
//! has to be built now and told later.
//!
//! Everything they do follows from that: an `Unknown` value that is a real
//! state rather than a missing one, an update fired when the first listener
//! arrives, and another on every return from the background -- because the
//! reader may have copied something in another application while away.
//!
//! ## Where the handles and the toolbar are
//!
//! [`TextSelectionOverlay`] and [`SelectionOverlay`] position handles and a
//! toolbar in an `Overlay`, and [`crate::selection_host`] is that overlay --
//! three entries, placed against the field through
//! [`crate::render::RenderRef::global_to_local`]. What stays here is the
//! configuration and the visibility rules, which is the half that decides
//! rather than draws. The gesture builder's own recognisers are the
//! `tap_and_drag` family, already ported.
//!
//! This paragraph used to end "which this crate does not have".

use crate::direction::TextDirection;
use crate::editable_text::TargetPlatform;
use crate::engine::Color;
use crate::gestures::PointerKind;
use crate::render::{Offset, PaintContext, Size};

/// Upstream `ClipboardStatus`.
///
/// **`Unknown` is a real third state**, not a missing answer. A paste button
/// shown while the answer is unknown would be a button that might do nothing;
/// one hidden would flicker into existence a frame later. Upstream keeps the
/// state so a caller can decide which it prefers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClipboardStatus {
    /// There is something to paste.
    Pasteable,
    /// Nobody has asked yet, or the last ask failed.
    #[default]
    Unknown,
    NotPasteable,
}

/// Upstream `LiveTextInputStatus`: whether the platform offers to read text
/// out of the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LiveTextInputStatus {
    Enabled,
    Disabled,
    #[default]
    Unknown,
}

/// Upstream `ClipboardStatusNotifier`.
pub struct ClipboardStatusNotifier {
    value: ClipboardStatus,
    listeners: usize,
    disposed: bool,
    /// How many times the host has been asked, which is what the tests below
    /// watch.
    updates: usize,
    observing: bool,
}

impl Default for ClipboardStatusNotifier {
    fn default() -> ClipboardStatusNotifier {
        ClipboardStatusNotifier::new()
    }
}

impl ClipboardStatusNotifier {
    pub fn new() -> ClipboardStatusNotifier {
        ClipboardStatusNotifier {
            value: ClipboardStatus::Unknown,
            listeners: 0,
            disposed: false,
            updates: 0,
            observing: false,
        }
    }

    pub fn value(&self) -> ClipboardStatus {
        self.value
    }

    pub fn has_listeners(&self) -> bool {
        self.listeners > 0
    }

    /// Whether this notifier is registered with the binding to hear about the
    /// application coming back to the foreground.
    pub fn is_observing(&self) -> bool {
        self.observing
    }

    pub fn updates(&self) -> usize {
        self.updates
    }

    /// Upstream's `addListener`.
    ///
    /// Two things happen only for the **first** listener: the notifier starts
    /// observing the lifecycle, and -- if the answer is still unknown -- it
    /// asks. A notifier nobody is listening to has no reason to be talking to
    /// the host at all.
    pub fn add_listener(&mut self) {
        if !self.has_listeners() {
            self.observing = true;
        }
        if self.value == ClipboardStatus::Unknown {
            self.begin_update();
        }
        self.listeners += 1;
    }

    /// Upstream's `removeListener`, which stops observing once the last
    /// listener goes -- and checks `_disposed` first, because a disposed
    /// notifier has already unregistered.
    pub fn remove_listener(&mut self) {
        self.listeners = self.listeners.saturating_sub(1);
        if !self.disposed && !self.has_listeners() {
            self.observing = false;
        }
    }

    /// Upstream's `update`, up to the point where it awaits the host.
    ///
    /// Returns whether the ask went out. Upstream checks `_disposed` **before
    /// and after** the await, and the second check is the one that matters: a
    /// notifier disposed while the host was answering must not write a value
    /// into a dead object.
    pub fn begin_update(&mut self) -> bool {
        if self.disposed {
            return false;
        }
        self.updates += 1;
        true
    }

    /// The host's answer arriving.
    pub fn complete_update(&mut self, has_strings: bool) {
        if self.disposed {
            return;
        }
        self.value = if has_strings {
            ClipboardStatus::Pasteable
        } else {
            ClipboardStatus::NotPasteable
        };
    }

    /// The host failing to answer.
    ///
    /// **The value goes back to unknown rather than staying as it was**, and
    /// upstream's comment says why: so that it will try again later. A stale
    /// `Pasteable` would leave a paste button that does nothing.
    pub fn fail_update(&mut self) {
        if self.disposed {
            return;
        }
        self.value = ClipboardStatus::Unknown;
    }

    /// Upstream's `didChangeAppLifecycleState`.
    ///
    /// Only `resumed` asks again, and it must: the reader may have copied
    /// something in another application while this one was in the background,
    /// and nothing would otherwise tell us.
    pub fn app_resumed(&mut self) {
        self.begin_update();
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
        self.observing = false;
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }
}

/// Upstream `LiveTextInputStatusNotifier`.
///
/// Nearly the same as the clipboard one, with **one deliberate difference**:
/// both its failure path and its success path return early when the value
/// would not change. Live Text availability is a property of the device and
/// almost never moves, so a notification saying it is still what it was would
/// rebuild every toolbar for nothing.
pub struct LiveTextInputStatusNotifier {
    value: LiveTextInputStatus,
    listeners: usize,
    disposed: bool,
    updates: usize,
    notifications: usize,
    observing: bool,
}

impl Default for LiveTextInputStatusNotifier {
    fn default() -> LiveTextInputStatusNotifier {
        LiveTextInputStatusNotifier::new()
    }
}

impl LiveTextInputStatusNotifier {
    pub fn new() -> LiveTextInputStatusNotifier {
        LiveTextInputStatusNotifier {
            value: LiveTextInputStatus::Unknown,
            listeners: 0,
            disposed: false,
            updates: 0,
            notifications: 0,
            observing: false,
        }
    }

    pub fn value(&self) -> LiveTextInputStatus {
        self.value
    }

    pub fn has_listeners(&self) -> bool {
        self.listeners > 0
    }

    pub fn is_observing(&self) -> bool {
        self.observing
    }

    pub fn updates(&self) -> usize {
        self.updates
    }

    /// How many times listeners were actually told.
    pub fn notifications(&self) -> usize {
        self.notifications
    }

    pub fn add_listener(&mut self) {
        if !self.has_listeners() {
            self.observing = true;
        }
        if self.value == LiveTextInputStatus::Unknown {
            self.begin_update();
        }
        self.listeners += 1;
    }

    pub fn remove_listener(&mut self) {
        self.listeners = self.listeners.saturating_sub(1);
        if !self.disposed && !self.has_listeners() {
            self.observing = false;
        }
    }

    pub fn begin_update(&mut self) -> bool {
        if self.disposed {
            return false;
        }
        self.updates += 1;
        true
    }

    /// Upstream's success path, which returns early when the answer has not
    /// changed.
    pub fn complete_update(&mut self, is_available: bool) {
        let next = if is_available {
            LiveTextInputStatus::Enabled
        } else {
            LiveTextInputStatus::Disabled
        };
        if self.disposed || next == self.value {
            return;
        }
        self.value = next;
        self.notifications += 1;
    }

    /// Upstream's failure path, whose guard is
    /// `_disposed || value == unknown`.
    pub fn fail_update(&mut self) {
        if self.disposed || self.value == LiveTextInputStatus::Unknown {
            return;
        }
        self.value = LiveTextInputStatus::Unknown;
        self.notifications += 1;
    }

    pub fn app_resumed(&mut self) {
        self.begin_update();
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
        self.observing = false;
    }
}

/// Upstream `ToolbarItemsParentData`: what a selection toolbar records about
/// each of its buttons.
///
/// The one field beyond position is `shouldPaint`, and it is there because a
/// toolbar lays out **more buttons than it shows**: it measures them all, then
/// paints the ones that fit and puts the rest behind an overflow button.
/// Laying out and painting are separate questions, so a button can be measured
/// and not drawn.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ToolbarItemsParentData {
    pub offset: Offset,
    pub should_paint: bool,
}

impl ToolbarItemsParentData {
    pub fn new() -> ToolbarItemsParentData {
        ToolbarItemsParentData {
            offset: Offset::ZERO,
            should_paint: false,
        }
    }

    pub fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_should_paint(mut self, should_paint: bool) -> Self {
        self.should_paint = should_paint;
        self
    }
}

/// Upstream `TextSelectionGestureDetectorBuilderDelegate`: what the builder
/// asks its field.
pub trait TextSelectionGestureDetectorBuilderDelegate {
    /// Upstream's `forcePressEnabled`.
    ///
    /// A property of the *field* rather than of the platform, because a field
    /// on a pressure-sensitive screen may still not want force press -- and a
    /// field that did want it on a screen without pressure would simply never
    /// see one.
    fn force_press_enabled(&self) -> bool;

    /// Upstream's `selectionEnabled`.
    fn selection_enabled(&self) -> bool;
}

/// Upstream `TextSelectionGestureDetector`: the callbacks a field wires up.
///
/// Upstream has more than twenty, and the count is the point: a text field's
/// gestures are not "tap" and "drag" but a long list of near-misses that mean
/// different things. What is here is the set and the one flag that changes
/// when they fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextSelectionGestureDetector {
    /// Upstream's `onUserTapAlwaysCalled`.
    ///
    /// **False by default**, meaning `onUserTap` fires only when the tap
    /// actually changed something. A field that wants to know about every tap,
    /// including one that landed where the caret already was, sets it -- which
    /// is what a form that scrolls to the focused field needs.
    pub on_user_tap_always_called: bool,
}

impl TextSelectionGestureDetector {
    pub fn new() -> TextSelectionGestureDetector {
        TextSelectionGestureDetector {
            on_user_tap_always_called: false,
        }
    }

    pub fn with_on_user_tap_always_called(mut self, always: bool) -> Self {
        self.on_user_tap_always_called = always;
        self
    }

    /// Upstream `_getEffectiveConsecutiveTapCount`, whose comment says it
    /// "should be used in all instances when `details.consecutiveTapCount`
    /// would be used".
    ///
    /// The recogniser counts taps upwards without limit. What a fourth rapid
    /// click *means* is a platform question, and the three answers are three
    /// different arithmetics rather than three constants -- which is why this
    /// is a function and not a table.
    ///
    /// Upstream's reasoning for each is observation of the native platform,
    /// and it records that plainly: this is what Debian with GTK does, this is
    /// what macOS does. Copied along with the shapes, because a number arrived
    /// at by watching a platform is not one anybody can re-derive later.
    ///
    /// * **Android, Fuchsia, Linux** wrap. Past a triple click the count
    ///   starts over: the fourth click moves the caret to the precise
    ///   position, the fifth selects the word, the sixth the paragraph.
    /// * **iOS and macOS** hold. Past a triple click the paragraph selected by
    ///   the third stays selected.
    /// * **Windows** alternates. After a triple click has taken a paragraph,
    ///   the next click takes the word and the one after it the paragraph
    ///   again -- so it oscillates between two and three and never returns to
    ///   one.
    ///
    /// Note what the three have in common: **none of them keeps counting**. No
    /// caller ever has to ask what a seventh tap means.
    pub fn effective_consecutive_tap_count(raw: u32, platform: TargetPlatform) -> u32 {
        match platform {
            TargetPlatform::Android | TargetPlatform::Fuchsia | TargetPlatform::Linux => {
                if raw <= 3 {
                    raw
                } else if raw % 3 == 0 {
                    3
                } else {
                    raw % 3
                }
            }
            TargetPlatform::IOS | TargetPlatform::MacOS => raw.min(3),
            TargetPlatform::Windows => {
                if raw < 2 {
                    raw
                } else {
                    2 + raw % 2
                }
            }
        }
    }

    /// Whether a tap reaches `onUserTap`, from upstream's `_handleTapUp`.
    ///
    /// The argument is the **effective** count from
    /// [`TextSelectionGestureDetector::effective_consecutive_tap_count`], not
    /// the raw one, which is the whole reason that function exists.
    ///
    /// This used to take a `changed_something: bool` and document itself as
    /// "fires only when the tap actually changed something ... including one
    /// that landed where the caret already was". That was a misreading.
    /// Upstream's condition is the first tap of a series, and a tap landing
    /// exactly where the caret already sits is still a first tap -- it fires.
    /// What does not fire is the *second* tap of a series, whether or not it
    /// moved anything.
    pub fn reports_tap(&self, effective_consecutive_tap_count: u32) -> bool {
        effective_consecutive_tap_count == 1 || self.on_user_tap_always_called
    }

    /// Whether a tap reaches `onSingleTapUp`, which unlike `onUserTap` has no
    /// flag that can widen it.
    pub fn reports_single_tap_up(effective_consecutive_tap_count: u32) -> bool {
        effective_consecutive_tap_count == 1
    }

    /// What a tap-down means beyond `onTapDown`, which fires for every one.
    ///
    /// Upstream returns early on each, so a tap-down is at most one of these.
    pub fn multi_tap_down(effective_consecutive_tap_count: u32) -> Option<MultiTapDown> {
        match effective_consecutive_tap_count {
            2 => Some(MultiTapDown::Double),
            3 => Some(MultiTapDown::Triple),
            _ => None,
        }
    }
}

/// Which of upstream's two extra tap-down callbacks a tap reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiTapDown {
    /// `onDoubleTapDown`.
    Double,
    /// `onTripleTapDown`.
    Triple,
}

/// When a tap moves the caret -- upstream's `onTapDown` against
/// `onSingleTapUp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretMovesOn {
    /// Desktop: the instant the button goes down, so a drag from there
    /// extends the selection.
    TapDown,
    /// Mobile: not until the finger lifts, so a scroll that begins with a
    /// touch does not move the caret on the way past.
    TapUp,
}

/// Where the caret lands within a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretLands {
    /// Exactly where the pointer was.
    Precisely,
    /// At the nearer edge of the word -- a fingertip is wider than a letter,
    /// so the precise position it reports is a guess, and the word's edge is
    /// somewhere the reader can actually have meant.
    AtTheWordEdge,
}

/// The platform rules a text field's gestures follow, which upstream keeps in
/// `TextSelectionGestureDetectorBuilder` so that a field writing its own would
/// not get some of them wrong.
pub struct TextSelectionGestures;

impl TextSelectionGestures {
    /// Upstream's `_shouldShowSelectionToolbar = kind == null || kind == touch
    /// || kind == stylus`.
    ///
    /// The toolbar and the handles are for a **finger or a stylus**. A mouse
    /// has a right-click menu and a keyboard; a fingertip has neither, so the
    /// controls have to be on screen.
    ///
    /// An unknown kind counts as touch, which is the safe way round: showing a
    /// toolbar nobody needed is a nuisance, and withholding one from someone
    /// with no other way to copy is a dead end.
    ///
    /// Upstream is not sure this is right, and says so in a comment above it:
    /// what about "a Windows device with a touchscreen"
    /// (flutter/flutter#106586)? The rule is by device kind and not by
    /// platform, so such a device gets the toolbar -- the open question is
    /// whether it should also keep the desktop behaviours.
    pub fn shows_selection_toolbar(kind: PointerKind) -> bool {
        matches!(
            kind,
            PointerKind::Touch | PointerKind::Stylus | PointerKind::Unknown
        )
    }

    /// Whether the handles follow the toolbar. Upstream assigns one from the
    /// other on the next line, so they cannot disagree.
    pub fn shows_selection_handles(kind: PointerKind) -> bool {
        TextSelectionGestures::shows_selection_toolbar(kind)
    }

    /// Upstream's two comments, each pointing at the other:
    /// "on mobile platforms the selection is set on tap up" in `onTapDown`,
    /// and "on desktop platforms the selection is set on tap down" in
    /// `onSingleTapUp`.
    ///
    /// A desktop moves the caret under the button because a press there is the
    /// start of a possible drag-select, and the caret has to be at one end of
    /// it already. A touch cannot commit that early: a finger going down might
    /// be the beginning of a scroll, and moving the caret for every scroll
    /// would make a list of text unreadable.
    pub fn caret_moves_on(platform: TargetPlatform) -> CaretMovesOn {
        match platform {
            TargetPlatform::Android | TargetPlatform::IOS | TargetPlatform::Fuchsia => {
                CaretMovesOn::TapUp
            }
            TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Windows => {
                CaretMovesOn::TapDown
            }
        }
    }

    /// Upstream's "a shift-tapped unfocused field expands from 0, not from the
    /// previous selection", written once for macOS and once for iOS.
    ///
    /// **Both Apple platforms and neither of the others.** A field that has
    /// never been focused has a previous selection that belongs to nobody, so
    /// expanding from it would extend a range the reader never made; expanding
    /// from the start is at least a range they can see the whole of.
    pub fn shift_tap_expands_from_zero_when_unfocused(platform: TargetPlatform) -> bool {
        matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS)
    }

    /// Where a tap puts the caret, which on iOS depends on **what you touched
    /// it with**.
    ///
    /// Upstream's comment on the macOS arm draws the contrast itself: "On
    /// macOS, a tap/click places the selection in a precise position. This
    /// differs from iOS/iPadOS, where if the gesture is done by a touch then
    /// the selection moves to the closest word edge, instead of a precise
    /// position."
    ///
    /// And iOS's own arm splits by kind: a mouse, a trackpad or a stylus gets
    /// the precise position, a touch or an unknown device gets the word edge.
    /// So a mouse on an iPad behaves like a desktop, because the reason for
    /// the word edge is the fingertip and not the operating system.
    pub fn caret_lands(platform: TargetPlatform, kind: PointerKind) -> CaretLands {
        match platform {
            TargetPlatform::IOS => match kind {
                PointerKind::Touch | PointerKind::Unknown => CaretLands::AtTheWordEdge,
                _ => CaretLands::Precisely,
            },
            _ => CaretLands::Precisely,
        }
    }
}

/// Upstream `TextSelectionGestureDetectorBuilder`: turns a field's delegate
/// into the gesture callbacks above.
///
/// Its value is that the *rules* live in one place. A single tap on a
/// read-only field, a double tap on a desktop, a drag that started on a
/// handle -- each has a platform-dependent answer, and a field that wrote its
/// own would get some of them wrong.
pub struct TextSelectionGestureDetectorBuilder<D: TextSelectionGestureDetectorBuilderDelegate> {
    pub delegate: D,
    /// Upstream's `shouldShowSelectionToolbar`, which a long press sets and a
    /// scroll clears.
    should_show_selection_toolbar: bool,
}

impl<D: TextSelectionGestureDetectorBuilderDelegate> TextSelectionGestureDetectorBuilder<D> {
    pub fn new(delegate: D) -> TextSelectionGestureDetectorBuilder<D> {
        TextSelectionGestureDetectorBuilder {
            delegate,
            should_show_selection_toolbar: true,
        }
    }

    /// Upstream's `shouldShowSelectionToolbar`.
    pub fn should_show_selection_toolbar(&self) -> bool {
        self.should_show_selection_toolbar
    }

    /// Upstream sets this false when a drag begins from a scroll, so that
    /// letting go after a scroll does not pop the toolbar up over the text the
    /// reader was scrolling to.
    pub fn set_should_show_selection_toolbar(&mut self, show: bool) {
        self.should_show_selection_toolbar = show;
    }

    /// Whether a force press should be acted on: the field has to want it.
    pub fn handles_force_press(&self) -> bool {
        self.delegate.force_press_enabled()
    }

    /// Whether a selection gesture should do anything at all.
    pub fn handles_selection(&self) -> bool {
        self.delegate.selection_enabled()
    }
}

/// Upstream `SelectionOverlay`: the handles and the toolbar, positioned.
///
/// Upstream puts them in an `Overlay` so they can be drawn over anything,
/// including outside the field's own bounds -- a handle below the last line of
/// a field would otherwise be clipped away. [`crate::overlay`] carries the
/// entry list and its ordering, and [`crate::selection_host`] hosts the
/// widgets; what is here is the configuration and the visibility rules it
/// reads.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SelectionOverlay {
    /// Upstream's `handlesVisible`.
    pub handles_visible: bool,
    /// Whether the toolbar is up.
    pub toolbar_visible: bool,
    /// Upstream's `lineHeightAtStart` and `lineHeightAtEnd`, which size the
    /// handles: a handle on a line of large text is drawn larger, so it stays
    /// proportionate to what it is holding.
    pub line_height_at_start: f32,
    pub line_height_at_end: f32,
}

impl SelectionOverlay {
    pub fn new() -> SelectionOverlay {
        SelectionOverlay {
            handles_visible: false,
            toolbar_visible: false,
            line_height_at_start: 0.0,
            line_height_at_end: 0.0,
        }
    }

    pub fn with_handles_visible(mut self, visible: bool) -> Self {
        self.handles_visible = visible;
        self
    }

    pub fn with_line_heights(mut self, start: f32, end: f32) -> Self {
        self.line_height_at_start = start;
        self.line_height_at_end = end;
        self
    }

    /// Upstream's `showToolbar`/`hideToolbar` pair.
    pub fn set_toolbar_visible(&mut self, visible: bool) {
        self.toolbar_visible = visible;
    }

    /// Upstream's `hide`, which takes **both** away.
    ///
    /// The pair is hidden together because a toolbar without handles is a
    /// toolbar acting on a selection the reader can no longer see the edges
    /// of.
    pub fn hide(&mut self) {
        self.handles_visible = false;
        self.toolbar_visible = false;
    }
}

/// Upstream `TextSelectionOverlay`: a [`SelectionOverlay`] wired to an
/// editable field.
///
/// Its own addition is the **drag** handling: it remembers where in a handle
/// the finger grabbed, so that dragging does not jump the selection to wherever
/// the finger is. The same reasoning as the drag anchor in
/// [`crate::drag_target`], and for the same reason -- a handle that jumped
/// would read as a different handle.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextSelectionOverlay {
    pub overlay: SelectionOverlay,
    /// Where in the handle the finger landed, kept for the whole drag.
    drag_offset: Option<Offset>,
}

impl TextSelectionOverlay {
    pub fn new() -> TextSelectionOverlay {
        TextSelectionOverlay {
            overlay: SelectionOverlay::new(),
            drag_offset: None,
        }
    }

    /// Upstream's `_handleSelectionStartHandleDragStart`, reduced to the part
    /// that matters: the grab point is recorded once.
    pub fn begin_handle_drag(&mut self, grab: Offset) {
        self.drag_offset = Some(grab);
    }

    /// Where the selection edge goes for a finger at `position`.
    pub fn handle_drag_position(&self, position: Offset) -> Offset {
        match self.drag_offset {
            Some(grab) => Offset::new(position.dx - grab.dx, position.dy - grab.dy),
            None => position,
        }
    }

    pub fn end_handle_drag(&mut self) {
        self.drag_offset = None;
    }

    pub fn is_dragging_handle(&self) -> bool {
        self.drag_offset.is_some()
    }
}

/// Upstream `TextSelectionToolbarLayoutDelegate`.
///
/// Where a selection toolbar goes. It is given **two anchors, not one** --
/// where to sit above the selection and where to sit below -- and picks between
/// them at layout, because nobody can know which side the toolbar fits on until
/// it has been measured, and by then the caller is long gone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionToolbarLayoutDelegate {
    pub anchor_above: (f32, f32),
    pub anchor_below: (f32, f32),
    /// An override. Upstream's reason is specific: the Material toolbar forces
    /// it while its overflow menu is open, because the open menu is taller than
    /// the closed one and the toolbar would otherwise flip sides in the middle
    /// of the reader using it. **The override exists so a widget can hold a
    /// decision still while it animates.**
    pub fits_above: Option<bool>,
}

impl TextSelectionToolbarLayoutDelegate {
    pub fn new(
        anchor_above: (f32, f32),
        anchor_below: (f32, f32),
    ) -> TextSelectionToolbarLayoutDelegate {
        TextSelectionToolbarLayoutDelegate {
            anchor_above,
            anchor_below,
            fits_above: None,
        }
    }

    pub fn with_fits_above(mut self, fits: bool) -> Self {
        self.fits_above = Some(fits);
        self
    }

    /// Upstream's static `centerOn`, whose three cases upstream names in
    /// comments: overflowing left, put it as far left as possible; overflowing
    /// right, as far right; otherwise perfectly centred.
    ///
    /// **A toolbar half off the screen is worse than one not quite over the
    /// selection** -- the same judgement the tooltip's `positionDependentBox`
    /// makes.
    pub fn center_on(position: f32, width: f32, max: f32) -> f32 {
        if position - width / 2.0 < 0.0 {
            return 0.0;
        }
        if position + width / 2.0 > max {
            return max - width;
        }
        position - width / 2.0
    }

    /// Upstream `getPositionForChild`.
    pub fn position_for_child(&self, size: (f32, f32), child_size: (f32, f32)) -> (f32, f32) {
        let fits_above = self
            .fits_above
            .unwrap_or(self.anchor_above.1 >= child_size.1);
        let anchor = if fits_above {
            self.anchor_above
        } else {
            self.anchor_below
        };
        let y = if fits_above {
            // Even "fits" is clamped: a toolbar pushed off the top would be
            // unreachable rather than merely misplaced.
            (anchor.1 - child_size.1).max(0.0)
        } else {
            anchor.1
        };
        (
            TextSelectionToolbarLayoutDelegate::center_on(anchor.0, child_size.0, size.0),
            y,
        )
    }

    /// Upstream `shouldRelayout`, which compares all three inputs -- including
    /// the override, so forcing it re-runs the layout.
    pub fn should_relayout(&self, old: &TextSelectionToolbarLayoutDelegate) -> bool {
        self.anchor_above != old.anchor_above
            || self.anchor_below != old.anchor_below
            || self.fits_above != old.fits_above
    }
}

// -- The pieces a selection is described with ---------------------------------

/// Upstream `TextSelectionPoint`: one end of a selection, and which way the
/// text runs there.
///
/// The direction is per *point*, not per selection, and that is the reason this
/// is a type rather than an `Offset`. A selection that starts in English and
/// ends in Arabic has ends that run opposite ways, and the handle drawn at each
/// end has to know which -- a left handle at a right-to-left end is the wrong
/// handle.
///
/// It is `Option` for the same reason upstream's is nullable: a point in text
/// with no strong direction has no answer to give.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionPoint {
    pub point: Offset,
    pub direction: Option<TextDirection>,
}

impl TextSelectionPoint {
    pub fn new(point: Offset, direction: Option<TextDirection>) -> TextSelectionPoint {
        TextSelectionPoint { point, direction }
    }
}

/// Upstream `DesktopTextSelectionToolbarLayoutDelegate`: where a desktop
/// selection toolbar goes.
///
/// # It hangs from the anchor and is pulled back only when it would fall off
///
/// The whole rule is upstream's `getPositionForChild`: put the toolbar's
/// top-left at the anchor, then, for each axis independently, if that would
/// push its far edge past the container, slide it back by exactly the overhang.
///
/// **Independently per axis** is the part worth having: a toolbar near the
/// right edge and nowhere near the bottom slides left and does not move up. The
/// Material toolbar's rule is different -- it flips above or below the selection
/// (see [`crate::text_selection::TextSelectionToolbarLayoutDelegate`]) -- because
/// a touch toolbar must not sit under the finger, and a desktop one has no
/// finger to avoid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopTextSelectionToolbarLayoutDelegate {
    pub anchor: Offset,
}

impl DesktopTextSelectionToolbarLayoutDelegate {
    pub fn new(anchor: Offset) -> DesktopTextSelectionToolbarLayoutDelegate {
        DesktopTextSelectionToolbarLayoutDelegate { anchor }
    }

    /// Upstream's `getConstraintsForChild`, which loosens: the toolbar is its
    /// own size, not the container's.
    pub fn constraints_for_child(&self, container: Size) -> Size {
        container
    }

    /// Upstream's `getPositionForChild`.
    pub fn position_for_child(&self, container: Size, child: Size) -> Offset {
        let overhang_x = self.anchor.dx + child.width - container.width;
        let overhang_y = self.anchor.dy + child.height - container.height;
        Offset::new(
            if overhang_x > 0.0 {
                self.anchor.dx - overhang_x
            } else {
                self.anchor.dx
            },
            if overhang_y > 0.0 {
                self.anchor.dy - overhang_y
            } else {
                self.anchor.dy
            },
        )
    }
}

/// Upstream `DefaultSelectionStyle`: the cursor and selection colours a field
/// takes when it was not told.
///
/// An inherited theme, so a form can set them once. Upstream's `merge` takes
/// each field from the new value **or the enclosing one**, which is what lets a
/// subtree override the cursor colour without restating the selection colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DefaultSelectionStyle {
    pub cursor_color: Option<Color>,
    pub selection_color: Option<Color>,
}

impl DefaultSelectionStyle {
    /// Upstream's `defaultColor`, a half-transparent grey. It is the fallback
    /// for *both* colours, which is why it is one constant.
    pub const DEFAULT_COLOR: Color = Color::argb(0x80, 0x80, 0x80, 0x80);

    pub fn new() -> DefaultSelectionStyle {
        DefaultSelectionStyle::default()
    }

    pub fn with_cursor_color(mut self, color: Color) -> Self {
        self.cursor_color = Some(color);
        self
    }

    pub fn with_selection_color(mut self, color: Color) -> Self {
        self.selection_color = Some(color);
        self
    }

    /// Upstream's `merge`: this style over `parent`, field by field.
    ///
    /// A field this style did not set is taken from the parent -- **not** from
    /// the default. Falling back to the default per field would make a subtree
    /// that set only the cursor colour silently discard the form's selection
    /// colour.
    pub fn merge(&self, parent: &DefaultSelectionStyle) -> DefaultSelectionStyle {
        DefaultSelectionStyle {
            cursor_color: self.cursor_color.or(parent.cursor_color),
            selection_color: self.selection_color.or(parent.selection_color),
        }
    }

    /// The colours a field paints with, read off the ambient
    /// `TextSelectionTheme` and the scheme -- upstream's `_TextFieldState`
    /// chain, including the rule that an error cursor is not negotiable.
    ///
    /// [`DefaultSelectionStyle::resolved`] is the same question with nothing to
    /// read from, and answers with upstream's grey.
    pub fn of(
        context: &mut crate::framework::BuildContext,
        has_error: bool,
    ) -> crate::component_themes::ResolvedTextSelection {
        let style = context
            .inherited::<DefaultSelectionStyle>()
            .map(|style| *style)
            .unwrap_or_default();
        crate::component_themes::ResolvedTextSelection::of(context, style.cursor_color, has_error)
    }

    /// The colours to actually paint with, once nobody above set them.
    pub fn resolved(&self) -> (Color, Color) {
        (
            self.cursor_color
                .unwrap_or(DefaultSelectionStyle::DEFAULT_COLOR),
            self.selection_color
                .unwrap_or(DefaultSelectionStyle::DEFAULT_COLOR),
        )
    }
}

/// Upstream `RenderEditablePainter`: something that draws over an editable,
/// under or above its text.
///
/// Upstream is an abstract `ChangeNotifier` with two members --
/// `shouldRepaint(old)` and `paint(canvas, size, renderEditable)` -- and this is
/// the same pair. What it is for is the caret and the selection highlight,
/// which upstream draws through two of these rather than in `RenderEditable`
/// itself, so that a field can replace either without replacing the field.
pub trait RenderEditablePainter {
    /// Upstream's `shouldRepaint`. `None` means there was no previous painter,
    /// which upstream treats as "yes" -- a painter that has just arrived has
    /// not drawn yet.
    fn should_repaint(&self, old: Option<&dyn RenderEditablePainter>) -> bool;

    /// Upstream's `paint`. The size is the editable's, and the painter draws in
    /// its coordinates.
    fn paint(&self, context: &mut PaintContext, offset: Offset, size: Size);
}

/// Upstream `VerticalCaretMovementRun`: the caret moving up or down through
/// lines, one arrow press at a time.
///
/// # The column is sticky, and that is the whole point
///
/// Pressing down from the end of a long line onto a short one puts the caret at
/// the end of the short line -- and pressing down *again* onto another long line
/// puts it back at the original column, not at the short line's end. Upstream
/// gets that by keeping the **original horizontal offset** for the life of the
/// run and asking each new line what is closest to it, rather than carrying the
/// position from line to line.
///
/// A run is therefore a thing with a lifetime: it starts when the reader begins
/// arrowing vertically and ends when they do anything else. Upstream's editable
/// holds one and drops it on any horizontal movement or edit.
///
/// # It goes invalid when the text is laid out again
///
/// Upstream compares the line metrics **by identity** -- `!identical(newLineMetrics,
/// _lineMetrics)` -- and gives up if they were recomputed, with a comment
/// admitting it is leaning on an implementation detail of `computeLineMetrics`.
/// A run that kept going would be indexing lines that no longer exist. This port
/// keeps the rule and takes a revision number instead of relying on identity,
/// which is the same test said in a way that does not depend on an allocator.
pub struct VerticalCaretMovementRun {
    /// Where the caret was when the run started. Nothing writes it after
    /// construction -- that is the stickiness, and it is a property of the type
    /// rather than of any one method.
    origin_x: f32,
    current_line: usize,
    line_count: usize,
    /// The layout this run was made against.
    revision: u64,
    valid: bool,
}

impl VerticalCaretMovementRun {
    pub fn new(
        origin_x: f32,
        current_line: usize,
        line_count: usize,
        revision: u64,
    ) -> VerticalCaretMovementRun {
        VerticalCaretMovementRun {
            origin_x,
            current_line,
            line_count,
            revision,
            valid: true,
        }
    }

    /// The column every line in this run is measured against.
    pub fn origin_x(&self) -> f32 {
        self.origin_x
    }

    pub fn current_line(&self) -> usize {
        self.current_line
    }

    /// Upstream's `isValid`, which consults the layout each time it is asked
    /// rather than being told.
    pub fn is_valid(&mut self, layout_revision: u64) -> bool {
        if layout_revision != self.revision {
            self.valid = false;
        }
        self.valid
    }

    /// Upstream's `moveNext`: down a line, or false at the last one.
    ///
    /// **False rather than clamping.** The caller needs to know it did not
    /// move, because an arrow at the bottom of a field should pass through to
    /// whatever is below rather than being swallowed.
    pub fn move_next(&mut self) -> bool {
        if !self.valid || self.current_line + 1 >= self.line_count {
            return false;
        }
        self.current_line += 1;
        true
    }

    /// Upstream's `movePrevious`.
    pub fn move_previous(&mut self) -> bool {
        if !self.valid || self.current_line == 0 {
            return false;
        }
        self.current_line -= 1;
        true
    }

    /// Where the caret lands on the current line: the sticky column, clamped
    /// into the line.
    ///
    /// `line_extent` is how wide the line's text is. A line shorter than the
    /// column puts the caret at its end, and the *next* line is still measured
    /// against the original column -- which is the behaviour the whole type
    /// exists for.
    pub fn offset_in_line(&self, line_extent: f32) -> f32 {
        self.origin_x.min(line_extent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_a_real_state_and_not_a_missing_answer() {
        // A paste button shown while the answer is unknown might do nothing;
        // one hidden would flicker into existence a frame later. Upstream
        // keeps the state so a caller can decide which it prefers.
        let notifier = ClipboardStatusNotifier::new();
        assert_eq!(notifier.value(), ClipboardStatus::Unknown);
        assert_eq!(ClipboardStatus::default(), ClipboardStatus::Unknown);
        assert_ne!(ClipboardStatus::Unknown, ClipboardStatus::NotPasteable);
    }

    #[test]
    fn a_notifier_nobody_listens_to_does_not_talk_to_the_host() {
        // The first listener is what starts it: the ask and the lifecycle
        // observation both happen then and not before.
        let mut notifier = ClipboardStatusNotifier::new();
        assert_eq!(notifier.updates(), 0);
        assert!(!notifier.is_observing());

        notifier.add_listener();
        assert_eq!(notifier.updates(), 1, "the first listener asks");
        assert!(notifier.is_observing());
    }

    #[test]
    fn a_second_listener_does_not_ask_again_once_the_answer_is_known() {
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.complete_update(true);
        assert_eq!(notifier.value(), ClipboardStatus::Pasteable);

        notifier.add_listener();
        assert_eq!(notifier.updates(), 1, "the answer is already in");

        // But while it is still unknown, every listener triggers an ask --
        // upstream's condition is on the value, not on the listener count.
        let mut unanswered = ClipboardStatusNotifier::new();
        unanswered.add_listener();
        unanswered.add_listener();
        assert_eq!(unanswered.updates(), 2);
    }

    #[test]
    fn the_last_listener_leaving_stops_the_observation() {
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.add_listener();
        notifier.remove_listener();
        assert!(notifier.is_observing(), "one listener left");
        notifier.remove_listener();
        assert!(!notifier.is_observing());
    }

    #[test]
    fn coming_back_from_the_background_asks_again() {
        // The reader may have copied something in another application while
        // away, and nothing else would tell us.
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.complete_update(false);
        assert_eq!(notifier.value(), ClipboardStatus::NotPasteable);

        notifier.app_resumed();
        assert_eq!(notifier.updates(), 2);
        notifier.complete_update(true);
        assert_eq!(notifier.value(), ClipboardStatus::Pasteable);
    }

    #[test]
    fn a_failed_ask_goes_back_to_unknown_rather_than_keeping_a_stale_answer() {
        // Upstream's comment: so that it will try again later. A stale
        // Pasteable would leave a paste button that does nothing.
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.complete_update(true);
        assert_eq!(notifier.value(), ClipboardStatus::Pasteable);

        notifier.fail_update();
        assert_eq!(notifier.value(), ClipboardStatus::Unknown);
    }

    #[test]
    fn an_answer_arriving_after_disposal_is_dropped() {
        // Upstream checks _disposed before and after the await, and the second
        // check is the one that matters: a notifier disposed while the host
        // was answering must not write into a dead object.
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.dispose();
        assert!(notifier.is_disposed());
        assert!(!notifier.is_observing(), "disposal unregisters");

        notifier.complete_update(true);
        assert_eq!(notifier.value(), ClipboardStatus::Unknown, "dropped");
        assert!(!notifier.begin_update(), "and no further asks go out");
    }

    #[test]
    fn and_so_is_a_failure_arriving_after_disposal() {
        // The other half of the same await. `fail_update` writes `Unknown`
        // **on purpose** -- so the notifier tries again later -- which makes
        // it the one path where writing into a dead object looks harmless:
        // the value it writes is the value a fresh notifier has.
        //
        // It is not harmless. A notifier is a `ValueNotifier`, and writing to
        // it is what tells its listeners; a disposed one has told them it is
        // finished. Nothing reached this guard until a screen for guards the
        // suite cannot make matter pointed at it, and the reason nothing did
        // is exactly that the value looks unchanged.
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.complete_update(true);
        assert_eq!(notifier.value(), ClipboardStatus::Pasteable);
        notifier.dispose();
        notifier.fail_update();
        assert_eq!(
            notifier.value(),
            ClipboardStatus::Pasteable,
            "the dead notifier keeps whatever it last said, so nothing it              is still wired to is told anything"
        );

        // And a live one does take the failure, which is what says the rule
        // belongs to disposal rather than to `fail_update` never doing
        // anything.
        let mut notifier = ClipboardStatusNotifier::new();
        notifier.add_listener();
        notifier.complete_update(true);
        notifier.fail_update();
        assert_eq!(notifier.value(), ClipboardStatus::Unknown);
    }

    #[test]
    fn live_text_says_nothing_when_the_answer_has_not_changed() {
        // Live Text availability is a property of the device and almost never
        // moves; a notification saying it is still what it was would rebuild
        // every toolbar for nothing.
        let mut notifier = LiveTextInputStatusNotifier::new();
        notifier.add_listener();
        notifier.complete_update(true);
        assert_eq!(notifier.value(), LiveTextInputStatus::Enabled);
        assert_eq!(notifier.notifications(), 1);

        notifier.complete_update(true);
        assert_eq!(notifier.notifications(), 1, "the same answer is not news");

        notifier.complete_update(false);
        assert_eq!(notifier.notifications(), 2);
        assert_eq!(notifier.value(), LiveTextInputStatus::Disabled);
    }

    #[test]
    fn a_live_text_failure_from_unknown_changes_nothing() {
        // Upstream's guard is `_disposed || value == unknown` -- setting
        // unknown to unknown would notify for nothing.
        let mut notifier = LiveTextInputStatusNotifier::new();
        notifier.add_listener();
        notifier.fail_update();
        assert_eq!(notifier.value(), LiveTextInputStatus::Unknown);
        assert_eq!(notifier.notifications(), 0);

        // From a known answer it does go back to unknown, and does notify.
        notifier.complete_update(true);
        assert_eq!(notifier.notifications(), 1);
        notifier.fail_update();
        assert_eq!(notifier.value(), LiveTextInputStatus::Unknown);
        assert_eq!(notifier.notifications(), 2);
    }

    #[test]
    fn the_two_notifiers_agree_about_their_lifecycle_even_where_they_differ() {
        let mut live = LiveTextInputStatusNotifier::new();
        assert!(!live.is_observing());
        live.add_listener();
        assert!(live.is_observing());
        assert_eq!(live.updates(), 1);

        live.app_resumed();
        assert_eq!(live.updates(), 2);

        live.dispose();
        assert!(!live.is_observing());
        assert!(!live.begin_update());
    }

    #[test]
    fn a_toolbar_measures_more_buttons_than_it_paints() {
        // Laying out and painting are separate questions, so a button can be
        // measured and not drawn -- which is how the overflow menu knows what
        // it holds.
        let measured = ToolbarItemsParentData::new().with_offset(Offset::new(10.0, 0.0));
        assert!(!measured.should_paint, "measured but not shown");
        assert_eq!(measured.offset, Offset::new(10.0, 0.0));

        let shown = measured.with_should_paint(true);
        assert!(shown.should_paint);
        assert_eq!(shown.offset, Offset::new(10.0, 0.0), "the same position");
    }

    struct Field {
        force_press: bool,
        selectable: bool,
    }

    impl TextSelectionGestureDetectorBuilderDelegate for Field {
        fn force_press_enabled(&self) -> bool {
            self.force_press
        }

        fn selection_enabled(&self) -> bool {
            self.selectable
        }
    }

    #[test]
    fn force_press_is_a_property_of_the_field_and_not_of_the_platform() {
        // A field on a pressure-sensitive screen may still not want it, and a
        // field that did want it on a screen without pressure would simply
        // never see one.
        let wants = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: true,
            selectable: true,
        });
        assert!(wants.handles_force_press());

        let does_not = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: false,
            selectable: true,
        });
        assert!(!does_not.handles_force_press());
        assert!(does_not.handles_selection());
    }

    #[test]
    fn letting_go_after_a_scroll_does_not_pop_the_toolbar_up() {
        // Over the very text the reader was scrolling to.
        let mut builder = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: false,
            selectable: true,
        });
        assert!(builder.should_show_selection_toolbar());
        builder.set_should_show_selection_toolbar(false);
        assert!(!builder.should_show_selection_toolbar());
    }

    #[test]
    fn only_the_first_tap_of_a_series_is_reported_unless_asked_otherwise() {
        // Not "a tap that changed something", which is what this said before:
        // a tap landing exactly where the caret already sits is still a first
        // tap and still fires. What does not fire is the second of a series.
        let ordinary = TextSelectionGestureDetector::new();
        assert!(ordinary.reports_tap(1));
        for later in 2..=3 {
            assert!(!ordinary.reports_tap(later), "{later}");
        }

        // Which is what a form that scrolls to the focused field needs.
        let always = TextSelectionGestureDetector::new().with_on_user_tap_always_called(true);
        for any in 1..=3 {
            assert!(always.reports_tap(any), "{any}");
        }
    }

    #[test]
    fn the_flag_widens_only_the_user_tap_and_not_the_single_tap_up() {
        // Two callbacks fire together on a first tap and part company after
        // it. onSingleTapUp has no flag that can widen it.
        let always = TextSelectionGestureDetector::new().with_on_user_tap_always_called(true);
        assert!(always.reports_tap(2));
        assert!(!TextSelectionGestureDetector::reports_single_tap_up(2));
        assert!(TextSelectionGestureDetector::reports_single_tap_up(1));
    }

    #[test]
    fn a_fourth_rapid_click_means_three_different_things() {
        // The recogniser counts upwards without limit; what the fourth click
        // means is a platform question, and upstream's answers come from
        // watching the native platforms rather than from a rule.
        use TargetPlatform::*;
        let effective = TextSelectionGestureDetector::effective_consecutive_tap_count;
        assert_eq!(effective(4, Linux), 1, "wraps: back to a precise caret");
        assert_eq!(effective(4, MacOS), 3, "holds: the paragraph stays");
        assert_eq!(effective(4, Windows), 2, "alternates: back to the word");
    }

    #[test]
    fn and_the_first_three_clicks_mean_the_same_thing_everywhere() {
        // The platforms differ only past the triple click, so a test that
        // stopped at three would find them identical.
        use TargetPlatform::*;
        for platform in [Android, Fuchsia, Linux, IOS, MacOS, Windows] {
            for raw in 0..=3 {
                assert_eq!(
                    TextSelectionGestureDetector::effective_consecutive_tap_count(raw, platform),
                    raw,
                    "{platform:?} {raw}"
                );
            }
        }
    }

    #[test]
    fn the_wrapping_platforms_start_the_series_over() {
        // Upstream's observation, in its own words: on the fourth click the
        // selection moves to the precise position, on the fifth the word, on
        // the sixth the paragraph.
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
        ] {
            let effective =
                |raw| TextSelectionGestureDetector::effective_consecutive_tap_count(raw, platform);
            assert_eq!(
                [effective(4), effective(5), effective(6)],
                [1, 2, 3],
                "{platform:?}"
            );
            assert_eq!(effective(7), 1, "{platform:?} and round again");
            assert_eq!(
                effective(9),
                3,
                "{platform:?} a multiple of three is the triple"
            );
        }
    }

    #[test]
    fn the_alternating_platform_never_gets_back_to_one() {
        // Which is the part that is not obvious from the name: past the first
        // click Windows oscillates between the word and the paragraph, and a
        // single tap is unreachable however long the series runs.
        let effective = |raw| {
            TextSelectionGestureDetector::effective_consecutive_tap_count(
                raw,
                TargetPlatform::Windows,
            )
        };
        assert_eq!(
            [effective(2), effective(3), effective(4), effective(5)],
            [2, 3, 2, 3]
        );
        for raw in 2..20 {
            assert_ne!(effective(raw), 1, "{raw}");
        }
    }

    #[test]
    fn none_of_the_three_rules_keeps_counting() {
        // What they have in common, and the reason no caller ever has to ask
        // what a seventh tap means.
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::IOS,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            for raw in 0..50 {
                let effective =
                    TextSelectionGestureDetector::effective_consecutive_tap_count(raw, platform);
                assert!(effective <= 3, "{platform:?} {raw} -> {effective}");
            }
        }
    }

    #[test]
    fn a_tap_down_reaches_at_most_one_of_the_extra_callbacks() {
        use super::MultiTapDown;
        assert_eq!(TextSelectionGestureDetector::multi_tap_down(1), None);
        assert_eq!(
            TextSelectionGestureDetector::multi_tap_down(2),
            Some(MultiTapDown::Double)
        );
        assert_eq!(
            TextSelectionGestureDetector::multi_tap_down(3),
            Some(MultiTapDown::Triple)
        );
        assert_eq!(
            TextSelectionGestureDetector::multi_tap_down(4),
            None,
            "and nothing above three, which is why the count is converted first"
        );
    }

    #[test]
    fn the_handles_and_the_toolbar_are_hidden_together() {
        // A toolbar without handles acts on a selection whose edges the reader
        // can no longer see.
        let mut overlay = SelectionOverlay::new().with_handles_visible(true);
        overlay.set_toolbar_visible(true);
        assert!(overlay.handles_visible && overlay.toolbar_visible);

        overlay.hide();
        assert!(!overlay.handles_visible);
        assert!(!overlay.toolbar_visible);
    }

    #[test]
    fn a_handle_is_sized_by_the_line_it_is_holding() {
        // So it stays proportionate to text of any size.
        let overlay = SelectionOverlay::new().with_line_heights(16.0, 32.0);
        assert_eq!(overlay.line_height_at_start, 16.0);
        assert_eq!(
            overlay.line_height_at_end, 32.0,
            "the two ends can be on lines of different size"
        );
    }

    #[test]
    fn a_dragged_handle_keeps_the_point_it_was_grabbed_by() {
        // The same reasoning as a drag anchor: a handle that jumped to the
        // finger would read as a different handle.
        let mut overlay = TextSelectionOverlay::new();
        assert!(!overlay.is_dragging_handle());
        assert_eq!(
            overlay.handle_drag_position(Offset::new(100.0, 50.0)),
            Offset::new(100.0, 50.0),
            "with no drag under way the position is the position"
        );

        overlay.begin_handle_drag(Offset::new(4.0, 6.0));
        assert!(overlay.is_dragging_handle());
        assert_eq!(
            overlay.handle_drag_position(Offset::new(100.0, 50.0)),
            Offset::new(96.0, 44.0),
            "the grab point is held for the whole drag"
        );

        overlay.end_handle_drag();
        assert!(!overlay.is_dragging_handle());
    }
    // -- TextSelectionToolbarLayoutDelegate ------------------------------------

    fn delegate() -> TextSelectionToolbarLayoutDelegate {
        TextSelectionToolbarLayoutDelegate::new((200.0, 300.0), (200.0, 360.0))
    }

    #[test]
    fn two_anchors_because_nobody_knows_which_side_fits_until_it_is_measured() {
        let delegate = delegate();
        // Room above: 300 of space for a 44-tall toolbar.
        assert_eq!(
            delegate.position_for_child((400.0, 800.0), (120.0, 44.0)).1,
            256.0
        );
        // No room above: a toolbar taller than the anchor goes below instead.
        assert_eq!(
            delegate
                .position_for_child((400.0, 800.0), (120.0, 400.0))
                .1,
            360.0
        );
    }

    #[test]
    fn a_toolbar_half_off_the_screen_is_worse_than_one_not_quite_centred() {
        assert_eq!(
            TextSelectionToolbarLayoutDelegate::center_on(200.0, 120.0, 400.0),
            140.0,
            "centred where it fits"
        );
        assert_eq!(
            TextSelectionToolbarLayoutDelegate::center_on(10.0, 120.0, 400.0),
            0.0,
            "flush left rather than off the left edge"
        );
        assert_eq!(
            TextSelectionToolbarLayoutDelegate::center_on(395.0, 120.0, 400.0),
            280.0,
            "and flush right at the other end"
        );
    }

    #[test]
    fn forcing_the_side_holds_it_still_while_something_animates() {
        // The Material toolbar forces it while its overflow menu opens, because
        // the open menu is taller and the toolbar would otherwise flip sides in
        // the middle of the reader using it.
        let forced = delegate().with_fits_above(true);
        assert_eq!(
            forced.position_for_child((400.0, 800.0), (120.0, 400.0)).1,
            0.0,
            "kept above, and clamped rather than pushed off the top"
        );

        let unforced = delegate();
        assert_eq!(
            unforced
                .position_for_child((400.0, 800.0), (120.0, 400.0))
                .1,
            360.0,
            "which is what it would have done on its own"
        );
    }

    #[test]
    fn even_fitting_above_is_clamped_to_the_top_of_the_screen() {
        // A toolbar pushed off the top would be unreachable rather than merely
        // misplaced.
        let tight = TextSelectionToolbarLayoutDelegate::new((200.0, 20.0), (200.0, 80.0))
            .with_fits_above(true);
        assert_eq!(
            tight.position_for_child((400.0, 800.0), (120.0, 44.0)).1,
            0.0
        );
    }

    #[test]
    fn changing_the_forced_side_re_runs_the_layout() {
        let base = delegate();
        assert!(!base.should_relayout(&delegate()));
        assert!(base.should_relayout(&delegate().with_fits_above(true)));
        assert!(
            base.should_relayout(&TextSelectionToolbarLayoutDelegate::new(
                (200.0, 301.0),
                (200.0, 360.0)
            ))
        );
    }
}

#[cfg(test)]
mod selection_pieces_tests {
    use super::*;

    fn at(x: f32, y: f32) -> Offset {
        Offset::new(x, y)
    }

    // -- TextSelectionPoint -----------------------------------------------------------

    #[test]
    fn a_selections_two_ends_may_run_opposite_ways() {
        // Which is why the direction is per point. A left handle at a
        // right-to-left end is the wrong handle.
        let start = TextSelectionPoint::new(at(10.0, 20.0), Some(TextDirection::Ltr));
        let end = TextSelectionPoint::new(at(90.0, 20.0), Some(TextDirection::Rtl));
        assert_ne!(start.direction, end.direction);
    }

    #[test]
    fn a_point_in_text_with_no_strong_direction_has_none_to_give() {
        let point = TextSelectionPoint::new(at(0.0, 0.0), None);
        assert_eq!(point.direction, None);
    }

    // -- The desktop toolbar's placement -------------------------------------------------

    #[test]
    fn a_desktop_toolbar_hangs_from_its_anchor() {
        let delegate = DesktopTextSelectionToolbarLayoutDelegate::new(at(40.0, 60.0));
        let at_anchor =
            delegate.position_for_child(Size::new(800.0, 600.0), Size::new(200.0, 40.0));
        assert_eq!(at_anchor, at(40.0, 60.0), "room on both sides");
    }

    #[test]
    fn it_slides_back_by_exactly_the_overhang() {
        // Anchor at 700 with a 200-wide toolbar in an 800-wide container
        // overhangs by 100, so it lands at 600 -- flush with the right edge.
        let delegate = DesktopTextSelectionToolbarLayoutDelegate::new(at(700.0, 60.0));
        let placed = delegate.position_for_child(Size::new(800.0, 600.0), Size::new(200.0, 40.0));
        assert_eq!(placed.dx, 600.0);
        assert_eq!(placed.dy, 60.0, "and the other axis did not move");
    }

    #[test]
    fn the_two_axes_are_decided_independently() {
        // A toolbar near the right edge and nowhere near the bottom slides left
        // and does not move up.
        let delegate = DesktopTextSelectionToolbarLayoutDelegate::new(at(700.0, 580.0));
        let placed = delegate.position_for_child(Size::new(800.0, 600.0), Size::new(200.0, 40.0));
        assert_eq!(placed.dx, 600.0, "pulled back");
        assert_eq!(placed.dy, 560.0, "and so was this one, on its own account");

        let one_axis = DesktopTextSelectionToolbarLayoutDelegate::new(at(700.0, 10.0));
        let placed = one_axis.position_for_child(Size::new(800.0, 600.0), Size::new(200.0, 40.0));
        assert_eq!((placed.dx, placed.dy), (600.0, 10.0));
    }

    #[test]
    fn a_toolbar_that_fits_exactly_is_not_moved() {
        // The overhang is zero, and the rule is `> 0`.
        let delegate = DesktopTextSelectionToolbarLayoutDelegate::new(at(600.0, 0.0));
        let placed = delegate.position_for_child(Size::new(800.0, 600.0), Size::new(200.0, 40.0));
        assert_eq!(placed.dx, 600.0);
    }

    // -- DefaultSelectionStyle ---------------------------------------------------------------

    #[test]
    fn merging_takes_each_field_from_the_parent_and_not_from_the_default() {
        // A subtree that set only the cursor colour must not silently discard
        // the form's selection colour.
        let form = DefaultSelectionStyle::new()
            .with_cursor_color(Color::argb(0xFF, 1, 0, 0))
            .with_selection_color(Color::argb(0xFF, 0, 1, 0));
        let subtree = DefaultSelectionStyle::new().with_cursor_color(Color::argb(0xFF, 0, 0, 1));

        let merged = subtree.merge(&form);
        assert_eq!(
            merged.cursor_color,
            Some(Color::argb(0xFF, 0, 0, 1)),
            "its own"
        );
        assert_eq!(
            merged.selection_color,
            Some(Color::argb(0xFF, 0, 1, 0)),
            "and the form's, kept"
        );
    }

    #[test]
    fn merging_two_styles_that_both_set_a_colour_takes_the_nearer_one() {
        // The test above sets the selection colour on the parent only, so it
        // shows that the parent's survives -- not that the child's would win.
        // With both set the direction is visible, and `tools/order_sweep.py`
        // found it by swapping the two sides and watching nothing fail.
        let form = DefaultSelectionStyle::new()
            .with_cursor_color(Color::argb(0xFF, 1, 0, 0))
            .with_selection_color(Color::argb(0xFF, 0, 1, 0));
        let subtree = DefaultSelectionStyle::new()
            .with_cursor_color(Color::argb(0xFF, 0, 0, 1))
            .with_selection_color(Color::argb(0xFF, 0, 0, 2));

        let merged = subtree.merge(&form);
        assert_eq!(merged.cursor_color, Some(Color::argb(0xFF, 0, 0, 1)));
        assert_eq!(merged.selection_color, Some(Color::argb(0xFF, 0, 0, 2)));
    }

    #[test]
    fn both_colours_fall_back_to_the_one_default() {
        let (cursor, selection) = DefaultSelectionStyle::new().resolved();
        assert_eq!(cursor, DefaultSelectionStyle::DEFAULT_COLOR);
        assert_eq!(selection, DefaultSelectionStyle::DEFAULT_COLOR);
        assert_eq!(
            DefaultSelectionStyle::DEFAULT_COLOR,
            Color::argb(0x80, 0x80, 0x80, 0x80),
            "upstream's half-transparent grey"
        );
    }

    // -- VerticalCaretMovementRun ---------------------------------------------------------------

    #[test]
    fn the_column_is_sticky_across_a_short_line() {
        // Down from the end of a long line onto a short one puts the caret at
        // the short line's end; down again onto a long one puts it back at the
        // original column. That is the whole reason this type exists.
        let mut run = VerticalCaretMovementRun::new(200.0, 0, 3, 1);
        assert_eq!(
            run.offset_in_line(400.0),
            200.0,
            "the long line it started on"
        );

        assert!(run.move_next());
        assert_eq!(
            run.offset_in_line(50.0),
            50.0,
            "clamped into the short line"
        );

        assert!(run.move_next());
        assert_eq!(
            run.offset_in_line(400.0),
            200.0,
            "back at the original column, not at 50"
        );
    }

    #[test]
    fn a_run_stops_at_the_ends_rather_than_clamping() {
        // The caller needs to know it did not move, because an arrow at the
        // bottom should pass through to whatever is below rather than being
        // swallowed.
        let mut run = VerticalCaretMovementRun::new(0.0, 2, 3, 1);
        assert!(!run.move_next(), "already on the last line");
        assert_eq!(run.current_line(), 2, "and it did not move");

        let mut top = VerticalCaretMovementRun::new(0.0, 0, 3, 1);
        assert!(!top.move_previous());
        assert_eq!(top.current_line(), 0);
    }

    #[test]
    fn a_run_goes_invalid_when_the_text_is_laid_out_again() {
        // A run that kept going would be indexing lines that no longer exist.
        let mut run = VerticalCaretMovementRun::new(100.0, 0, 5, 7);
        assert!(run.is_valid(7));
        assert!(run.move_next());

        assert!(!run.is_valid(8), "the layout was recomputed");
        assert!(!run.move_next(), "and it stops moving");
        assert!(
            !run.is_valid(7),
            "and stays invalid even if the revision came back"
        );
    }

    #[test]
    fn a_column_past_the_end_of_every_line_lands_at_each_end() {
        let run = VerticalCaretMovementRun::new(1000.0, 0, 2, 1);
        assert_eq!(run.offset_in_line(30.0), 30.0);
        assert_eq!(
            run.offset_in_line(0.0),
            0.0,
            "an empty line is its own start"
        );
    }
}

#[cfg(test)]
mod selection_theme_tests {
    use super::*;
    use crate::component_themes::{
        ResolvedTextSelection, TextSelectionTheme, TextSelectionThemeData,
    };
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component, provide};

    struct Reader {
        has_error: bool,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedTextSelection>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(DefaultSelectionStyle::of(context, self.has_error));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(
        style: Option<DefaultSelectionStyle>,
        data: TextSelectionThemeData,
        has_error: bool,
    ) -> ResolvedTextSelection {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let reader = component(Reader {
            has_error,
            seen: std::rc::Rc::clone(&seen),
        });
        let inner = match style {
            Some(style) => provide(style, reader),
            None => reader,
        };
        let mut tree = ElementTree::new();
        tree.rebuild(TextSelectionTheme::new(data, inner));
        seen.borrow_mut().take().expect("built once")
    }

    fn scheme() -> crate::color_scheme::ColorScheme {
        crate::theme::ThemeData::fallback().color_scheme
    }

    const MINE: Color = Color::argb(0xFF, 0x31, 0x41, 0x59);

    #[test]
    fn an_error_cursor_is_not_negotiable() {
        // Upstream puts the error *outside* the chain rather than at the top of
        // it: a caller who set a cursor colour does not keep it while the field
        // is refusing what was typed. A field that looks the same wrong as
        // right is worse than an ugly one.
        let mut data = TextSelectionThemeData::new();
        data.cursor_color = Some(MINE);
        let style = DefaultSelectionStyle::new().with_cursor_color(MINE);

        assert_eq!(resolve(Some(style), data.clone(), false).cursor, MINE);
        assert_eq!(
            resolve(Some(style), data, true).cursor,
            scheme().error,
            "and neither the widget nor the theme gets a say"
        );
    }

    #[test]
    fn a_selection_is_not_recoloured_by_an_error() {
        // A selection is the reader's own doing; recolouring it would be
        // blaming them for the error.
        let plain = resolve(None, TextSelectionThemeData::new(), false);
        let wrong = resolve(None, TextSelectionThemeData::new(), true);
        assert_eq!(plain.selection, wrong.selection);
        assert_ne!(plain.cursor, wrong.cursor);
    }

    #[test]
    fn the_selection_is_the_cursors_colour_at_forty_per_cent() {
        // Not a colour of its own. A selection has to be visible *through* --
        // the text under it must stay readable -- so it is the same hue said
        // quietly rather than a second colour competing.
        let resolved = resolve(None, TextSelectionThemeData::new(), false);
        assert_eq!(resolved.cursor, scheme().primary);
        assert_eq!(resolved.selection.red(), scheme().primary.red());
        assert_eq!(resolved.selection.alpha(), 102, "forty per cent of 255");
    }

    #[test]
    fn a_handle_is_solid_where_the_selection_behind_it_is_faint() {
        // A handle is a thing to grab. Falling back to the selection colour
        // would make the one part the reader has to hit the hardest to see.
        let resolved = resolve(None, TextSelectionThemeData::new(), false);
        assert_eq!(resolved.handle, scheme().primary);
        assert_eq!(resolved.handle.alpha(), 255);
        assert_ne!(resolved.handle, resolved.selection);
    }

    #[test]
    fn the_style_beats_the_theme_beats_the_scheme() {
        let mut data = TextSelectionThemeData::new();
        data.cursor_color = Some(Color::argb(0xFF, 1, 2, 3));

        assert_eq!(
            resolve(None, data.clone(), false).cursor,
            Color::argb(0xFF, 1, 2, 3),
            "the theme's"
        );
        assert_eq!(
            resolve(
                Some(DefaultSelectionStyle::new().with_cursor_color(MINE)),
                data,
                false
            )
            .cursor,
            MINE,
            "and the style's over it"
        );
        assert_eq!(
            resolve(None, TextSelectionThemeData::new(), false).cursor,
            scheme().primary,
            "and the scheme when neither said"
        );
    }

    #[test]
    fn each_of_the_three_colours_is_its_own_chain() {
        let mut data = TextSelectionThemeData::new();
        data.selection_handle_color = Some(MINE);
        let resolved = resolve(None, data, false);
        assert_eq!(resolved.handle, MINE);
        assert_eq!(
            resolved.cursor,
            scheme().primary,
            "setting one does not move the others"
        );
        assert_eq!(resolved.selection.alpha(), 102);
    }
}

#[cfg(test)]
mod selection_gesture_rule_tests {
    use super::*;

    const DESKTOP: [TargetPlatform; 3] = [
        TargetPlatform::Linux,
        TargetPlatform::MacOS,
        TargetPlatform::Windows,
    ];
    const MOBILE: [TargetPlatform; 3] = [
        TargetPlatform::Android,
        TargetPlatform::IOS,
        TargetPlatform::Fuchsia,
    ];

    #[test]
    fn the_toolbar_is_for_a_finger_or_a_stylus_and_not_for_a_mouse() {
        // A mouse has a right-click menu and a keyboard; a fingertip has
        // neither, so the controls have to be on screen.
        assert!(TextSelectionGestures::shows_selection_toolbar(
            PointerKind::Touch
        ));
        assert!(TextSelectionGestures::shows_selection_toolbar(
            PointerKind::Stylus
        ));
        assert!(!TextSelectionGestures::shows_selection_toolbar(
            PointerKind::Mouse
        ));
        assert!(!TextSelectionGestures::shows_selection_toolbar(
            PointerKind::Trackpad
        ));
    }

    #[test]
    fn an_unknown_device_counts_as_a_finger() {
        // The safe way round: a toolbar nobody needed is a nuisance,
        // withholding one from someone with no other way to copy is a dead
        // end.
        assert!(TextSelectionGestures::shows_selection_toolbar(
            PointerKind::Unknown
        ));
    }

    #[test]
    fn an_inverted_stylus_is_not_on_upstreams_list() {
        // `kind == null || kind == touch || kind == stylus`, and an inverted
        // stylus is its own kind. Pinned because it is the one that looks like
        // an oversight and might be one -- upstream has an open question about
        // this rule (flutter/flutter#106586) and this is what it does today.
        assert!(!TextSelectionGestures::shows_selection_toolbar(
            PointerKind::InvertedStylus
        ));
    }

    #[test]
    fn the_handles_cannot_disagree_with_the_toolbar() {
        // Upstream assigns one from the other on the next line.
        for kind in [
            PointerKind::Touch,
            PointerKind::Mouse,
            PointerKind::Stylus,
            PointerKind::InvertedStylus,
            PointerKind::Trackpad,
            PointerKind::Unknown,
        ] {
            assert_eq!(
                TextSelectionGestures::shows_selection_handles(kind),
                TextSelectionGestures::shows_selection_toolbar(kind),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_desktop_moves_the_caret_under_the_button_and_a_phone_waits_for_the_lift() {
        // A press on a desktop is the start of a possible drag-select, so the
        // caret has to be at one end of it already. A finger going down might
        // be the beginning of a scroll.
        for platform in DESKTOP {
            assert_eq!(
                TextSelectionGestures::caret_moves_on(platform),
                CaretMovesOn::TapDown,
                "{platform:?}"
            );
        }
        for platform in MOBILE {
            assert_eq!(
                TextSelectionGestures::caret_moves_on(platform),
                CaretMovesOn::TapUp,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn shift_tapping_an_unfocused_field_expands_from_zero_on_both_apple_platforms() {
        // Written twice upstream, once in the macOS arm and once in the iOS
        // one, and nowhere else.
        assert!(
            TextSelectionGestures::shift_tap_expands_from_zero_when_unfocused(TargetPlatform::IOS)
        );
        assert!(
            TextSelectionGestures::shift_tap_expands_from_zero_when_unfocused(
                TargetPlatform::MacOS
            )
        );
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert!(
                !TextSelectionGestures::shift_tap_expands_from_zero_when_unfocused(platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_mouse_on_an_ipad_behaves_like_a_desktop() {
        // The reason for the word edge is the fingertip, not the operating
        // system -- so iOS asks what you touched it with.
        assert_eq!(
            TextSelectionGestures::caret_lands(TargetPlatform::IOS, PointerKind::Touch),
            CaretLands::AtTheWordEdge
        );
        for precise in [
            PointerKind::Mouse,
            PointerKind::Trackpad,
            PointerKind::Stylus,
            PointerKind::InvertedStylus,
        ] {
            assert_eq!(
                TextSelectionGestures::caret_lands(TargetPlatform::IOS, precise),
                CaretLands::Precisely,
                "{precise:?}"
            );
        }
    }

    #[test]
    fn and_a_finger_on_a_mac_still_lands_precisely() {
        // The contrast upstream's own comment draws: macOS places the caret
        // precisely where iOS would go to the word edge. So it is the
        // platform *and* the kind, not either alone.
        assert_eq!(
            TextSelectionGestures::caret_lands(TargetPlatform::MacOS, PointerKind::Touch),
            CaretLands::Precisely
        );
        assert_ne!(
            TextSelectionGestures::caret_lands(TargetPlatform::MacOS, PointerKind::Touch),
            TextSelectionGestures::caret_lands(TargetPlatform::IOS, PointerKind::Touch)
        );
    }

    #[test]
    fn an_unknown_device_on_ios_is_treated_as_a_finger_there_too() {
        // The same way round as the toolbar rule: when in doubt, assume the
        // less precise input.
        assert_eq!(
            TextSelectionGestures::caret_lands(TargetPlatform::IOS, PointerKind::Unknown),
            CaretLands::AtTheWordEdge
        );
    }

    #[test]
    fn every_other_platform_ignores_the_device_kind_entirely() {
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            for kind in [PointerKind::Touch, PointerKind::Mouse, PointerKind::Unknown] {
                assert_eq!(
                    TextSelectionGestures::caret_lands(platform, kind),
                    CaretLands::Precisely,
                    "{platform:?} {kind:?}"
                );
            }
        }
    }
}
