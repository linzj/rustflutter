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
use crate::text_selection_controls::TextSelectionHandleType;

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

/// Upstream's `_isShiftPressed`, and the pair of callbacks that maintain it:
/// `onTapTrackStart` and `onTapTrackReset`.
///
/// ```dart
/// void onTapTrackStart() {
///   _isShiftPressed = HardwareKeyboard.instance.logicalKeysPressed
///       .intersection(<LogicalKeyboardKey>{shiftLeft, shiftRight}).isNotEmpty;
/// }
///
/// void onTapTrackReset() {
///   _isShiftPressed = false;
/// }
/// ```
///
/// # Shift is sampled once, at the start of the tap sequence
///
/// Every shift rule in this file -- [`shift_tap_down`], [`shift_drag_update`],
/// [`GestureHandler::shift_is_usable`] -- takes a `shift_pressed` it does not
/// decide. **This is where that value comes from**, and where it stops being
/// read: the keyboard is asked when the tap *track* begins and not again, so
/// the whole of a double- or triple-tap runs on the answer the first press
/// gave.
///
/// Two consequences the port has to get right, because they read as bugs
/// either way round:
///
/// * A reader who presses shift **after** starting a multi-tap does not get a
///   shift-extend from it. The sequence they began was an ordinary one.
/// * A reader who **lets go** of shift part way through still does. The
///   sequence they began was a shift one, and changing its mind between the
///   second and third tap would select something they never asked for.
///
/// Either could be argued the other way; what cannot be argued is reading the
/// keyboard live and getting a different answer on each tap of one gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TapTrackShift {
    held: bool,
}

impl TapTrackShift {
    pub fn new() -> TapTrackShift {
        TapTrackShift::default()
    }

    /// Upstream's `onTapTrackStart`: ask the keyboard, once.
    ///
    /// `shift_now` is the intersection upstream computes -- **either** shift
    /// key counts, which is why upstream tests a set of two rather than one
    /// key.
    pub fn track_started(&mut self, shift_now: bool) {
        self.held = shift_now;
    }

    /// Upstream's `onTapTrackReset`: the sequence is over, and the next one
    /// starts from no.
    ///
    /// Not "ask the keyboard again" -- a reset that re-sampled would leave
    /// shift held between sequences for a reader who never let go, and the
    /// next unrelated tap would extend a selection.
    pub fn track_reset(&mut self) {
        self.held = false;
    }

    /// What the shift rules should be told, for every tap of this sequence.
    pub fn is_held(&self) -> bool {
        self.held
    }
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

/// The gestures that write `_shouldShowSelectionToolbar` and
/// `_shouldShowSelectionHandles`. Every other handler leaves both alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionGesture {
    /// `onTapDown`.
    TapDown,
    /// `onDragSelectionStart`.
    DragSelectionStart,
    /// `onSecondaryTapDown` -- a right-click, or a long press on a touch
    /// screen, asking for the context menu.
    SecondaryTapDown,
    /// `onForcePressStart`, which exists only where the screen reports
    /// pressure.
    ForcePressStart,
}

/// What a gesture leaves the two flags saying. `None` is upstream not
/// assigning that field at all, which leaves the previous gesture's answer
/// standing -- a different thing from assigning `false`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionUiFlags {
    pub toolbar: Option<bool>,
    pub handles: Option<bool>,
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

    /// Whether the handles follow the toolbar **for a primary tap or a drag**,
    /// where upstream assigns one from the other on the next line:
    ///
    /// ```dart
    /// _shouldShowSelectionToolbar =
    ///     kind == null || kind == PointerDeviceKind.touch || kind == PointerDeviceKind.stylus;
    /// _shouldShowSelectionHandles = _shouldShowSelectionToolbar;
    /// ```
    ///
    /// **This doc used to say they "cannot disagree", full stop.** They can,
    /// and [`TextSelectionGestures::flags_for`] is where. The sentence was
    /// true of the two gestures it was read from and not of the class.
    pub fn shows_selection_handles(kind: PointerKind) -> bool {
        TextSelectionGestures::shows_selection_toolbar(kind)
    }

    /// What a gesture leaves the two flags saying.
    ///
    /// Upstream keeps `_shouldShowSelectionToolbar` and
    /// `_shouldShowSelectionHandles` as separate fields and writes them at
    /// four places. Three of those write the same value into both, which is
    /// why reading any one of them suggests the two are one thing. The fourth
    /// is `onSecondaryTapDown`:
    ///
    /// ```dart
    /// _shouldShowSelectionToolbar = true;
    /// _shouldShowSelectionHandles =
    ///     details.kind == null ||
    ///     details.kind == PointerDeviceKind.touch ||
    ///     details.kind == PointerDeviceKind.stylus;
    /// ```
    ///
    /// **A right-click always earns a toolbar and only sometimes earns
    /// handles.** That is the whole reason they are two fields: the toolbar is
    /// the context menu the secondary tap asked for, and a mouse that can
    /// summon it has a pointer precise enough to select with -- draggable
    /// handles would be furniture in the way. A finger long-pressing to the
    /// same menu still needs them.
    ///
    /// `onForcePressStart` is the other asymmetry, and a quieter one: it sets
    /// the toolbar flag to `true` and **does not touch the handles flag at
    /// all**, so the handles keep whatever the tap that preceded the press
    /// decided. A force press only exists on a pressure-sensitive screen, so
    /// that earlier decision was a finger's and the handles are already on.
    pub fn flags_for(gesture: SelectionGesture, kind: PointerKind) -> SelectionUiFlags {
        let by_kind = TextSelectionGestures::shows_selection_toolbar(kind);
        match gesture {
            SelectionGesture::TapDown | SelectionGesture::DragSelectionStart => SelectionUiFlags {
                toolbar: Some(by_kind),
                handles: Some(by_kind),
            },
            SelectionGesture::SecondaryTapDown => SelectionUiFlags {
                toolbar: Some(true),
                handles: Some(by_kind),
            },
            SelectionGesture::ForcePressStart => SelectionUiFlags {
                toolbar: Some(true),
                handles: None,
            },
        }
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
    /// Upstream's Android arm of `onTapDown`: whether a pointer going down
    /// should *ask* about starting stylus handwriting.
    ///
    /// Three gates, and this answers the first two -- the widget's
    /// `stylusHandwritingEnabled` flag and the pointer kind. The third is
    /// `Scribe.isFeatureAvailable()`, a channel round trip, and it is why
    /// [`TextSelectionGestures::stylus_handwriting_starts`] is a separate
    /// question.
    ///
    /// **Android alone.** iOS has Scribble and reaches it another way; the
    /// other four arms of the switch do not mention a stylus.
    ///
    /// **Both stylus kinds.** An inverted stylus is the same instrument turned
    /// round, and upstream's `switch` lists it beside the ordinary one.
    pub fn asks_about_stylus_handwriting(
        platform: TargetPlatform,
        kind: PointerKind,
        stylus_handwriting_enabled: bool,
    ) -> bool {
        platform == TargetPlatform::Android
            && stylus_handwriting_enabled
            && matches!(kind, PointerKind::Stylus | PointerKind::InvertedStylus)
    }

    /// The third gate: what the platform said.
    ///
    /// **Nothing happens on the frame the stylus goes down.** The tap-down
    /// handler asks and returns; the caret moves later, from inside the
    /// callback, and only if the answer was yes. A port that treats this as a
    /// synchronous three-way `&&` moves the caret a frame early and moves it
    /// on a device that cannot do handwriting at all.
    pub fn stylus_handwriting_starts(asked: bool, feature_is_available: bool) -> bool {
        asked && feature_is_available
    }

    /// The channel method this arm asks with -- upstream's
    /// `Scribe.isFeatureAvailable()`, *is this build capable of it*, and not
    /// `Scribe.isStylusHandwritingAvailable`, *is it available right now*.
    pub const STYLUS_HANDWRITING_GATE: &'static str =
        crate::services::system_channels::Scribe::IS_FEATURE_AVAILABLE;

    /// The cause the selection change carries once handwriting starts.
    ///
    /// Not `tap`: the change came from the platform's recogniser rather than
    /// from the finger, so anything downstream that keys off the cause can
    /// tell the two apart.
    pub fn stylus_handwriting_cause() -> crate::services::text_input::SelectionChangedCause {
        crate::services::text_input::SelectionChangedCause::StylusHandwriting
    }

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

    /// Whether `buildGestureDetector` hands the detector a callback for
    /// `handler` at all.
    ///
    /// Upstream wires nineteen of them and **exactly two are conditional**:
    ///
    /// ```dart
    /// onForcePressStart: delegate.forcePressEnabled ? onForcePressStart : null,
    /// onForcePressEnd:   delegate.forcePressEnabled ? onForcePressEnd   : null,
    /// ```
    ///
    /// # The two delegate flags are enforced at two different layers
    ///
    /// `selectionEnabled` is checked **inside** each handler -- every one of
    /// them opens with `if (!delegate.selectionEnabled) { return; }`. Force
    /// press is checked **at the wiring**, and is not connected at all.
    ///
    /// That is not a style choice, and the difference has a reason at each
    /// end. A null callback tells the gesture detector not to build the
    /// recognizer, so a field that does not want force press puts nothing in
    /// the arena for it; a recognizer that merely returned early would still
    /// be competing for the pointer, and could still win it away from the ones
    /// that do want it. Whereas a field with selection turned off still has to
    /// take a tap -- tapping it moves focus and opens the keyboard -- so those
    /// recognizers must stay in the arena and decline the work individually.
    ///
    /// # And it is why the force-press asserts can never fire
    ///
    /// ```dart
    /// void onForcePressStart(ForcePressDetails details) {
    ///   assert(delegate.forcePressEnabled);
    /// ```
    ///
    /// The only caller is a wiring that exists only when the flag is true. The
    /// assert is a statement about the wiring rather than a runtime check, and
    /// a port that moved it to an early `return` would be answering a question
    /// nobody asks.
    pub fn wires(&self, handler: GestureHandler) -> bool {
        match handler {
            GestureHandler::ForcePressStart | GestureHandler::ForcePressEnd => {
                self.delegate.force_press_enabled()
            }
            _ => true,
        }
    }
}

/// The callbacks `buildGestureDetector` passes on, named as upstream names
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureHandler {
    TapTrackStart,
    TapTrackReset,
    TapDown,
    ForcePressStart,
    ForcePressEnd,
    SecondaryTap,
    SecondaryTapDown,
    SingleTapUp,
    SingleTapCancel,
    UserTap,
    SingleLongTapStart,
    SingleLongTapMoveUpdate,
    SingleLongTapEnd,
    SingleLongTapCancel,
    DoubleTapDown,
    TripleTapDown,
    DragSelectionStart,
    DragSelectionUpdate,
    DragSelectionEnd,
}

impl GestureHandler {
    /// All nineteen, so a test can walk them rather than sample them.
    ///
    /// The count is the assertion. `buildGestureDetector` passes nineteen
    /// callbacks -- `onUserTapAlwaysCalled`, `behavior`, `child` and `key` are
    /// the arguments that are not one -- and a list that quietly lost a row
    /// would leave every walk below still passing, having simply stopped
    /// looking at it.
    pub const ALL: [GestureHandler; 19] = [
        GestureHandler::TapTrackStart,
        GestureHandler::TapTrackReset,
        GestureHandler::TapDown,
        GestureHandler::ForcePressStart,
        GestureHandler::ForcePressEnd,
        GestureHandler::SecondaryTap,
        GestureHandler::SecondaryTapDown,
        GestureHandler::SingleTapUp,
        GestureHandler::SingleTapCancel,
        GestureHandler::UserTap,
        GestureHandler::SingleLongTapStart,
        GestureHandler::SingleLongTapMoveUpdate,
        GestureHandler::SingleLongTapEnd,
        GestureHandler::SingleLongTapCancel,
        GestureHandler::DoubleTapDown,
        GestureHandler::TripleTapDown,
        GestureHandler::DragSelectionStart,
        GestureHandler::DragSelectionUpdate,
        GestureHandler::DragSelectionEnd,
    ];
}

/// Upstream `TextAffinity` (`dart:ui`): which side of an ambiguous offset a
/// position means.
///
/// One offset can be two places on the screen. Where a line **wraps**, the
/// offset at the break is both the end of the first line and the start of the
/// second; in bidirectional text the boundary between an LTR and an RTL run is
/// the same. Upstream's own note is worth keeping: this is only about wrapping
/// and about direction changes, **not** about explicit newlines -- a `\n` puts
/// the offset in one place and there is nothing to disambiguate.
///
/// The port has carried the string `TextAffinity.downstream` over the wire
/// since the text-input work and never had the type, so nothing could say
/// which of the two a position meant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAffinity {
    /// Towards the beginning of the string: at a wrap, the end of the first
    /// line.
    Upstream,
    /// Towards the end of the string: at a wrap, the start of the second line.
    ///
    /// Upstream's default everywhere, and this port's wire messages have
    /// always said so.
    #[default]
    Downstream,
}

impl TextAffinity {
    pub const ALL: [TextAffinity; 2] = [TextAffinity::Upstream, TextAffinity::Downstream];

    /// The name the engine's channel uses, which this crate was already
    /// sending as a literal.
    pub fn as_wire(self) -> &'static str {
        match self {
            TextAffinity::Upstream => "TextAffinity.upstream",
            TextAffinity::Downstream => "TextAffinity.downstream",
        }
    }
}

/// What a single tap on a focused field does, once the platform has decided
/// the tap is a touch one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapOutcome {
    /// The tap landed on the selection the field already had: show the toolbar
    /// if it is down, hide it if it is up.
    ToggleToolbar,
    /// The tap landed somewhere else: move the caret to the nearest word edge.
    /// What happens to the toolbar afterwards depends on whether that moved
    /// anything -- see [`after_selecting_the_word_edge`].
    SelectWordEdge,
    /// The word under the tap is misspelled, which upstream checks before
    /// anything else.
    SelectWordAndOfferSpelling,
}

/// What the toolbar does after a tap that chose [`TapOutcome::SelectWordEdge`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterWordEdge {
    /// The word edge was already where the caret was, so the tap was a second
    /// tap in the same place and means "show me the toolbar".
    ToggleToolbar,
    /// The tap moved the caret, so any toolbar belongs to where it used to be.
    HideToolbar,
}

/// Upstream's `_positionWasOnSelectionExclusive`: strictly **inside** the
/// selection, touching neither end.
///
/// The ends are where the handles are. A tap on one is aimed at the handle, or
/// at putting the caret out of the selection -- not at the selection itself.
pub fn position_was_on_selection_exclusive(selection: (i32, i32), offset: i32) -> bool {
    let (start, end) = ordered(selection);
    start < offset && end > offset
}

/// Upstream's `_positionWasOnSelectionInclusive`: inside **or on either end**.
///
/// The pair only makes sense together. A collapsed selection is a caret, and
/// "on the caret" can only mean *at* it -- an exclusive test on a zero-width
/// selection is false for every offset there is, so the collapsed case would
/// never fire.
pub fn position_was_on_selection_inclusive(selection: (i32, i32), offset: i32) -> bool {
    let (start, end) = ordered(selection);
    start <= offset && end >= offset
}

fn ordered(selection: (i32, i32)) -> (i32, i32) {
    (selection.0.min(selection.1), selection.0.max(selection.1))
}

/// Upstream's single-tap-up branch for a touch on iOS, which is the longest
/// piece of reasoning in the file and is quoted in its own comment there.
///
/// ```dart
/// } else if (((_positionWasOnSelectionExclusive(textPosition) && !previousSelection.isCollapsed)
///          || (_positionWasOnSelectionInclusive(textPosition) && previousSelection.isCollapsed
///              && isAffinityTheSame && !renderEditable.readOnly))
///         && renderEditable.hasFocus) {
///   editableText.toggleToolbar(false);
/// } else {
///   renderEditable.selectWordEdge(cause: SelectionChangedCause.tap);
///   ...
/// }
/// ```
///
/// Every term earns its place.
///
/// * **Exclusive for a range, inclusive for a caret.** Tapping inside a
///   highlighted run toggles the toolbar; tapping exactly on an end does not,
///   because that is where the handle is. A caret has no inside, so the
///   collapsed case has to be inclusive or it could never fire.
/// * **The affinity has to match.** At a line wrap one offset is two places.
///   If the reader's tap means the other one, the caret should move to the
///   following line rather than the toolbar appearing where they did not tap.
/// * **Not read-only.** A read-only field's caret is not something the reader
///   put there, so a tap on it is a request to select rather than a second tap
///   on their own caret.
/// * **Focused.** An unfocused field's first tap is about taking focus and
///   placing the caret, whatever it landed on.
/// * **Misspelled first.** Checked before all of it: a tap on a misspelling is
///   a request for the suggestions, and it does not matter what the selection
///   was.
pub fn tap_outcome(
    misspelled: bool,
    selection: (i32, i32),
    offset: i32,
    affinity_is_the_same: bool,
    read_only: bool,
    has_focus: bool,
) -> TapOutcome {
    if misspelled {
        return TapOutcome::SelectWordAndOfferSpelling;
    }
    let (start, end) = ordered(selection);
    let collapsed = start == end;
    let on_the_selection = (position_was_on_selection_exclusive(selection, offset) && !collapsed)
        || (position_was_on_selection_inclusive(selection, offset)
            && collapsed
            && affinity_is_the_same
            && !read_only);
    if on_the_selection && has_focus {
        TapOutcome::ToggleToolbar
    } else {
        TapOutcome::SelectWordEdge
    }
}

/// Upstream's `else` arm, once the word edge has been selected.
///
/// The toolbar comes up only when the tap changed **nothing** -- the reader
/// tapped their own caret a second time. A tap that moved the caret hides the
/// toolbar instead, because a toolbar belongs to the selection it was raised
/// for.
pub fn after_selecting_the_word_edge(
    selection_changed: bool,
    read_only: bool,
    has_focus: bool,
) -> AfterWordEdge {
    if !selection_changed && has_focus && !read_only {
        AfterWordEdge::ToggleToolbar
    } else {
        AfterWordEdge::HideToolbar
    }
}

/// Upstream's `_extendSelection`: keep the base where it is and move the
/// extent to the tap.
///
/// The base is the end the reader started from; the extent is the loose one.
/// So this drags the loose end and never touches the anchor, which is the
/// plain reading of "extend".
///
/// A selection is `(base, extent)` here, in that order, and the order matters
/// -- these two functions are the reason it cannot be reduced to a sorted
/// pair.
pub fn extend_selection(selection: (i32, i32), tapped: i32) -> (i32, i32) {
    (selection.0, tapped)
}

/// Upstream's `_expandSelection`: **re-choose which end is loose**, so that
/// the end further from the tap stays put.
///
/// ```dart
/// final bool baseIsCloser =
///     (tappedPosition.offset - selection.baseOffset).abs()
///   < (tappedPosition.offset - selection.extentOffset).abs();
/// selection.copyWith(
///   baseOffset: baseIsCloser ? selection.extentOffset : selection.baseOffset,
///   extentOffset: tappedPosition.offset,
/// );
/// ```
///
/// # Where the two part company
///
/// Tapping **beyond** the loose end they agree. Tapping **past the anchor**
/// they do not, and that is the case the name is about. With `4..9` selected
/// and a shift-click at 1:
///
/// * extend moves the extent to 1, giving `4..1` -- the run from 4 to 9 is
///   gone, and the reader has lost the selection they were adding to;
/// * expand notices 1 is nearer the base than the extent, anchors on the
///   **extent** instead, and gives `9..1` -- the original run is still inside
///   it.
///
/// So expand never drops the far boundary. That is the whole difference, and
/// it is why the Apple platforms use it: shift-clicking back past where you
/// started grows the selection there rather than starting a new one.
pub fn expand_selection(selection: (i32, i32), tapped: i32) -> (i32, i32) {
    let (base, extent) = selection;
    let base_is_closer = (tapped - base).abs() < (tapped - extent).abs();
    let anchor = if base_is_closer { extent } else { base };
    (anchor, tapped)
}

/// What a shift-click does on tap **down**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShiftTapDown {
    /// Upstream's `_expandSelection` from the selection the field already has.
    Expand,
    /// The same, but from a caret at offset zero. macOS only, and only when
    /// the field was not focused -- upstream's comment: *"On macOS, a
    /// shift-tapped unfocused field expands from 0, not from the previous
    /// selection."*
    ExpandFromTheStart,
    /// Upstream's `_extendSelection`.
    Extend,
    /// Nothing happens here. The mobile platforms set the selection on tap
    /// **up**, so tap down is not where the answer is.
    Nothing,
}

/// Upstream's `onTapDown`, reduced to what the shift key decides.
///
/// `has_selection` is upstream's
/// `renderEditable.selection?.baseOffset != null`, and its own comment says
/// what it is for: *"It is impossible to extend the selection when the shift
/// key is pressed, if the renderEditable.selection is invalid."* There is
/// nothing to extend from.
///
/// The three groups:
///
/// * **macOS** expands, and from zero when the field had no focus. A
///   shift-click into a cold field selects from the start of the text to the
///   click, which is that platform's convention in every text view it has.
/// * **Linux and Windows** extend.
/// * **Android, Fuchsia and iOS** do nothing on the way down; those decide on
///   the way up.
pub fn shift_tap_down(
    platform: crate::editable_text::TargetPlatform,
    shift_pressed: bool,
    has_selection: bool,
    has_focus: bool,
) -> ShiftTapDown {
    use crate::editable_text::TargetPlatform;
    if !shift_pressed || !has_selection {
        return ShiftTapDown::Nothing;
    }
    match platform {
        TargetPlatform::MacOS => {
            if has_focus {
                ShiftTapDown::Expand
            } else {
                ShiftTapDown::ExpandFromTheStart
            }
        }
        TargetPlatform::Linux | TargetPlatform::Windows => ShiftTapDown::Extend,
        TargetPlatform::Android | TargetPlatform::Fuchsia | TargetPlatform::IOS => {
            ShiftTapDown::Nothing
        }
    }
}

/// What the beginning of a drag does to the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragStartSelects {
    /// Nothing -- and on several paths that is the answer rather than an
    /// oversight. See [`drag_selection_start`].
    Nothing,
    /// Shift was held: [`expand_selection`] from where the selection was.
    Expand,
    /// Shift was held: [`extend_selection`].
    Extend,
    /// Put the caret where the drag began.
    CaretAtTheFinger,
}

/// Everything upstream's `onDragSelectionStart` decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragSelectionStart {
    pub selects: DragStartSelects,
    /// Only one path raises it, and it is not the one a desktop takes.
    pub shows_magnifier: bool,
    /// Whether `_shouldShowSelectionToolbar` and `_shouldShowSelectionHandles`
    /// are set from the pointer kind. Both are set together and to the same
    /// value; upstream assigns the second from the first.
    pub sets_the_overlay_flags: bool,
}

/// Upstream's `onDragSelectionStart`.
///
/// # A double tap that becomes a drag keeps its words
///
/// ```dart
/// if (_getEffectiveConsecutiveTapCount(details.consecutiveTapCount) > 1) {
///   // Do not set the selection on a consecutive tap and drag.
///   return;
/// }
/// ```
///
/// The second tap already selected a word, and the drag that follows grows
/// the selection word by word. Placing a caret here would throw that away at
/// the very moment the reader started to drag.
///
/// Note where the return sits: **after** the flags and the drag-start state
/// are recorded. Those are needed by the update that follows whether or not
/// this call sets a selection.
///
/// # A finger does not start a drag-selection the way a mouse does
///
/// * **Desktop (Linux, macOS, Windows)** places the caret, whatever the
///   pointer is. There is nothing else a drag on a desktop could mean.
/// * **Android and Fuchsia** place the caret for a mouse or a trackpad, and
///   for a finger **only in a field that already has focus** -- and that is
///   the one path in this whole method that raises the magnifier.
/// * **iOS** places the caret for a mouse or a trackpad and does **nothing at
///   all** for a finger. Upstream's comment on the Android branch says "For
///   Android, Fuchsia, and iOS platforms, a touch drag does not initiate
///   unless the editable has focus", but iOS's own touch case is empty: there
///   is no focus test there because there is no path there. The code is what
///   is ported; the comment is a sentence about three platforms sitting over
///   a branch that serves two.
///
/// A stylus starts nothing anywhere, on any platform, even though it sets the
/// overlay flags on the way past.
///
/// # One thing this port cannot say
///
/// Upstream's `details.kind` is nullable, and `null` is a **third** answer
/// next to `PointerDeviceKind.unknown`: on Android and Fuchsia `unknown`
/// takes the focus-gated touch path and `null` takes none. This crate's
/// [`PointerKind`] has no null, so [`PointerKind::Unknown`] is upstream's
/// `unknown` and the null case has nowhere to live. Written down rather than
/// smoothed over.
pub fn drag_selection_start(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    kind: PointerKind,
    shift_pressed: bool,
    has_valid_selection: bool,
    consecutive_taps: u32,
    has_focus: bool,
) -> DragSelectionStart {
    use crate::editable_text::TargetPlatform;
    if !selection_enabled {
        return DragSelectionStart {
            selects: DragStartSelects::Nothing,
            shows_magnifier: false,
            sets_the_overlay_flags: false,
        };
    }
    let nothing = DragSelectionStart {
        selects: DragStartSelects::Nothing,
        shows_magnifier: false,
        sets_the_overlay_flags: true,
    };
    if consecutive_taps > 1 {
        return nothing;
    }
    if shift_pressed && has_valid_selection {
        return DragSelectionStart {
            selects: match platform {
                TargetPlatform::IOS | TargetPlatform::MacOS => DragStartSelects::Expand,
                _ => DragStartSelects::Extend,
            },
            ..nothing
        };
    }
    let caret = DragSelectionStart {
        selects: DragStartSelects::CaretAtTheFinger,
        ..nothing
    };
    let precise = matches!(kind, PointerKind::Mouse | PointerKind::Trackpad);
    match platform {
        TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Windows => caret,
        TargetPlatform::IOS => {
            if precise {
                caret
            } else {
                nothing
            }
        }
        TargetPlatform::Android | TargetPlatform::Fuchsia => {
            if precise {
                caret
            } else if matches!(kind, PointerKind::Touch | PointerKind::Unknown) && has_focus {
                DragSelectionStart {
                    shows_magnifier: true,
                    ..caret
                }
            } else {
                nothing
            }
        }
    }
}

/// What upstream's `onDragSelectionUpdate` does on the path where shift **is**
/// held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShiftDragUpdate {
    /// `_extendSelection(globalPosition)` -- the loose end follows the finger
    /// and the anchor stays where it was.
    Extend,
    /// The selection pivots to the far end of the one the drag started from:
    /// `(base: dragStart.extent, extent: nextExtent)`.
    PivotToTheFarEnd,
    /// It pivots back: `(base: dragStart.base, extent: nextExtent)`.
    PivotBack,
}

/// Upstream's shift branch of `onDragSelectionUpdate`, and **the whole of it
/// applies to two platforms only**:
///
/// ```dart
/// if (_dragStartSelection!.isCollapsed ||
///     (defaultTargetPlatform != TargetPlatform.iOS &&
///         defaultTargetPlatform != TargetPlatform.macOS)) {
///   return _extendSelection(details.globalPosition, SelectionChangedCause.drag);
/// }
/// // If the drag inverts the selection, Mac and iOS revert to the initial
/// // selection.
/// ```
///
/// Two conditions, and either one alone sends the drag down the ordinary path:
/// the drag must have **started from a range** rather than a caret, and the
/// platform must be Apple.
///
/// # What "inverts" means
///
/// Not "the loose end passed the fixed end" -- it is measured against the
/// **base the drag started with**, whichever direction that original selection
/// ran in:
///
/// ```dart
/// final bool isShiftTapDragSelectionForward =
///     _dragStartSelection!.baseOffset < _dragStartSelection!.extentOffset;
/// final bool isInverted = isShiftTapDragSelectionForward
///     ? nextExtent.offset < _dragStartSelection!.baseOffset
///     : nextExtent.offset > _dragStartSelection!.baseOffset;
/// ```
///
/// So a selection made right-to-left inverts by going *right* past its base,
/// and one made left-to-right inverts by going *left* past it. A port that
/// tested `nextExtent < base` unconditionally would have the rule backwards
/// for every selection the reader made backwards.
///
/// # And what it does about it
///
/// Ordinary extending would drop everything on the far side of the crossing
/// and grow a fresh range from there. Apple instead **keeps the original
/// selection whole and pivots**: the anchor becomes the original selection's
/// *other* end, so dragging back past the start swings the range around rather
/// than shrinking it to nothing and re-growing. Cross back and it pivots back.
///
/// # The two guards are what make it idempotent
///
/// `selection.baseOffset == _dragStartSelection!.baseOffset` on the first arm
/// and `!=` on the second. They fire on the **transition** and not on every
/// move event: once pivoted, the base no longer equals the original base, so
/// the first arm stops matching and further movement is ordinary extending.
///
/// `already_pivoted` is that comparison, and `next_extent` is where the finger
/// is in the text.
pub fn shift_drag_update(
    platform: crate::editable_text::TargetPlatform,
    drag_start_selection: (i32, i32),
    next_extent: i32,
    already_pivoted: bool,
) -> ShiftDragUpdate {
    use crate::editable_text::TargetPlatform;
    let (base, extent) = drag_start_selection;
    let apple = matches!(platform, TargetPlatform::IOS | TargetPlatform::MacOS);
    if base == extent || !apple {
        return ShiftDragUpdate::Extend;
    }
    let forward = base < extent;
    let inverted = if forward {
        next_extent < base
    } else {
        next_extent > base
    };
    if inverted && !already_pivoted {
        ShiftDragUpdate::PivotToTheFarEnd
    } else if !inverted && next_extent != base && already_pivoted {
        ShiftDragUpdate::PivotBack
    } else {
        ShiftDragUpdate::Extend
    }
}

/// The selection [`shift_drag_update`] asks for, for the two pivoting answers.
pub fn shift_drag_selection(
    outcome: ShiftDragUpdate,
    drag_start_selection: (i32, i32),
    next_extent: i32,
) -> Option<(i32, i32)> {
    let (base, extent) = drag_start_selection;
    match outcome {
        ShiftDragUpdate::PivotToTheFarEnd => Some((extent, next_extent)),
        ShiftDragUpdate::PivotBack => Some((base, next_extent)),
        ShiftDragUpdate::Extend => None,
    }
}

/// What upstream's `onDragSelectionEnd` leaves behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragSelectionEnd {
    pub shows_toolbar: bool,
    /// `_dragStartSelection = null`, and **only when shift was held**.
    pub clears_the_drag_start_selection: bool,
    pub hides_magnifier: bool,
    /// Always `false`, and it is the difference from
    /// [`long_press_finish`] worth having a field for. See below.
    pub resets_the_drag_anchors: bool,
}

/// Upstream's `onDragSelectionEnd`, which is three lines and three different
/// conditions:
///
/// ```dart
/// void onDragSelectionEnd(TapDragEndDetails details) {
///   if (_shouldShowSelectionToolbar &&
///       _getEffectiveConsecutiveTapCount(details.consecutiveTapCount) == 2) {
///     editableText.showToolbar();
///   }
///   if (_isShiftPressed) {
///     _dragStartSelection = null;
///   }
///   _hideMagnifierIfSupportedByPlatform();
/// }
/// ```
///
/// # The menu belongs to the double-tap drag alone
///
/// **`== 2`, not `>= 2`.** A drag that grew the selection word by word ends
/// with the toolbar; one that placed a caret does not, and neither does the
/// triple-tap drag that selected whole paragraphs. Reading the condition as
/// "two or more" would put a menu up after a paragraph drag, which upstream
/// does not do.
///
/// The flag is still asked first, so a mouse that dragged out words gets no
/// toolbar -- it has a right-click menu instead. That is the flag
/// [`TextSelectionGestures::flags_for`] describes.
///
/// # The drag start selection is cleared only on the path that read it
///
/// `_dragStartSelection` is taken by every `onDragSelectionStart`, and read by
/// exactly one thing: the shift branch of [`shift_drag_update`]. So it is
/// released on exactly that path. A plain drag leaves its value behind, which
/// is harmless because the next drag's start overwrites it -- upstream is
/// tidying the thing it borrowed, not clearing state that would be wrong.
///
/// # And it does *not* zero the scroll anchors, where the long press does
///
/// [`long_press_finish`] sets both `_dragStartViewportOffset` and
/// `_dragStartScrollOffset` back to zero. This does not, and the difference is
/// not that one gesture needs it and the other does not: **both starts take
/// both anchors afresh**, so neither reading survives into a gesture that
/// would misuse it.
///
/// The one place the long press's zeroing has anything to do is a field that
/// does not select, where `onSingleLongTapStart` returns before it reaches the
/// two assignments -- so the press takes no anchors and the end puts them back
/// to zero rather than leaving the last gesture's. Ported as written, with the
/// asymmetry named rather than smoothed away.
pub fn drag_selection_end(
    should_show_selection_toolbar: bool,
    effective_consecutive_tap_count: u32,
    shift_pressed: bool,
) -> DragSelectionEnd {
    DragSelectionEnd {
        shows_toolbar: should_show_selection_toolbar && effective_consecutive_tap_count == 2,
        clears_the_drag_start_selection: shift_pressed,
        hides_magnifier: true,
        resets_the_drag_anchors: false,
    }
}

/// What a drag does to the selection, once the corrections of
/// [`drag_anchor_correction`] have been applied to its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragSelects {
    /// Upstream returns without touching the selection. Several paths do, and
    /// on each of them it is the answer rather than an oversight.
    Nothing,
    /// `selectPositionAt(from: anchor, to: finger)` -- a caret drag that marks
    /// out a range from where the drag began.
    RangeFromTheAnchor,
    /// `selectPositionAt(from: finger)` with **no `to`** -- the caret follows
    /// the finger and nothing is selected. Not the same as the above with a
    /// short range: there is no anchor at all.
    CaretAtTheFingerOnly,
    /// `selectWordsInRange` -- the range grows a whole word at a time.
    WordsFromTheAnchor,
    /// `_selectParagraphsInRange`.
    ParagraphsFromTheAnchor,
    /// `_selectLinesInRange`.
    LinesFromTheAnchor,
}

/// Everything upstream's `onDragSelectionUpdate` decides on the path where
/// shift is **not** held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragSelectionUpdate {
    pub selects: DragSelects,
    pub shows_magnifier: bool,
}

/// Upstream's `onDragSelectionUpdate`, minus the shift branch.
///
/// The question it answers is "what does dragging select", and the answer
/// depends on three things at once: **how many times you tapped before the
/// drag, what you are dragging with, and which platform you are on.**
///
/// # The granularity comes from the tap count
///
/// One tap and a drag marks out a range; two marks it out a word at a time;
/// three, a paragraph at a time. The count is
/// [`TextSelectionGestureDetector::effective_consecutive_tap_count`], not the
/// raw one, so a fourth tap is a first tap again and the ladder restarts.
///
/// # And Linux drags by the line where every other desktop drags by the paragraph
///
/// ```dart
/// case TargetPlatform.linux:
///   return _selectLinesInRange(...);
/// case TargetPlatform.windows:
/// case TargetPlatform.macOS:
///   return _selectParagraphsInRange(...);
/// ```
///
/// One arm of one switch, and it is the whole difference between selecting a
/// wrapped paragraph and selecting the visual row under the pointer.
///
/// # A finger on Android drags the caret rather than a range
///
/// Everywhere a precise pointer drags, upstream passes both ends --
/// `selectPositionAt(from: anchor, to: finger)`. On Android and Fuchsia a
/// **touch** drag passes only one:
///
/// ```dart
/// renderEditable.selectPositionAt(from: details.globalPosition, cause: ...);
/// return _showMagnifierIfSupportedByPlatform(details.globalPosition);
/// ```
///
/// No anchor, so nothing is selected -- the caret simply follows the finger,
/// with the magnifier over it so the reader can see where it landed. That is
/// [`DragSelects::CaretAtTheFingerOnly`], and reading it as a range of length
/// zero loses the distinction: one of them can grow into a selection by
/// dragging further and the other cannot.
///
/// And it only happens **when the field already has focus**; an unfocused one
/// does nothing at all, because the drag that would focus it is the same
/// gesture and it has not finished.
///
/// # The magnifier follows the finger, not the drag
///
/// At two taps upstream raises it for touch, stylus and unknown and returns
/// early for mouse and trackpad. A magnifier exists to show what a fingertip
/// is covering; a pointer covers nothing.
pub fn drag_selection_update(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    kind: PointerKind,
    effective_consecutive_tap_count: u32,
    has_focus: bool,
) -> DragSelectionUpdate {
    use crate::editable_text::TargetPlatform;
    let nothing = DragSelectionUpdate {
        selects: DragSelects::Nothing,
        shows_magnifier: false,
    };
    if !selection_enabled {
        return nothing;
    }
    let precise = matches!(kind, PointerKind::Mouse | PointerKind::Trackpad);
    let fingerlike = matches!(
        kind,
        PointerKind::Touch
            | PointerKind::Stylus
            | PointerKind::InvertedStylus
            | PointerKind::Unknown
    );

    match effective_consecutive_tap_count {
        2 => DragSelectionUpdate {
            selects: DragSelects::WordsFromTheAnchor,
            // Raised for a fingertip and withheld from a pointer, which
            // covers nothing that needs magnifying.
            shows_magnifier: fingerlike,
        },
        3 => match platform {
            // "Triple tap to drag is not present on these platforms when
            // using non-precise pointer devices at the moment."
            TargetPlatform::Android | TargetPlatform::Fuchsia | TargetPlatform::IOS => {
                if precise {
                    DragSelectionUpdate {
                        selects: DragSelects::ParagraphsFromTheAnchor,
                        ..nothing
                    }
                } else {
                    nothing
                }
            }
            TargetPlatform::Linux => DragSelectionUpdate {
                selects: DragSelects::LinesFromTheAnchor,
                ..nothing
            },
            TargetPlatform::Windows | TargetPlatform::MacOS => DragSelectionUpdate {
                selects: DragSelects::ParagraphsFromTheAnchor,
                ..nothing
            },
        },
        _ => {
            let range = DragSelectionUpdate {
                selects: DragSelects::RangeFromTheAnchor,
                ..nothing
            };
            match platform {
                TargetPlatform::MacOS | TargetPlatform::Linux | TargetPlatform::Windows => range,
                // "With a mouse device, a drag should select the range from
                // the origin of the drag to the current position of the drag.
                // With a touch device, nothing should happen."
                TargetPlatform::IOS => {
                    if precise {
                        range
                    } else {
                        nothing
                    }
                }
                TargetPlatform::Android | TargetPlatform::Fuchsia => {
                    // A stylus counts as precise here and did not at two taps.
                    if precise || matches!(kind, PointerKind::Stylus | PointerKind::InvertedStylus)
                    {
                        range
                    } else if matches!(kind, PointerKind::Touch | PointerKind::Unknown) && has_focus
                    {
                        DragSelectionUpdate {
                            selects: DragSelects::CaretAtTheFingerOnly,
                            shows_magnifier: true,
                        }
                    } else {
                        nothing
                    }
                }
            }
        }
    }
}

/// How the drag after a long press moves the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongPressMove {
    /// The field does not select.
    Nothing,
    /// Grow the selection a word at a time, from the anchor to the finger.
    SelectWordsInRange,
    /// Move the caret to the finger and keep the floating cursor with it.
    MoveCaretAndFloatingCursor,
}

/// Upstream's `onSingleLongTapMoveUpdate`, which is
/// [`long_press_start`]'s decision carried forward.
///
/// The Apple branch does not ask whether the field has focus **now** -- by
/// this point it does, because the press took it. It asks
/// `_longPressStartedWithoutFocus || readOnly`, which is the same question
/// the press answered, preserved. That is the whole reason the flag exists:
/// a drag has to keep doing what the press it grew out of started doing, and
/// the fact that would tell it is gone by the time it runs.
///
/// Everywhere else the drag grows the selection by words, as the press did.
pub fn long_press_move_update(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    started_without_focus: bool,
    read_only: bool,
) -> LongPressMove {
    use crate::editable_text::TargetPlatform;
    if !selection_enabled {
        return LongPressMove::Nothing;
    }
    match platform {
        TargetPlatform::IOS | TargetPlatform::MacOS => {
            if started_without_focus || read_only {
                LongPressMove::SelectWordsInRange
            } else {
                LongPressMove::MoveCaretAndFloatingCursor
            }
        }
        TargetPlatform::Android
        | TargetPlatform::Fuchsia
        | TargetPlatform::Linux
        | TargetPlatform::Windows => LongPressMove::SelectWordsInRange,
    }
}

/// How far the anchor of a long-press drag has to be pulled back, because the
/// text moved underneath it.
///
/// ```dart
/// final editableOffset = renderEditable.maxLines == 1
///     ? Offset(renderEditable.offset.pixels - _dragStartViewportOffset, 0.0)
///     : Offset(0.0, renderEditable.offset.pixels - _dragStartViewportOffset);
/// final Offset scrollableOffset = switch (axisDirectionToAxis(_scrollDirection ?? AxisDirection.left)) {
///   Axis.horizontal => Offset(_scrollPosition - _dragStartScrollOffset, 0.0),
///   Axis.vertical => Offset(0.0, _scrollPosition - _dragStartScrollOffset),
/// };
/// ```
///
/// The anchor is a **global** point recorded when the press began, and the
/// selection runs from there to the finger. Two things can have moved it since:
/// the field scrolled its own text, and the page the field sits on scrolled.
/// Neither moves the finger, so without the correction a drag in a field that
/// auto-scrolls anchors somewhere the reader never pressed, and the selection
/// grows from the wrong end.
///
/// **The axis is not the same question in the two halves.** A single-line
/// field scrolls its text sideways, so its correction is on x; a multi-line
/// one scrolls up and down, so its correction is on y. The surrounding
/// scrollable has its own axis and answers separately -- a single-line field
/// inside a vertically scrolling page corrects on x for one and on y for the
/// other.
///
/// `scroll_axis` is `None` where there is no scrollable above. Upstream falls
/// back to `AxisDirection.left` there, which is horizontal, and it makes no
/// difference: with no scrollable both pixel readings are zero and the
/// correction is `Offset::ZERO` whichever axis it lands on.
///
/// The caller **subtracts** this from the press position, as upstream's
/// `details.globalPosition - details.offsetFromOrigin - editableOffset -
/// scrollableOffset` does.
pub fn drag_anchor_correction(
    single_line: bool,
    field_pixels: f32,
    field_pixels_at_press: f32,
    scroll_pixels: f32,
    scroll_pixels_at_press: f32,
    scroll_axis: Option<crate::render::Axis>,
) -> Offset {
    use crate::render::Axis;
    let field = field_pixels - field_pixels_at_press;
    let scrolled = scroll_pixels - scroll_pixels_at_press;
    let (field_dx, field_dy) = if single_line {
        (field, 0.0)
    } else {
        (0.0, field)
    };
    let (scroll_dx, scroll_dy) = match scroll_axis.unwrap_or(Axis::Horizontal) {
        Axis::Horizontal => (scrolled, 0.0),
        Axis::Vertical => (0.0, scrolled),
    };
    Offset::new(field_dx + scroll_dx, field_dy + scroll_dy)
}

/// What a long press moves the selection to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongPressSelects {
    /// The field does not select, and upstream returns before the switch.
    Nothing,
    /// The word under the finger.
    Word,
    /// The caret, at the finger. Only reached on the Apple platforms, and
    /// only in a field the reader can type in right now.
    CaretAtTheFinger,
}

/// Everything upstream's `onSingleLongTapStart` decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongPressStart {
    pub selects: LongPressSelects,
    /// `Feedback.forLongPress`, which is **not** given on every path.
    pub haptic: bool,
    /// iOS and macOS start a floating cursor when the press lands in a field
    /// that can be typed in.
    pub starts_floating_cursor: bool,
    /// Upstream's `_longPressStartedWithoutFocus`, remembered so that the
    /// drag which follows can be consistent with how the press began.
    pub remembers_it_began_unfocused: bool,
    /// `_showMagnifierIfSupportedByPlatform`, after the switch and so on
    /// every path that reaches it.
    pub shows_magnifier: bool,
}

/// Upstream's `onSingleLongTapStart`.
///
/// # The Apple platforms answer three different ways
///
/// * **Unfocused** -- select the word, remember that the press began without
///   focus, and give **no** haptic. The field is about to take focus and put a
///   keyboard on the screen, which is feedback enough.
/// * **Focused and read-only** -- select the word, **and** give the haptic.
///   This is the one Apple path that buzzes.
/// * **Focused and editable** -- do not select a word at all. Put the caret
///   where the finger is and start the **floating cursor**, which is that
///   platform's gesture for placing a caret precisely. No haptic: the
///   floating cursor is its own feedback, and a buzz would announce a
///   selection that is not happening.
///
/// Everywhere else there is one answer: select the word and buzz.
///
/// So a long press means *select this word* on Android and *let me put the
/// caret here* on a live iOS field, and the port that treats them alike gets
/// one of the two wrong.
///
/// # Why the flag is remembered
///
/// `onSingleLongTapMoveUpdate` reads it: on Apple, dragging after a long
/// press extends **by words** when the press began unfocused or the field is
/// read-only, and moves the caret otherwise. Without the flag the drag would
/// have to guess, and by then the field has focus -- the very fact it needs.
pub fn long_press_start(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    has_focus: bool,
    read_only: bool,
) -> LongPressStart {
    use crate::editable_text::TargetPlatform;
    if !selection_enabled {
        return LongPressStart {
            selects: LongPressSelects::Nothing,
            haptic: false,
            starts_floating_cursor: false,
            remembers_it_began_unfocused: false,
            shows_magnifier: false,
        };
    }
    let base = LongPressStart {
        selects: LongPressSelects::Word,
        haptic: true,
        starts_floating_cursor: false,
        remembers_it_began_unfocused: false,
        shows_magnifier: true,
    };
    match platform {
        TargetPlatform::IOS | TargetPlatform::MacOS => {
            if !has_focus {
                LongPressStart {
                    haptic: false,
                    remembers_it_began_unfocused: true,
                    ..base
                }
            } else if read_only {
                base
            } else {
                LongPressStart {
                    selects: LongPressSelects::CaretAtTheFinger,
                    haptic: false,
                    starts_floating_cursor: true,
                    ..base
                }
            }
        }
        TargetPlatform::Android
        | TargetPlatform::Fuchsia
        | TargetPlatform::Linux
        | TargetPlatform::Windows => base,
    }
}

/// Everything upstream's `_onSingleLongTapEndOrCancel` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongPressEnd {
    pub hides_magnifier: bool,
    pub clears_the_flag: bool,
    /// `_dragStartViewportOffset = 0.0; _dragStartScrollOffset = 0.0;`
    ///
    /// The two readings [`drag_anchor_correction`] subtracts against. They are
    /// **taken at the press and spent by the drag**, so leaving them behind
    /// would have the next press correct its anchor against a scroll position
    /// belonging to the last one -- a selection that begins somewhere the
    /// reader never pressed, and only in a field that had scrolled since.
    ///
    /// Unconditional, and it has to be: the cancel path is the one where a
    /// press is taken away mid-drag, which is exactly when the anchors are
    /// most likely to be stale.
    pub resets_the_drag_anchors: bool,
    pub ends_floating_cursor: bool,
    /// Only [`LongPressFinish::Ended`] can raise it, and only when the
    /// pointer that pressed had earned one. See [`long_press_finish`].
    pub shows_toolbar: bool,
}

/// Which way a long press stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongPressFinish {
    /// `onSingleLongTapEnd` -- the finger lifted.
    Ended,
    /// `onSingleLongTapCancel` -- the gesture was taken away, by the arena or
    /// by the field going out from under it.
    Cancelled,
}

/// Upstream's `_onSingleLongTapEndOrCancel`, which the end and the cancel
/// share -- a long press that was taken away has to leave the same way one
/// that finished does.
///
/// The first two happen unconditionally. The third is gated three ways:
///
/// ```dart
/// if (_isEditableTextMounted
///     && defaultTargetPlatform == TargetPlatform.iOS
///     && delegate.selectionEnabled
///     && editableText.textEditingValue.selection.isCollapsed) {
/// ```
///
/// **`iOS` alone, not the Apple pair** -- and [`long_press_start`] begins a
/// floating cursor on macOS as well. Ported as it is written rather than as
/// it looks like it should be: this is upstream's asymmetry, and guessing it
/// into symmetry would be inventing behaviour on a platform where a long
/// press with a mouse is unusual enough that nobody has needed to.
///
/// The collapsed check is the other half: a floating cursor is for placing a
/// caret, so a press that ended with a range selected was never doing that.
pub fn long_press_end(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    selection_is_collapsed: bool,
) -> LongPressEnd {
    long_press_finish(
        LongPressFinish::Ended,
        platform,
        selection_enabled,
        selection_is_collapsed,
        true,
        true,
    )
}

/// Upstream's `onSingleLongTapEnd` and `onSingleLongTapCancel`, which are the
/// same function plus one line:
///
/// ```dart
/// void onSingleLongTapEnd(LongPressEndDetails details) {
///   _onSingleLongTapEndOrCancel();
///   if (shouldShowSelectionToolbar) {
///     editableText.showToolbar();
///   }
/// }
///
/// void onSingleLongTapCancel() {
///   _onSingleLongTapEndOrCancel();
/// }
/// ```
///
/// **A cancelled press tidies up exactly as a finished one does**, and the
/// toolbar is the whole difference. That is the reason the tail is factored
/// out at all: a press taken away by the gesture arena, or by the field being
/// disposed under it, must not leave the magnifier on screen or the anchors
/// half-set, and it must not put a menu up for a gesture the reader never
/// completed.
///
/// `editable_text_mounted` is upstream's `_isEditableTextMounted`, and it
/// guards only the floating cursor. A cancel can arrive **after the field is
/// gone** -- that is a normal way for one to arrive -- so the one step that
/// talks to the field is the one that has to ask. Hiding the magnifier and
/// zeroing the anchors are this object's own business and happen either way.
pub fn long_press_finish(
    finish: LongPressFinish,
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    selection_is_collapsed: bool,
    editable_text_mounted: bool,
    should_show_selection_toolbar: bool,
) -> LongPressEnd {
    use crate::editable_text::TargetPlatform;
    LongPressEnd {
        hides_magnifier: true,
        clears_the_flag: true,
        resets_the_drag_anchors: true,
        ends_floating_cursor: editable_text_mounted
            && platform == TargetPlatform::IOS
            && selection_enabled
            && selection_is_collapsed,
        shows_toolbar: finish == LongPressFinish::Ended && should_show_selection_toolbar,
    }
}

/// What one of the heavier gestures does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PressAction {
    /// Whether the selection moves. `false` is upstream returning early
    /// because the field does not select.
    pub selects: bool,
    /// Whether the toolbar goes up.
    pub shows_toolbar: bool,
    /// Whether `_shouldShowSelectionToolbar` is **set to true** by this
    /// gesture, which only [`force_press_start`] does.
    pub raises_the_flag: bool,
}

/// Upstream's `onDoubleTapDown`.
///
/// ```dart
/// if (delegate.selectionEnabled) {
///   renderEditable.selectWord(cause: SelectionChangedCause.doubleTap);
///   if (shouldShowSelectionToolbar) {
///     editableText.showToolbar();
///   }
/// }
/// ```
///
/// Both halves are inside the `selectionEnabled` check, and the toolbar is
/// gated. It does **not** set the flag -- a double tap goes with whatever the
/// tap before it decided, which is how a double tap with a mouse selects the
/// word without raising a menu nobody asked for.
pub fn double_tap_down(
    selection_enabled: bool,
    should_show_selection_toolbar: bool,
) -> PressAction {
    PressAction {
        selects: selection_enabled,
        shows_toolbar: selection_enabled && should_show_selection_toolbar,
        raises_the_flag: false,
    }
}

/// Upstream's `onForcePressStart`.
///
/// ```dart
/// assert(delegate.forcePressEnabled);
/// _shouldShowSelectionToolbar = true;
/// if (!delegate.selectionEnabled) {
///   return;
/// }
/// renderEditable.selectWordsInRange(from: details.globalPosition, ...);
/// editableText.showToolbar();
/// ```
///
/// Two orderings, both deliberate.
///
/// * **The flag is raised before the check**, so a field that does not select
///   still leaves it true. A force press is by definition a considered
///   gesture -- nobody presses hard by accident -- and that is the claim the
///   flag carries forward, whatever this particular field does with it.
/// * **The toolbar is shown unconditionally here**, where every other handler
///   asks the flag first. It has just set the flag itself; asking would be
///   asking its own question back.
pub fn force_press_start(selection_enabled: bool) -> PressAction {
    PressAction {
        selects: selection_enabled,
        shows_toolbar: selection_enabled,
        raises_the_flag: true,
    }
}

/// Upstream's `onForcePressEnd`, which selects again and **asks the flag**.
///
/// The asymmetry with [`force_press_start`] is the whole of this function.
/// Start set the flag true, so ordinarily the gate is open and the end shows
/// the toolbar too. The one thing that closes it in between is a drag: a
/// force press the reader turned into a scroll clears the flag, and then
/// letting go does not pop a toolbar over the text they were scrolling to.
///
/// Note also that this selects **whether or not** the field selects -- there
/// is no `selectionEnabled` check here, only the assert that force press was
/// enabled. Upstream reaches this handler at all only through a recogniser
/// the delegate asked for.
pub fn force_press_end(should_show_selection_toolbar: bool) -> PressAction {
    PressAction {
        selects: true,
        shows_toolbar: should_show_selection_toolbar,
        raises_the_flag: false,
    }
}

/// What a plain tap does when the finger (or the pointer) lifts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapUp {
    /// The field does not select. Upstream still asks for the keyboard --
    /// see [`single_tap_up`] -- and does nothing else.
    SelectionDisabled,
    /// The desktops decided on the way **down**; there is nothing left here.
    Nothing,
    /// Shift was held: [`extend_selection`].
    Extend,
    /// Shift was held: [`expand_selection`] from the current selection.
    Expand,
    /// Shift was held on an unfocused iOS field: expand from a caret at zero.
    ExpandFromTheStart,
    /// Put the caret where the tap was.
    PlaceCaret,
    /// Put the caret there **and** offer spelling suggestions. Android only.
    PlaceCaretAndOfferSpelling,
    /// Put the caret there and take the toolbar down. iOS with a precise
    /// device.
    PlaceCaretAndHideToolbar,
    /// iOS under a finger: [`tap_outcome`] is the rule, and it is long enough
    /// to live on its own.
    AskTheTouchRule,
}

/// Upstream's `onSingleTapUp`, which is where the mobile platforms decide.
///
/// The mirror of [`shift_tap_down`]: everything the desktops settle on the way
/// down, the phones settle on the way up, and each list is the other's
/// complement.
///
/// # Three things worth having read the code for
///
/// * **iOS has macOS's shift-from-zero rule too**, with its own copy of the
///   comment -- but on tap *up*, because that is where iOS decides. The two
///   Apple platforms agree about the behaviour and disagree about when.
/// * **Android offers spelling suggestions after a plain tap and Fuchsia does
///   not.** The two branches are otherwise the same five lines, and this is
///   the only difference between them: a port that folded them together would
///   lose the spell-check on Android or invent it on Fuchsia.
/// * **A precise device on iOS hides the toolbar; a finger toggles it.** The
///   long touch rule at [`tap_outcome`] is only reached under a finger. A
///   mouse on an iPad places the caret and takes the menu down, because a
///   mouse can aim and does not need a second tap to say where it meant.
///
/// And the keyboard is asked for on **every** path, including the one where
/// the field does not select at all -- upstream's `requestKeyboard()` is
/// after the switch, and the disabled branch returns through it. A read-only
/// or unselectable field still takes the keyboard when tapped.
pub fn single_tap_up(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    shift_pressed: bool,
    has_selection: bool,
    has_focus: bool,
    kind: PointerKind,
) -> TapUp {
    use crate::editable_text::TargetPlatform;
    if !selection_enabled {
        return TapUp::SelectionDisabled;
    }
    let shift = shift_pressed && has_selection;
    match platform {
        TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Windows => TapUp::Nothing,
        TargetPlatform::Android => {
            if shift {
                TapUp::Extend
            } else {
                TapUp::PlaceCaretAndOfferSpelling
            }
        }
        TargetPlatform::Fuchsia => {
            if shift {
                TapUp::Extend
            } else {
                TapUp::PlaceCaret
            }
        }
        TargetPlatform::IOS => {
            if shift {
                return if has_focus {
                    TapUp::Expand
                } else {
                    TapUp::ExpandFromTheStart
                };
            }
            match kind {
                PointerKind::Touch | PointerKind::Unknown => TapUp::AskTheTouchRule,
                _ => TapUp::PlaceCaretAndHideToolbar,
            }
        }
    }
}

/// Whether a tap asks for the keyboard, which upstream answers with a single
/// unconditional call after the switch.
///
/// Its own function because the alternative is a field on [`TapUp`] that is
/// true in every variant, and a constant is not an answer worth carrying
/// around. A tap on a field that cannot be selected in still opens the
/// keyboard: the reader tapped a text field, and typing is the other thing
/// they might have meant.
pub fn tap_up_requests_keyboard() -> bool {
    true
}

/// What a right-click moves the selection to, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecondarySelects {
    /// The word under the pointer. What the Apple platforms do, so that the
    /// menu's `Copy` and `Look Up` have something to act on.
    Word,
    /// Just the caret. What the others do: a right-click there is about
    /// opening the menu, not about choosing text.
    Position,
    /// Nothing -- whatever was selected stays selected.
    Nothing,
}

/// What a right-click does to the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecondaryToolbar {
    /// Hide it and show it again. **Not** a toggle: a second right-click
    /// somewhere else moves the menu there rather than dismissing it.
    Reshow,
    /// Up if it was down, down if it was up. A second right-click dismisses.
    Toggle,
    /// Leave it alone.
    Nothing,
}

/// Both halves of upstream's `onSecondaryTap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondaryTap {
    pub selects: SecondarySelects,
    pub toolbar: SecondaryToolbar,
}

/// Upstream's `_lastSecondaryTapWasOnSelection`, which is
/// [`position_was_on_selection_inclusive`] under another name -- the same
/// `start <= offset && end >= offset`.
///
/// Worth saying rather than leaving to be noticed: a right-click **on the
/// edge** of a highlighted run counts as on it, where a left-click at
/// [`tap_outcome`] does not. The two are asking different questions. A
/// left-click on the edge is aimed at the handle; there are no handles under a
/// right-click, and what it is aimed at is the menu.
///
/// `None` is upstream's null selection, which answers false: there is nothing
/// to have clicked inside of.
pub fn last_secondary_tap_was_on_selection(selection: Option<(i32, i32)>, offset: i32) -> bool {
    match selection {
        Some(selection) => position_was_on_selection_inclusive(selection, offset),
        None => false,
    }
}

/// Upstream's `TextSelectionGestureDetectorBuilder.onSecondaryTap`.
///
/// ```dart
/// case iOS || macOS:
///   if (!_lastSecondaryTapWasOnSelection || !renderEditable.hasFocus) {
///     renderEditable.selectWord(cause: SelectionChangedCause.tap);
///   }
///   if (shouldShowSelectionToolbar) {
///     editableText.hideToolbar();
///     editableText.showToolbar();
///   }
/// case android || fuchsia || linux || windows:
///   if (!renderEditable.hasFocus) {
///     renderEditable.selectPosition(cause: SelectionChangedCause.tap);
///   }
///   editableText.toggleToolbar();
/// ```
///
/// Four differences, and none of them is decoration.
///
/// * **Apple selects a word; the rest place a caret.** Right-clicking a word
///   on macOS selects it, so the menu's `Copy` and `Look Up` have something to
///   act on. On Windows and Linux a right-click is about opening the menu, and
///   moving the caret is as much as it does.
/// * **Apple keeps a selection the click landed inside.** That is what
///   `_lastSecondaryTapWasOnSelection` is for: right-clicking within a
///   highlighted run leaves it alone, so `Copy` copies what the reader
///   highlighted rather than the one word under the pointer.
/// * **The rest only touch the selection when the field is unfocused.** A
///   focused field keeps whatever it had, wherever the click landed.
/// * **Apple re-shows the toolbar; the rest toggle it.** Hide-then-show means
///   a second right-click somewhere else *moves* the menu there. Toggling
///   means a second right-click dismisses it. Both are deliberate, and a port
///   that used one everywhere would be wrong on four platforms or on two.
///
/// `shouldShowSelectionToolbar` gates only the Apple branch. The others
/// toggle regardless, which is upstream's shape and not an oversight: a
/// toggle that the flag suppressed would leave a menu up with no way to
/// dismiss it.
pub fn secondary_tap(
    platform: crate::editable_text::TargetPlatform,
    selection_enabled: bool,
    tap_was_on_selection: bool,
    has_focus: bool,
    should_show_selection_toolbar: bool,
) -> SecondaryTap {
    use crate::editable_text::TargetPlatform;
    if !selection_enabled {
        return SecondaryTap {
            selects: SecondarySelects::Nothing,
            toolbar: SecondaryToolbar::Nothing,
        };
    }
    match platform {
        TargetPlatform::IOS | TargetPlatform::MacOS => SecondaryTap {
            selects: if !tap_was_on_selection || !has_focus {
                SecondarySelects::Word
            } else {
                SecondarySelects::Nothing
            },
            toolbar: if should_show_selection_toolbar {
                SecondaryToolbar::Reshow
            } else {
                SecondaryToolbar::Nothing
            },
        },
        TargetPlatform::Android
        | TargetPlatform::Fuchsia
        | TargetPlatform::Linux
        | TargetPlatform::Windows => SecondaryTap {
            selects: if has_focus {
                SecondarySelects::Nothing
            } else {
                SecondarySelects::Position
            },
            toolbar: SecondaryToolbar::Toggle,
        },
    }
}

/// What a single tap on a text field does first -- upstream
/// `TextSelectionGestureDetectorBuilder.onSingleTapUp`, as the decision it
/// makes.
///
/// The long iOS-touch tail of that method is **not** here: it was already
/// ported as [`tap_outcome`] and [`after_selecting_the_word_edge`], and this
/// hands off to them rather than saying it a second time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleTapAction {
    /// The desktop platforms, whose selection was already set on tap *down*.
    Nothing,
    /// Upstream's `_extendSelection`, on a valid shift-tap.
    ExtendSelection,
    /// Upstream's `_expandSelection`, iOS's shift-tap.
    ///
    /// `from_zero` is upstream's `fromSelection`: a shift-tapped **unfocused**
    /// iOS field expands from offset 0 rather than from what was selected
    /// before.
    ExpandSelection { from_zero: bool },
    /// `selectPosition`, and then the spell check toolbar on Android only --
    /// the single line by which the Android and Fuchsia arms differ.
    SelectPosition { spell_check_toolbar: bool },
    /// iOS with a precise pointer: place the cursor and hide the toolbar.
    SelectPositionAndHideToolbar,
    /// iOS with a touch, answered by [`tap_outcome`].
    Touch(TapOutcome),
}

/// One tap's whole outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleTapOutcome {
    pub action: SingleTapAction,
    /// Upstream's trailing `editableText.requestKeyboard()`.
    ///
    /// It is the method's **last line**, so it runs for every platform --
    /// including the desktop arm that does nothing else -- and it runs on the
    /// selection-disabled path too, which asks for it and then returns. The
    /// only paths that skip it are the two shift-taps, which `return` from
    /// inside the switch: **a shift-tap does not focus the field.**
    pub requests_keyboard: bool,
    /// Upstream's `hideToolbar(false)` at the top of the Android and Fuchsia
    /// arms, and nowhere else.
    pub hides_toolbar_first: bool,
}

/// What the toolbar does after a tap chose
/// [`TapOutcome::SelectWordAndOfferSpelling`].
///
/// The sibling for [`TapOutcome::SelectWordEdge`] is [`AfterWordEdge`], and
/// the two read a changed selection **opposite ways**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterMisspelledWord {
    ShowSpellCheckToolbar,
    ToggleToolbar,
}

/// Everything the platform-level decision reads.
#[derive(Clone, Copy, Debug)]
pub struct SingleTapContext {
    pub selection_enabled: bool,
    pub platform: crate::editable_text::TargetPlatform,
    pub shift_pressed: bool,
    /// Upstream's `renderEditable.selection?.baseOffset != null`.
    pub selection_base_valid: bool,
    pub has_focus: bool,
    pub pointer: crate::gestures::PointerKind,
    pub read_only: bool,
    /// Upstream's `findSuggestionSpanAtCursorIndex(...) != null`.
    pub misspelled: bool,
    /// The selection the field already had, and where the tap landed -- what
    /// [`tap_outcome`] needs for the iOS touch branch.
    pub previous_selection: (i32, i32),
    pub tapped_offset: i32,
    /// Upstream's `textPosition.affinity == previousSelection.affinity`.
    pub affinity_same: bool,
}

/// Upstream `TextSelectionGestureDetectorBuilder.onSingleTapUp`.
pub struct SingleTapUp;

impl SingleTapUp {
    /// Upstream's `isShiftPressedValid`, and **both halves matter**: "It is
    /// impossible to extend the selection when the shift key is pressed, if
    /// the renderEditable.selection is invalid." Shift held over a field with
    /// no selection behaves as though shift were not held.
    pub fn shift_is_usable(context: &SingleTapContext) -> bool {
        context.shift_pressed && context.selection_base_valid
    }

    /// Upstream's `details.kind` switch inside the iOS arm. Mouse, trackpad
    /// and both stylus kinds are the *precise* devices; touch and unknown are
    /// not.
    pub fn is_precise(pointer: crate::gestures::PointerKind) -> bool {
        use crate::gestures::PointerKind;
        matches!(
            pointer,
            PointerKind::Mouse
                | PointerKind::Trackpad
                | PointerKind::Stylus
                | PointerKind::InvertedStylus
        )
    }

    pub fn decide(context: &SingleTapContext) -> SingleTapOutcome {
        use crate::editable_text::TargetPlatform;

        // Asked for and then returned from -- a field that cannot be selected
        // in still takes focus when it is tapped.
        if !context.selection_enabled {
            return SingleTapOutcome {
                action: SingleTapAction::Nothing,
                requests_keyboard: true,
                hides_toolbar_first: false,
            };
        }

        let shift = SingleTapUp::shift_is_usable(context);
        let keyboard = |action| SingleTapOutcome {
            action,
            requests_keyboard: true,
            hides_toolbar_first: false,
        };

        match context.platform {
            // "On desktop platforms the selection is set on tap down." The
            // arm does nothing -- but the method's last line still runs.
            TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Windows => {
                keyboard(SingleTapAction::Nothing)
            }
            TargetPlatform::Android | TargetPlatform::Fuchsia => {
                if shift {
                    // Returns from inside the switch, so no keyboard.
                    return SingleTapOutcome {
                        action: SingleTapAction::ExtendSelection,
                        requests_keyboard: false,
                        hides_toolbar_first: true,
                    };
                }
                SingleTapOutcome {
                    // The one line by which the two arms differ.
                    action: SingleTapAction::SelectPosition {
                        spell_check_toolbar: context.platform == TargetPlatform::Android,
                    },
                    requests_keyboard: true,
                    hides_toolbar_first: true,
                }
            }
            TargetPlatform::IOS => {
                if shift {
                    return SingleTapOutcome {
                        action: SingleTapAction::ExpandSelection {
                            // "On iOS, a shift-tapped unfocused field expands
                            // from 0, not from the previous selection."
                            from_zero: !context.has_focus,
                        },
                        requests_keyboard: false,
                        hides_toolbar_first: false,
                    };
                }
                if SingleTapUp::is_precise(context.pointer) {
                    return keyboard(SingleTapAction::SelectPositionAndHideToolbar);
                }
                keyboard(SingleTapAction::Touch(tap_outcome(
                    context.misspelled,
                    context.previous_selection,
                    context.tapped_offset,
                    context.affinity_same,
                    context.read_only,
                    context.has_focus,
                )))
            }
        }
    }

    /// After [`TapOutcome::SelectWordAndOfferSpelling`]: **show** the spell
    /// check toolbar if selecting the word moved the selection, and **toggle**
    /// it if the word was already selected -- a second tap on a misspelled
    /// word that is already selected puts the toolbar away.
    ///
    /// The sense is the opposite of [`after_selecting_the_word_edge`], where a
    /// change means *hide*.
    pub fn after_misspelled_word(selection_changed: bool) -> AfterMisspelledWord {
        if selection_changed {
            AfterMisspelledWord::ShowSpellCheckToolbar
        } else {
            AfterMisspelledWord::ToggleToolbar
        }
    }
}

/// What a triple tap selects -- upstream
/// `TextSelectionGestureDetectorBuilder.onTripleTapDown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TripleTapSelects {
    /// A field that cannot wrap: the paragraph, the line and the whole text
    /// are the same thing, and upstream calls `selectAll` outright.
    Everything,
    /// Every platform but one.
    Paragraph,
    /// **Linux alone.**
    Line,
}

/// The boundary selection a triple tap or a boundary drag makes -- upstream
/// `TextSelectionGestureDetectorBuilder._selectTextBoundariesInRange` and
/// `_moveToTextBoundary`.
pub struct BoundarySelection;

impl BoundarySelection {
    /// Upstream's `onTripleTapDown`, as the decision it makes.
    ///
    /// **The one-line case is answered before the platform is asked.** After
    /// that it is one row against five: Linux takes a line and everybody else
    /// takes a paragraph.
    pub fn triple_tap(
        max_lines: Option<usize>,
        platform: crate::editable_text::TargetPlatform,
    ) -> TripleTapSelects {
        use crate::editable_text::TargetPlatform;
        if max_lines == Some(1) {
            return TripleTapSelects::Everything;
        }
        match platform {
            TargetPlatform::Linux => TripleTapSelects::Line,
            _ => TripleTapSelects::Paragraph,
        }
    }

    /// Upstream's `_moveToTextBoundary`.
    ///
    /// **The minus one is at the end of the text and only on the leading
    /// side.** Upstream's comment: "Use extent.offset - 1 when `extent` is at
    /// the end of the text to retrieve the previous text boundary's location."
    /// A caret at the very end is past the last boundary, so asking there for
    /// the leading boundary answers the end itself and the range comes back
    /// empty; stepping back one character asks about the last paragraph
    /// instead.
    ///
    /// The trailing lookup gets no adjustment -- it walks forwards, and
    /// forwards from the end is the end. And the two fallbacks go to opposite
    /// ends of the text: **0** for the leading one and the **length** for the
    /// trailing one.
    pub fn move_to_boundary(
        extent: isize,
        text_length: isize,
        boundary: &dyn crate::services::text_boundary::TextBoundary,
    ) -> crate::services::text_boundary::TextRange {
        let leading_from = if extent == text_length {
            extent - 1
        } else {
            extent
        };
        crate::services::text_boundary::TextRange {
            start: boundary.leading_boundary_at(leading_from).unwrap_or(0),
            end: boundary.trailing_boundary_at(extent).unwrap_or(text_length),
        }
    }

    /// Upstream's `_selectTextBoundariesInRange`, minus the hit testing.
    ///
    /// `to` is `None` for a tap that never became a drag, and then the far end
    /// *is* the near end -- one boundary, selected from its own start to its
    /// own end.
    ///
    /// Returns `(base, extent)`. The swap test is
    /// `fromRange.start < toRange.end`, the same one
    /// `WordSelection::words_in_range` uses one
    /// boundary kind down: a backwards drag selects the same span with the
    /// ends the other way about, so the handles stay where the finger put
    /// them.
    pub fn in_range(
        from_extent: isize,
        to_extent: Option<isize>,
        text_length: isize,
        boundary: &dyn crate::services::text_boundary::TextBoundary,
    ) -> (isize, isize) {
        let from_range = BoundarySelection::move_to_boundary(from_extent, text_length, boundary);
        let to_extent = to_extent.unwrap_or(from_extent);
        let to_range = if to_extent == from_extent {
            from_range
        } else {
            BoundarySelection::move_to_boundary(to_extent, text_length, boundary)
        };
        if from_range.start < to_range.end {
            (from_range.start, to_range.end)
        } else {
            (from_range.end, to_range.start)
        }
    }
}

/// Which end of the selection a handle drag is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionHandleEnd {
    Start,
    End,
}

/// What a handle drag reports back -- upstream's `onStartHandleDragStart`,
/// `onStartHandleDragUpdate` and `onStartHandleDragEnd` and their end-handle
/// twins, as the events they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleDragCallback {
    /// Upstream's `onStartHandleDragStart` / `onEndHandleDragStart`. Also
    /// raised by an *update* that is picking up a drag it never got the start
    /// of.
    Start(SelectionHandleEnd),
    Update(SelectionHandleEnd),
    End(SelectionHandleEnd),
}

/// The drag lifecycle of the two selection handles -- upstream
/// `SelectionOverlay`'s `_handle*HandleDrag*` methods and the two booleans
/// behind each handle.
///
/// # Two flags per handle, and they mean different things
///
/// `in_progress` is set the moment a drag begins, **before** the can-drag
/// guard, and says a gesture is happening at all. `dragging` is set only past
/// the guard and only for a **touch** -- it says this is being *treated* as a
/// handle drag. A mouse drag on a handle sets the first and not the second.
///
/// Upstream's public getter is the *or* of the two, so from the outside a
/// handle is being dragged in either case; only in here are they apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HandleDragState {
    /// Upstream's `_isDraggingStartHandle` / `_isDraggingEndHandle`.
    dragging_start: bool,
    dragging_end: bool,
    /// Upstream's `_startHandleDragInProgress` / `_endHandleDragInProgress`.
    in_progress_start: bool,
    in_progress_end: bool,
}

impl HandleDragState {
    pub fn new() -> HandleDragState {
        HandleDragState::default()
    }

    /// Upstream's `isDraggingStartHandle` / `isDraggingEndHandle`: the **or**
    /// of the two flags, so a mouse drag still reads as a drag from outside.
    pub fn is_dragging(&self, end: SelectionHandleEnd) -> bool {
        match end {
            SelectionHandleEnd::Start => self.dragging_start || self.in_progress_start,
            SelectionHandleEnd::End => self.dragging_end || self.in_progress_end,
        }
    }

    fn dragging(&self, end: SelectionHandleEnd) -> bool {
        match end {
            SelectionHandleEnd::Start => self.dragging_start,
            SelectionHandleEnd::End => self.dragging_end,
        }
    }

    fn set_dragging(&mut self, end: SelectionHandleEnd, value: bool) {
        match end {
            SelectionHandleEnd::Start => self.dragging_start = value,
            SelectionHandleEnd::End => self.dragging_end = value,
        }
    }

    fn set_in_progress(&mut self, end: SelectionHandleEnd, value: bool) {
        match end {
            SelectionHandleEnd::Start => self.in_progress_start = value,
            SelectionHandleEnd::End => self.in_progress_end = value,
        }
    }

    /// Upstream's `_canDragStartHandle` / `_canDragEndHandle`.
    ///
    /// **On Apple and on the web only one handle moves at a time**, so a drag
    /// on one blocks the other. Everywhere else both can move at once. Note
    /// the shape: the guard is open whenever the *opposite* handle is idle, on
    /// every platform.
    pub fn can_drag(
        &self,
        end: SelectionHandleEnd,
        platform: crate::editable_text::TargetPlatform,
        is_web: bool,
    ) -> bool {
        use crate::editable_text::TargetPlatform;
        let opposite = match end {
            SelectionHandleEnd::Start => self.dragging_end,
            SelectionHandleEnd::End => self.dragging_start,
        };
        !opposite
            || (platform != TargetPlatform::IOS && platform != TargetPlatform::MacOS && !is_web)
    }

    /// Upstream's `_handleStartHandleDragStart` / `_handleEndHandleDragStart`.
    ///
    /// `handles_present` is upstream's `_handles != null`, and it is checked
    /// first for the reason upstream records: "Calling OverlayEntry.remove may
    /// not happen until the following frame, so it's possible for the handles
    /// to receive a gesture after calling remove."
    ///
    /// `is_touch` is `details.kind == PointerDeviceKind.touch`.
    pub fn drag_start(
        &mut self,
        end: SelectionHandleEnd,
        handles_present: bool,
        is_touch: bool,
        platform: crate::editable_text::TargetPlatform,
        is_web: bool,
    ) -> Option<HandleDragCallback> {
        if !handles_present {
            self.set_dragging(end, false);
            return None;
        }
        // Set **before** the guard: the gesture is happening whether or not
        // this handle is allowed to answer it.
        self.set_in_progress(end, true);
        if !self.can_drag(end, platform, is_web) {
            return None;
        }
        // And only a touch counts as dragging the handle.
        self.set_dragging(end, is_touch);
        Some(HandleDragCallback::Start(end))
    }

    /// Upstream's `_handleStartHandleDragUpdate` / `..EndHandleDragUpdate`.
    ///
    /// Returns the callbacks in the order upstream raises them. **An update
    /// can raise a `Start`**: if the drag was blocked when it began -- the
    /// opposite handle was down on an Apple platform, and has since been let
    /// go -- this synthesises the start it never got, so that everything meant
    /// to run on start still runs. Without it a drag would move without ever
    /// having begun.
    pub fn drag_update(
        &mut self,
        end: SelectionHandleEnd,
        handles_present: bool,
        is_touch: bool,
        platform: crate::editable_text::TargetPlatform,
        is_web: bool,
    ) -> Vec<HandleDragCallback> {
        if !handles_present {
            self.set_dragging(end, false);
            return Vec::new();
        }
        if !self.can_drag(end, platform, is_web) {
            return Vec::new();
        }
        let mut raised = Vec::new();
        if !self.dragging(end) {
            self.set_dragging(end, is_touch);
            raised.push(HandleDragCallback::Start(end));
        }
        raised.push(HandleDragCallback::Update(end));
        raised
    }

    /// Upstream's `_handleStartHandleDragEnd` / `..EndHandleDragEnd`.
    ///
    /// The two flags are cleared on opposite sides of the null check, and that
    /// asymmetry is upstream's: `dragging` is cleared **before** anything
    /// else, so it is always cleared; `in_progress` is cleared only when the
    /// handles are still there.
    pub fn drag_end(
        &mut self,
        end: SelectionHandleEnd,
        handles_present: bool,
        platform: crate::editable_text::TargetPlatform,
        is_web: bool,
    ) -> Option<HandleDragCallback> {
        self.set_dragging(end, false);
        if !handles_present {
            return None;
        }
        self.set_in_progress(end, false);
        if !self.can_drag(end, platform, is_web) {
            return None;
        }
        Some(HandleDragCallback::End(end))
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
    /// The two questions upstream asks about the magnifier, which is one
    /// question more than the handles and the toolbar get here.
    pub magnifier: OverlayMagnifier,
}

impl SelectionOverlay {
    pub fn new() -> SelectionOverlay {
        SelectionOverlay {
            handles_visible: false,
            toolbar_visible: false,
            line_height_at_start: 0.0,
            line_height_at_end: 0.0,
            magnifier: OverlayMagnifier::default(),
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

    /// Upstream's `_updateTextSelectionOverlayVisibilities`.
    ///
    /// `handles_built` is `_handles != null` -- whether `showHandles` has put
    /// anything in the overlay. It is a separate axis from
    /// [`Self::handles_visible`], and upstream keeps them apart on purpose:
    /// `showHandles`/`hideHandles` **build and destroy** ("Builds the handles
    /// by inserting them into the overlay", "Destroys the handles by removing
    /// them"), while `handlesVisible` shows and hides what is already built.
    /// `handlesAreVisible` is the conjunction, `_handles != null &&
    /// handlesVisible`.
    ///
    /// The toolbar has no such second axis here: it is built when shown, so
    /// this crate's `toolbar_visible` stands in for both. Upstream's line is
    /// the two viewport readings **alone** --
    /// `_effectiveToolbarVisibility.value = startInViewport || endInViewport`
    /// -- because that is a *visibility signal* handed to a toolbar whose
    /// existence is tracked elsewhere (`_toolbar != null`, or the context menu
    /// controller). What reaches the screen is that signal and that existence
    /// together, which is what the conjunct here spells out. The mutation
    /// sweep found it unasserted; it is the same shape as `handles_built &&
    /// handles_visible` one line above.
    pub fn visibilities(
        &self,
        handles_built: bool,
        in_viewport: (bool, bool),
    ) -> OverlayVisibilities {
        let (start_in_viewport, end_in_viewport) = in_viewport;
        let wanted = handles_built && self.handles_visible;
        OverlayVisibilities {
            start_handle: wanted && start_in_viewport,
            end_handle: wanted && end_in_viewport,
            // Neither `handlesVisible` nor whether the handles were built.
            toolbar: self.toolbar_visible && (start_in_viewport || end_in_viewport),
        }
    }

    /// Upstream's `showHandles`, which **builds** rather than reveals:
    ///
    /// ```dart
    /// void showHandles() {
    ///   if (_handles != null) {
    ///     return;
    ///   }
    ///   ...
    /// ```
    ///
    /// The same guard as `showMagnifier` and for the same reason -- a second
    /// call would insert a second pair over the first. Returns whether
    /// anything was built.
    pub fn show_handles(handles_built: bool) -> bool {
        !handles_built
    }

    /// Upstream's `hideHandles`, which **destroys**: it removes both entries,
    /// disposes them and drops the pair. Returns whether there was anything to
    /// take down.
    ///
    /// Note what it does *not* do: it leaves `handlesVisible` alone. Hiding
    /// and destroying are different verbs on different axes, and a port that
    /// made `hideHandles` clear the flag would leave a field that could never
    /// show its handles again without somebody setting it back.
    pub fn hide_handles(handles_built: bool) -> bool {
        handles_built
    }

    /// What one of the two update paths did.
    ///
    /// `rebuilt` is upstream's `markNeedsBuild()`, and it is the interesting
    /// half: `_updateSelectionOverlay` writes properties, and writing a
    /// property that already holds that value rebuilds nothing. Both callers
    /// therefore ask for a build outright, and **each has a different case in
    /// mind**.
    pub fn update_outcome(refreshed: bool, rebuilt: bool) -> OverlayUpdate {
        OverlayUpdate { refreshed, rebuilt }
    }

    /// Upstream's `update(TextEditingValue newValue)`.
    ///
    /// ```dart
    /// void update(TextEditingValue newValue) {
    ///   if (_value == newValue) {
    ///     return;
    ///   }
    ///   _value = newValue;
    ///   _updateSelectionOverlay();
    ///   // _updateSelectionOverlay may not rebuild the selection overlay if the
    ///   // text metrics and selection doesn't change even if the text has changed.
    ///   // This rebuild is needed for the toolbar to update based on the latest text
    ///   // value.
    ///   _selectionOverlay.markNeedsBuild();
    /// }
    /// ```
    ///
    /// **The equality guard is what lets this be called freely.** Every edit
    /// funnels through here, and a value that has not moved does no work.
    ///
    /// **The explicit rebuild is for the toolbar.** Text can change without
    /// the *metrics or the selection* changing -- replace a word with another
    /// of the same width and the endpoints, the line heights and the selection
    /// are all where they were -- so nothing `_updateSelectionOverlay` writes
    /// is different, and a menu offering "Look Up" on the old word would stay
    /// as it was. The comment upstream leaves is exactly that.
    pub fn update(&mut self, value_changed: bool) -> OverlayUpdate {
        if !value_changed {
            return OverlayUpdate {
                refreshed: false,
                rebuilt: false,
            };
        }
        OverlayUpdate {
            refreshed: true,
            rebuilt: true,
        }
    }

    /// Upstream's `updateForScroll`.
    ///
    /// ```dart
    /// void updateForScroll() {
    ///   _updateSelectionOverlay();
    ///   // This method may be called due to windows metrics changes. In that case,
    ///   // non of the properties in _selectionOverlay will change, but a rebuild is
    ///   // still needed.
    ///   _selectionOverlay.markNeedsBuild();
    /// }
    /// ```
    ///
    /// The same two lines as [`Self::update`] and **no guard at all**, which
    /// is the whole difference. There is no value to compare: what moved is
    /// the render object's text metrics, and this method is told only that
    /// something did.
    ///
    /// Its rebuild has a different reason from the other one. Upstream names
    /// window-metrics changes, where **not one** property of the overlay comes
    /// out different -- the selection is the same, the endpoints are the same
    /// in the field's own coordinates -- and a build is still needed because
    /// what the overlay draws depends on things outside those properties.
    ///
    /// So the pair is: one guarded on the value and rebuilding for the
    /// toolbar, one unguarded and rebuilding for the window. Folding them into
    /// a single `update(Option<value>)` would have to drop one of the two
    /// reasons, and the reasons are what they are for.
    pub fn update_for_scroll(&mut self) -> OverlayUpdate {
        OverlayUpdate {
            refreshed: true,
            rebuilt: true,
        }
    }

    /// Upstream's `hide`, which takes **both** away.
    ///
    /// The pair is hidden together because a toolbar without handles is a
    /// toolbar acting on a selection the reader can no longer see the edges
    /// of.
    pub fn hide(&mut self) {
        self.handles_visible = false;
        self.toolbar_visible = false;
        self.magnifier.hide();
    }
}

/// Which handle shape each end of a selection gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleTypes {
    pub start: TextSelectionHandleType,
    pub end: TextSelectionHandleType,
}

/// Upstream's handle-type choice inside `_updateSelectionOverlay`.
///
/// # "start" and "end" are about the text; "left" and "right" are about the screen
///
/// ```dart
/// startHandleType = switch (startHandleDirection) {
///   TextDirection.ltr => TextSelectionHandleType.left,
///   TextDirection.rtl => TextSelectionHandleType.right,
/// };
/// endHandleType = switch (endHandleDirection) {
///   TextDirection.ltr => TextSelectionHandleType.right,
///   TextDirection.rtl => TextSelectionHandleType.left,
/// };
/// ```
///
/// **The two maps are opposites, and neither is the identity.** In
/// right-to-left text the selection's start is the handle on the *right*.
/// Writing `start => left` because the names line up would put both handles on
/// the wrong sides of every Arabic or Hebrew selection, and the onion each one
/// draws would point away from the text it holds -- see
/// [`crate::selection_host`], which rotates the shape by handle type.
///
/// # A collapsed selection gets a third shape, not one of the two
///
/// Both ends answer `collapsed`. An insertion point has no left and no right
/// to be on, and upstream keeps a separate value rather than picking one
/// arbitrarily.
///
/// # iOS orients both handles by the field
///
/// > UIKit keeps selection handles aligned with the field direction.
///
/// So a selection running through mixed-direction text gets handles from
/// `renderObject.textDirection` on iOS and from **each endpoint's own**
/// direction everywhere else. Same selection, same text, two different pairs
/// of shapes.
///
/// # And fewer than two endpoints falls back the same way
///
/// Upstream names four causes, and they are worth keeping because each is a
/// real moment rather than a defensive shrug: the overlay updated before the
/// render object laid the new text out; a selection boundary fell inside a
/// multi-code-unit cluster such as an emoji; the layout was momentarily
/// squashed, with `preferredLineHeight` at zero during a fold transition. In
/// all of them the endpoint directions are not available, so the field's
/// direction stands in.
///
/// `endpoint_directions` is `None` for that case, and `Some((start, end))`
/// otherwise, where each may itself be `None` -- upstream's
/// `endpoints.first.direction ?? textDirection`.
pub fn handle_types(
    collapsed: bool,
    platform: crate::editable_text::TargetPlatform,
    field_direction: TextDirection,
    endpoint_directions: Option<(Option<TextDirection>, Option<TextDirection>)>,
) -> HandleTypes {
    use crate::editable_text::TargetPlatform;
    if collapsed {
        return HandleTypes {
            start: TextSelectionHandleType::Collapsed,
            end: TextSelectionHandleType::Collapsed,
        };
    }
    let prefer_field = platform == TargetPlatform::IOS;
    let (start_direction, end_direction) = match endpoint_directions {
        Some((start, end)) if !prefer_field => (
            start.unwrap_or(field_direction),
            end.unwrap_or(field_direction),
        ),
        _ => (field_direction, field_direction),
    };
    HandleTypes {
        start: match start_direction {
            TextDirection::Ltr => TextSelectionHandleType::Left,
            TextDirection::Rtl => TextSelectionHandleType::Right,
        },
        end: match end_direction {
            TextDirection::Ltr => TextSelectionHandleType::Right,
            TextDirection::Rtl => TextSelectionHandleType::Left,
        },
    }
}

/// What an update did: whether the overlay's properties were refreshed from
/// the render object, and whether a build was asked for on top of that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayUpdate {
    /// `_updateSelectionOverlay()` ran.
    pub refreshed: bool,
    /// `markNeedsBuild()` was called, which upstream does **in addition** at
    /// both call sites and never leaves to the property writes.
    pub rebuilt: bool,
}

/// What the overlay actually puts on screen, from the two viewport readings
/// and the one wish.
///
/// Upstream computes all three in `_updateTextSelectionOverlayVisibilities`,
/// and **no two of them combine the inputs the same way**:
///
/// ```dart
/// _effectiveStartHandleVisibility.value =
///     _handlesVisible && renderObject.selectionStartInViewport.value;
/// _effectiveEndHandleVisibility.value =
///     _handlesVisible && renderObject.selectionEndInViewport.value;
/// _effectiveToolbarVisibility.value =
///     renderObject.selectionStartInViewport.value || renderObject.selectionEndInViewport.value;
/// ```
///
/// **Each handle is gated by its own end.** Scroll a selection until only its
/// beginning is in the field and the start handle stays while the end handle
/// goes -- they are not one control that appears and disappears together, and
/// they must not be, because the one still on screen is still draggable.
///
/// **The toolbar takes `||` where the handles take `&&`.** It stays up while
/// *either* end is in view, because it acts on the selection as a whole and a
/// selection with one end scrolled away is still a selection worth copying.
///
/// **And the toolbar does not consult `handlesVisible` at all.** A caller that
/// turns the handles off keeps the menu. That is what the property is for:
/// upstream's doc says "use this property to show or hide the handle without
/// rebuilding them", and it is about the handles alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayVisibilities {
    pub start_handle: bool,
    pub end_handle: bool,
    pub toolbar: bool,
}

/// Upstream keeps **two** questions about the magnifier and answers them from
/// two different places:
///
/// ```dart
/// bool get magnifierIsVisible => _magnifierController.shown;
///
/// /// This differs from [magnifierIsVisible] in that the magnifier may exist
/// /// in the overlay, but not be shown.
/// bool get magnifierExists => _magnifierController.overlayEntry != null;
/// ```
///
/// The same split is in `handlesAreVisible`, which is
/// `_handles != null && handlesVisible` -- whether they were built, *and*
/// whether they are wanted. One boolean cannot say both, and which one a rule
/// reads turns out to matter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayMagnifier {
    /// `_magnifierController.overlayEntry != null`.
    pub exists: bool,
    /// `_magnifierController.shown`, which upstream's own doc calls out as
    /// **not** the source of truth for whether a magnifier is up: "magnifiers
    /// may hide themselves".
    pub shown: bool,
}

/// What upstream's `showMagnifier` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowMagnifier {
    /// It returned at the first line because one was already there.
    pub already_there: bool,
    /// `if (toolbarIsVisible) { hideToolbar(); }`
    pub hides_toolbar: bool,
    /// Whether anything was put in the overlay. `false` when the builder
    /// returned null, which is how a platform without a magnifier opts out.
    pub inserts: bool,
}

impl OverlayMagnifier {
    /// Upstream's `showMagnifier`, in the order it does things:
    ///
    /// ```dart
    /// // Do not show the magnifier if one already exists.
    /// if (_magnifierController.overlayEntry != null) { return; }
    /// if (toolbarIsVisible) { hideToolbar(); }
    /// ...
    /// final Widget? builtMagnifier = magnifierConfiguration.magnifierBuilder(...);
    /// if (builtMagnifier == null) { return; }
    /// _magnifierController.show(...);
    /// ```
    ///
    /// **The guard is on `overlayEntry`, not on `shown`** -- so a magnifier
    /// that exists and has hidden itself is not re-shown. Keying off `shown`
    /// would try to insert a second one on top of the first.
    ///
    /// **A magnifier and a toolbar are never up together.** Showing one takes
    /// the other down, and it does not come back: nothing here remembers that
    /// there was a toolbar.
    ///
    /// **And the toolbar goes before it is known whether a magnifier will be
    /// built.** The builder is consulted two statements later, and a platform
    /// without one returns null and nothing is inserted -- but the toolbar has
    /// already gone. The gesture path never reaches this on a desktop
    /// (`_showMagnifierIfSupportedByPlatform` answers only for Android and
    /// iOS), so it is the public method rather than ordinary use that can do
    /// it, and the method's own doc invites the call by saying it is "safe to
    /// call on platforms not mobile".
    pub fn show(&mut self, toolbar_visible: bool, builder_gives_one: bool) -> ShowMagnifier {
        if self.exists {
            return ShowMagnifier {
                already_there: true,
                hides_toolbar: false,
                inserts: false,
            };
        }
        let hides_toolbar = toolbar_visible;
        if !builder_gives_one {
            return ShowMagnifier {
                already_there: false,
                hides_toolbar,
                inserts: false,
            };
        }
        self.exists = true;
        self.shown = true;
        ShowMagnifier {
            already_there: false,
            hides_toolbar,
            inserts: true,
        }
    }

    /// Upstream's `hideMagnifier`, whose guard is the same one and whose
    /// comment says why:
    ///
    /// ```dart
    /// // This cannot be a check on `MagnifierController.shown`, since
    /// // it's possible that the magnifier is still in the overlay, but
    /// // not shown in cases where the magnifier hides itself.
    /// if (_magnifierController.overlayEntry == null) { return; }
    /// ```
    ///
    /// A magnifier that hid itself is still **there**, and a hide that asked
    /// `shown` would leave that entry in the overlay for ever. Both ends of
    /// this pair are about existence; visibility is the magnifier's own
    /// business.
    ///
    /// Returns whether there was anything to take down.
    pub fn hide(&mut self) -> bool {
        if !self.exists {
            return false;
        }
        self.exists = false;
        self.shown = false;
        true
    }

    /// Upstream's `magnifierIsVisible`.
    pub fn is_visible(&self) -> bool {
        self.shown
    }

    /// Upstream's `magnifierExists`.
    pub fn exists(&self) -> bool {
        self.exists
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

    /// Where inside the handle the finger landed, if a drag is under way.
    ///
    /// The grab is what stops a handle jumping under the finger: upstream
    /// keeps it for the whole drag rather than re-deriving a position from
    /// the handle's middle each move.
    pub fn grab_offset(&self) -> Option<Offset> {
        self.drag_offset
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

/// The two line heights that size a selection's rectangle: upstream's
/// `startGlyphHeight` / `endGlyphHeight`, from `EditableTextState.getGlyphHeights`
/// and `TextSelectionOverlay._getStartGlyphHeight` / `_getEndGlyphHeight`.
///
/// # Why the two ends are measured separately
///
/// Not for symmetry. A selection dragged from a heading into the paragraph
/// under it has ends set in different sizes, and each handle has to be as tall
/// as *its own* end -- a handle sized by the field's line height would stand
/// off the small end and be swallowed by the large one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphHeights {
    pub start: f32,
    pub end: f32,
}

/// Upstream's `getGlyphHeights`.
///
/// # Three ways out, and all three answer the field's line height
///
/// Upstream measures the first and last glyph of the selection, but only after
/// getting past a guard that refuses in three cases -- and the guard is the
/// part worth porting, because each clause is there for a different accident:
///
/// * **`prevText != currText`** -- the render object is the *previous frame's*.
///   Measuring a range against text that has since changed is not a smaller
///   answer, it is a wrong one, and upstream's comment says
///   `getRectForComposingRange` "might fail" outright.
/// * **the selection is invalid** -- `(-1, -1)`, a field that has never been
///   placed in. There is no first glyph to measure.
/// * **the selection is collapsed** -- a caret selects no glyph at all. Note
///   this is not the same as the previous clause: a collapsed selection at
///   offset 4 is perfectly valid, and still has nothing to measure.
///
/// Past the guard there is a fourth fallback, and it is **per end**:
/// `startCharacterRect?.height ?? preferredLineHeight`. One end may measure
/// while the other does not.
///
/// The `Option<Rect>` arguments are what [`crate::editable::ComposingRegion::rect`]
/// answers for the range covering that end's glyph. Upstream takes the first
/// and last *extended grapheme cluster* of the selection rather than the first
/// and last code unit -- an emoji or a combining mark at the boundary would
/// otherwise be cut in half and measured wrong. This crate has no grapheme
/// segmenter (see the note in `services::text_boundary`), so the range is the
/// caller's to build; what is ported here is the choosing.
pub fn glyph_heights(
    text_unchanged: bool,
    selection_valid: bool,
    collapsed: bool,
    start_rect: Option<crate::engine::Rect>,
    end_rect: Option<crate::engine::Rect>,
    preferred_line_height: f32,
) -> GlyphHeights {
    if !text_unchanged || !selection_valid || collapsed {
        return GlyphHeights {
            start: preferred_line_height,
            end: preferred_line_height,
        };
    }
    GlyphHeights {
        start: start_rect.map_or(preferred_line_height, |rect| rect.height()),
        end: end_rect.map_or(preferred_line_height, |rect| rect.height()),
    }
}

/// Upstream's `TextSelectionToolbarAnchors.getSelectionRect`: the rectangle a
/// selection covers, in global coordinates, which is what
/// [`crate::text_selection_controls::TextSelectionToolbarAnchors::from_selection`]
/// then points the toolbar at.
///
/// # A selection over two lines is as wide as the field, not as wide as its ends
///
/// The endpoints of a wrapped selection say nothing useful about its width: the
/// first is somewhere in the middle of one line, the last somewhere in the
/// middle of another, and the *lines between them run edge to edge*. So once
/// the selection is multiline upstream throws both `dx` away and spans the
/// whole editing region. The toolbar then centres over the field rather than
/// over an arbitrary pair of columns.
///
/// # Multiline is decided by half of the **end** glyph's height
///
/// `last.dy - first.dy > endGlyphHeight / 2`. Three things about that:
///
/// * It is a **vertical distance**, not a line index -- there is no line number
///   to hand at this point, only two points.
/// * The **half** is the tolerance. Two ends on the same line need not share a
///   `dy` exactly; a superscript or a taller run shifts one of them, and a bare
///   `> 0` would call an ordinary one-line selection multiline.
/// * It is the **end**'s height, not the start's and not their larger. Dragging
///   from a heading down into small text, the threshold is the small text's.
///
/// # The top walks up and the bottom does not
///
/// `top = first.dy - startGlyphHeight`, `bottom = last.dy`. The asymmetry is
/// not a bug to be tidied: a `TextSelectionPoint`'s `dy` is the *bottom* of its
/// line, so the bottom edge is already right and only the top has to climb a
/// glyph to reach the top of the first line. And it climbs by the **start**'s
/// height, the other half of the pair.
///
/// A `NaN` anywhere in the editing region answers an empty rectangle rather
/// than propagating -- a NaN in a `Rect` would go on to poison every comparison
/// the toolbar layout makes, and `Rect::ZERO` is the value `from_selection`
/// already reads as "there is nothing to point at".
pub fn selection_rect(
    editing_region: crate::engine::Rect,
    endpoints: &[TextSelectionPoint],
    heights: GlyphHeights,
) -> crate::engine::Rect {
    let empty = crate::engine::Rect::ltrb(0.0, 0.0, 0.0, 0.0);
    if editing_region.left.is_nan()
        || editing_region.top.is_nan()
        || editing_region.right.is_nan()
        || editing_region.bottom.is_nan()
    {
        return empty;
    }
    let (first, last) = match (endpoints.first(), endpoints.last()) {
        (Some(first), Some(last)) => (first, last),
        _ => return empty,
    };

    let multiline = last.point.dy - first.point.dy > heights.end / 2.0;
    crate::engine::Rect::ltrb(
        if multiline {
            editing_region.left
        } else {
            editing_region.left + first.point.dx
        },
        editing_region.top + first.point.dy - heights.start,
        if multiline {
            editing_region.right
        } else {
            editing_region.left + last.point.dx
        },
        editing_region.top + last.point.dy,
    )
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

/// One press of the up or down arrow -- upstream
/// `RenderEditable.getTextPositionAbove` and `getTextPositionBelow`.
///
/// The two numbers are **not** symmetric, and upstream says why: "The caret
/// offset gives a location in the upper left hand corner of the caret so the
/// middle of the line above is a half line above that point and the line below
/// is 1.5 lines below that point."
///
/// A caret's offset is its *top*. From there the middle of the line above is
/// half a line up and the middle of the line below is a line and a half down.
/// Minus one and plus one land on a line boundary instead of inside a line,
/// and which line the hit test picks there is a coin toss.
pub struct VerticalCaretStep;

impl VerticalCaretStep {
    /// Upstream's `-0.5 * preferredLineHeight`.
    pub const ABOVE: f32 = -0.5;
    /// Upstream's `1.5 * preferredLineHeight`.
    pub const BELOW: f32 = 1.5;

    /// The point to hit-test for the line above. Only the y moves.
    pub fn above(caret: Offset, preferred_line_height: f32) -> Offset {
        Offset::new(
            caret.dx,
            caret.dy + VerticalCaretStep::ABOVE * preferred_line_height,
        )
    }

    /// The point to hit-test for the line below.
    pub fn below(caret: Offset, preferred_line_height: f32) -> Offset {
        Offset::new(
            caret.dx,
            caret.dy + VerticalCaretStep::BELOW * preferred_line_height,
        )
    }
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

    /// Where the caret lands on `line` -- upstream's `_getTextPositionForLine`.
    ///
    /// The y is the line's **baseline**, not its top and not its middle. The x
    /// is the sticky column, carried through untouched.
    ///
    /// `None` for a line that is not there, which is how the walk in
    /// [`VerticalCaretMovementRun::move_by_offset`] knows it has run out.
    pub fn offset_for_line(&self, line: usize, baselines: &[f32]) -> Option<(f32, f32)> {
        baselines
            .get(line)
            .map(|baseline| (self.origin_x, *baseline))
    }

    /// Upstream's `moveByOffset`: move by a number of **pixels** rather than a
    /// number of lines, which is what a page up or down is.
    ///
    /// Lines are not all the same height, so this steps one line at a time
    /// until it has gone far enough rather than dividing. Three things follow
    /// from the shape of that loop:
    ///
    /// * it stops at the **first** line at or past the target, because the
    ///   condition is tested on the line it is standing on before it moves;
    /// * reaching the top or bottom **breaks** rather than failing, so a page
    ///   down near the end moves as far as it can and reports success;
    /// * an offset of zero moves nothing and returns **false**, because the
    ///   condition is already false on entry.
    ///
    /// The return value is upstream's `initialOffset != _currentOffset`:
    /// whether anything moved, not whether it moved the whole way.
    pub fn move_by_offset(&mut self, offset: f32, baselines: &[f32]) -> bool {
        let Some((_, initial)) = self.offset_for_line(self.current_line, baselines) else {
            return false;
        };
        let target = initial + offset;
        if offset >= 0.0 {
            while self
                .offset_for_line(self.current_line, baselines)
                .is_some_and(|(_, dy)| dy < target)
            {
                if !self.move_next() {
                    break;
                }
            }
        } else {
            while self
                .offset_for_line(self.current_line, baselines)
                .is_some_and(|(_, dy)| dy > target)
            {
                if !self.move_previous() {
                    break;
                }
            }
        }
        self.offset_for_line(self.current_line, baselines)
            .map(|(_, dy)| dy)
            != Some(initial)
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

    // -- Stylus handwriting is asked for twice, tick 285 ---------------------
    //
    // The one arm of onTapDown this port did not already answer for. The rest
    // of that method is shows_selection_toolbar, shows_selection_handles,
    // caret_moves_on, shift_tap_down and
    // shift_tap_expands_from_zero_when_unfocused.

    #[test]
    fn only_android_asks_about_stylus_handwriting() {
        // iOS has Scribble and reaches it another way; the other four arms of
        // the switch do not mention a stylus at all.
        use crate::editable_text::TargetPlatform;
        assert!(TextSelectionGestures::asks_about_stylus_handwriting(
            TargetPlatform::Android,
            PointerKind::Stylus,
            true
        ));
        for platform in [
            TargetPlatform::IOS,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            assert!(
                !TextSelectionGestures::asks_about_stylus_handwriting(
                    platform,
                    PointerKind::Stylus,
                    true
                ),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn both_stylus_kinds_ask_and_nothing_else_does() {
        // An inverted stylus is the same instrument turned round, and
        // upstream's switch lists it beside the ordinary one.
        use crate::editable_text::TargetPlatform;
        for kind in [PointerKind::Stylus, PointerKind::InvertedStylus] {
            assert!(
                TextSelectionGestures::asks_about_stylus_handwriting(
                    TargetPlatform::Android,
                    kind,
                    true
                ),
                "{kind:?}"
            );
        }
        for kind in [
            PointerKind::Touch,
            PointerKind::Mouse,
            PointerKind::Trackpad,
            PointerKind::Unknown,
        ] {
            assert!(
                !TextSelectionGestures::asks_about_stylus_handwriting(
                    TargetPlatform::Android,
                    kind,
                    true
                ),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_field_with_handwriting_switched_off_never_asks() {
        // The widget's own flag is the first gate, before the pointer is even
        // looked at.
        use crate::editable_text::TargetPlatform;
        assert!(!TextSelectionGestures::asks_about_stylus_handwriting(
            TargetPlatform::Android,
            PointerKind::Stylus,
            false
        ));
    }

    #[test]
    fn asking_is_not_starting() {
        // The third gate is a channel round trip. A stylus going down on an
        // Android field with handwriting enabled moves no caret on that
        // frame: the handler asks and returns, and the selection changes later
        // from inside the callback -- and only if the answer was yes.
        assert!(!TextSelectionGestures::stylus_handwriting_starts(
            true, false
        ));
        assert!(TextSelectionGestures::stylus_handwriting_starts(true, true));
    }

    #[test]
    fn a_platform_that_says_yes_to_something_that_was_never_asked_starts_nothing() {
        // The two gates are conjunctive: an available feature does not start
        // handwriting for a finger.
        assert!(!TextSelectionGestures::stylus_handwriting_starts(
            false, true
        ));
    }

    #[test]
    fn the_gate_asks_whether_the_build_can_and_not_whether_it_is_ready() {
        // Scribe.isFeatureAvailable, not Scribe.isStylusHandwritingAvailable.
        // Both are on the channel here and they are different questions.
        assert_eq!(
            TextSelectionGestures::STYLUS_HANDWRITING_GATE,
            "Scribe.isFeatureAvailable"
        );
        assert_ne!(
            TextSelectionGestures::STYLUS_HANDWRITING_GATE,
            crate::services::system_channels::Scribe::IS_STYLUS_HANDWRITING_AVAILABLE
        );
    }

    #[test]
    fn the_selection_change_carries_its_own_cause() {
        // Not `tap`: the change came from the platform's recogniser rather
        // than from the finger.
        assert_eq!(
            TextSelectionGestures::stylus_handwriting_cause(),
            crate::services::text_input::SelectionChangedCause::StylusHandwriting
        );
        assert_ne!(
            TextSelectionGestures::stylus_handwriting_cause(),
            crate::services::text_input::SelectionChangedCause::Tap
        );
    }

    // -- What one tap does, tick 284 -----------------------------------------
    //
    // The long iOS-touch tail is not tested here: `tap_outcome` and
    // `after_selecting_the_word_edge` already own it, with their own tests.
    // The first version of this block re-implemented and re-tested that tail
    // before noticing, which is what the whole-crate member check is for --
    // and it did not catch this one, because the tail had been ported under a
    // name no upstream member has.

    fn tap(platform: crate::editable_text::TargetPlatform) -> SingleTapContext {
        SingleTapContext {
            selection_enabled: true,
            platform,
            shift_pressed: false,
            selection_base_valid: true,
            has_focus: true,
            pointer: crate::gestures::PointerKind::Touch,
            read_only: false,
            misspelled: false,
            previous_selection: (2, 6),
            tapped_offset: 4,
            affinity_same: true,
        }
    }

    const EVERY_PLATFORM: [crate::editable_text::TargetPlatform; 6] = [
        crate::editable_text::TargetPlatform::Android,
        crate::editable_text::TargetPlatform::Fuchsia,
        crate::editable_text::TargetPlatform::IOS,
        crate::editable_text::TargetPlatform::Linux,
        crate::editable_text::TargetPlatform::MacOS,
        crate::editable_text::TargetPlatform::Windows,
    ];

    #[test]
    fn a_field_that_cannot_be_selected_in_still_takes_focus() {
        // requestKeyboard() is called and *then* the method returns.
        for platform in EVERY_PLATFORM {
            let mut context = tap(platform);
            context.selection_enabled = false;
            let outcome = SingleTapUp::decide(&context);
            assert!(outcome.requests_keyboard, "{platform:?}");
            assert_eq!(outcome.action, SingleTapAction::Nothing, "{platform:?}");
        }
    }

    #[test]
    fn the_desktop_platforms_do_nothing_but_still_ask_for_the_keyboard() {
        // "On desktop platforms the selection is set on tap down." The arm
        // breaks, and the method's last line still runs.
        use crate::editable_text::TargetPlatform;
        for platform in [
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            let outcome = SingleTapUp::decide(&tap(platform));
            assert_eq!(outcome.action, SingleTapAction::Nothing, "{platform:?}");
            assert!(outcome.requests_keyboard, "{platform:?}");
        }
    }

    #[test]
    fn a_shift_tap_is_the_one_path_that_does_not_focus_the_field() {
        // Both shift arms `return` from inside the switch, so they never reach
        // the trailing requestKeyboard().
        use crate::editable_text::TargetPlatform;
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
        ] {
            let mut context = tap(platform);
            context.shift_pressed = true;
            let outcome = SingleTapUp::decide(&context);
            assert!(!outcome.requests_keyboard, "{platform:?}");
        }
    }

    #[test]
    fn shift_without_a_selection_to_extend_from_is_not_shift() {
        // "It is impossible to extend the selection when the shift key is
        // pressed, if the renderEditable.selection is invalid." Both halves.
        let mut context = tap(crate::editable_text::TargetPlatform::Android);
        context.shift_pressed = true;
        context.selection_base_valid = false;
        assert!(!SingleTapUp::shift_is_usable(&context));
        let outcome = SingleTapUp::decide(&context);
        assert_eq!(
            outcome.action,
            SingleTapAction::SelectPosition {
                spell_check_toolbar: true
            },
            "the ordinary path, as though shift were not held"
        );
        assert!(outcome.requests_keyboard, "and it focuses again");
    }

    #[test]
    fn android_and_fuchsia_differ_by_exactly_one_line() {
        // Both hide the toolbar, both extend on shift, both select the
        // position. Android alone raises the spell check toolbar afterwards.
        use crate::editable_text::TargetPlatform;
        let android = SingleTapUp::decide(&tap(TargetPlatform::Android));
        let fuchsia = SingleTapUp::decide(&tap(TargetPlatform::Fuchsia));
        assert_eq!(
            android.action,
            SingleTapAction::SelectPosition {
                spell_check_toolbar: true
            }
        );
        assert_eq!(
            fuchsia.action,
            SingleTapAction::SelectPosition {
                spell_check_toolbar: false
            }
        );
        assert_eq!(android.hides_toolbar_first, fuchsia.hides_toolbar_first);
        assert_eq!(android.requests_keyboard, fuchsia.requests_keyboard);

        let mut shifted_android = tap(TargetPlatform::Android);
        shifted_android.shift_pressed = true;
        let mut shifted_fuchsia = tap(TargetPlatform::Fuchsia);
        shifted_fuchsia.shift_pressed = true;
        assert_eq!(
            SingleTapUp::decide(&shifted_android),
            SingleTapUp::decide(&shifted_fuchsia),
            "identical on the shift path"
        );
    }

    #[test]
    fn only_android_and_fuchsia_put_the_toolbar_away_first() {
        use crate::editable_text::TargetPlatform;
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            assert!(
                SingleTapUp::decide(&tap(platform)).hides_toolbar_first,
                "{platform:?}"
            );
        }
        for platform in [
            TargetPlatform::IOS,
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            assert!(
                !SingleTapUp::decide(&tap(platform)).hides_toolbar_first,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_shift_tapped_unfocused_ios_field_expands_from_the_beginning() {
        // "On iOS, a shift-tapped unfocused field expands from 0, not from the
        // previous selection."
        let mut context = tap(crate::editable_text::TargetPlatform::IOS);
        context.shift_pressed = true;

        context.has_focus = false;
        assert_eq!(
            SingleTapUp::decide(&context).action,
            SingleTapAction::ExpandSelection { from_zero: true }
        );
        context.has_focus = true;
        assert_eq!(
            SingleTapUp::decide(&context).action,
            SingleTapAction::ExpandSelection { from_zero: false },
            "a focused field expands from what was selected"
        );
    }

    #[test]
    fn ios_shift_expands_where_the_others_extend() {
        // Two different verbs, and only iOS uses the expanding one.
        use crate::editable_text::TargetPlatform;
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            let mut context = tap(platform);
            context.shift_pressed = true;
            assert_eq!(
                SingleTapUp::decide(&context).action,
                SingleTapAction::ExtendSelection,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_precise_pointers_place_the_cursor_and_the_rest_do_not() {
        // Mouse, trackpad and both stylus kinds are precise; touch and unknown
        // go to `tap_outcome` instead.
        use crate::gestures::PointerKind;
        let mut context = tap(crate::editable_text::TargetPlatform::IOS);
        for pointer in [
            PointerKind::Mouse,
            PointerKind::Trackpad,
            PointerKind::Stylus,
            PointerKind::InvertedStylus,
        ] {
            assert!(SingleTapUp::is_precise(pointer), "{pointer:?}");
            context.pointer = pointer;
            assert_eq!(
                SingleTapUp::decide(&context).action,
                SingleTapAction::SelectPositionAndHideToolbar,
                "{pointer:?}"
            );
        }
        for pointer in [PointerKind::Touch, PointerKind::Unknown] {
            assert!(!SingleTapUp::is_precise(pointer), "{pointer:?}");
            context.pointer = pointer;
            assert!(
                matches!(
                    SingleTapUp::decide(&context).action,
                    SingleTapAction::Touch(_)
                ),
                "{pointer:?}"
            );
        }
    }

    #[test]
    fn a_precise_pointer_never_reaches_the_touch_branch() {
        // The pointer table is asked before anything `tap_outcome` looks at,
        // so a mouse click on a misspelled word places the cursor rather than
        // selecting the word.
        let mut context = tap(crate::editable_text::TargetPlatform::IOS);
        context.pointer = crate::gestures::PointerKind::Mouse;
        context.misspelled = true;
        assert_eq!(
            SingleTapUp::decide(&context).action,
            SingleTapAction::SelectPositionAndHideToolbar
        );
    }

    #[test]
    fn a_touch_on_ios_is_handed_to_the_outcome_that_already_answers_for_it() {
        // Not a second implementation: the same `tap_outcome` this file has
        // had, reached through the platform table.
        let mut context = tap(crate::editable_text::TargetPlatform::IOS);
        context.misspelled = true;
        assert_eq!(
            SingleTapUp::decide(&context).action,
            SingleTapAction::Touch(TapOutcome::SelectWordAndOfferSpelling)
        );

        context.misspelled = false;
        assert_eq!(
            SingleTapUp::decide(&context).action,
            SingleTapAction::Touch(tap_outcome(
                false,
                context.previous_selection,
                context.tapped_offset,
                context.affinity_same,
                context.read_only,
                context.has_focus,
            ))
        );
    }

    #[test]
    fn only_ios_reaches_the_touch_branch_at_all() {
        // Android and Fuchsia never consult the pointer kind, so a touch there
        // takes the ordinary select-position path.
        use crate::editable_text::TargetPlatform;
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            let mut context = tap(platform);
            context.misspelled = true;
            assert!(
                !matches!(
                    SingleTapUp::decide(&context).action,
                    SingleTapAction::Touch(_)
                ),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_two_follow_ups_read_a_changed_selection_opposite_ways() {
        // After a misspelled word, a *change* means show. After a word edge, a
        // change means hide. The same fact, the other way round.
        assert_eq!(
            SingleTapUp::after_misspelled_word(true),
            AfterMisspelledWord::ShowSpellCheckToolbar
        );
        assert_eq!(
            SingleTapUp::after_misspelled_word(false),
            AfterMisspelledWord::ToggleToolbar,
            "a second tap on an already-selected misspelling puts it away"
        );
        assert_eq!(
            after_selecting_the_word_edge(true, false, true),
            AfterWordEdge::HideToolbar,
            "the opposite sense"
        );
        assert_eq!(
            after_selecting_the_word_edge(false, false, true),
            AfterWordEdge::ToggleToolbar
        );
    }

    // -- What a triple tap selects, tick 283 ---------------------------------

    // The **real** `ParagraphBoundary`, not a stand-in. The first version of
    // these tests used a hand-rolled one that walked to the nearest newline
    // and clamped, and three mutations of the minus-one rule read green
    // against it -- because that stand-in answered 4 at the end of the text
    // where the real boundary answers 7, so the adjustment had nothing to fix.
    //
    // A stand-in that is kinder than the real thing hides the rule the real
    // thing needs.
    fn paragraphs(text: &str) -> crate::services::text_boundary::ParagraphBoundary<'_> {
        crate::services::text_boundary::ParagraphBoundary::new(text)
    }

    #[test]
    fn a_one_line_field_selects_everything_before_the_platform_is_asked() {
        // In a field that cannot wrap, the paragraph and the line and the
        // whole text are the same thing, and upstream says so with selectAll.
        use crate::editable_text::TargetPlatform;
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
            TargetPlatform::Linux,
        ] {
            assert_eq!(
                BoundarySelection::triple_tap(Some(1), platform),
                TripleTapSelects::Everything,
                "{platform:?}: even Linux"
            );
        }
    }

    #[test]
    fn linux_alone_selects_a_line_and_everyone_else_a_paragraph() {
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            BoundarySelection::triple_tap(Some(4), TargetPlatform::Linux),
            TripleTapSelects::Line
        );
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            assert_eq!(
                BoundarySelection::triple_tap(Some(4), platform),
                TripleTapSelects::Paragraph,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn an_unbounded_field_is_not_a_one_line_field() {
        // maxLines == null is the growing field, and it takes the platform
        // branch like any other multi-line one.
        use crate::editable_text::TargetPlatform;
        assert_eq!(
            BoundarySelection::triple_tap(None, TargetPlatform::Linux),
            TripleTapSelects::Line
        );
        assert_eq!(
            BoundarySelection::triple_tap(None, TargetPlatform::Android),
            TripleTapSelects::Paragraph
        );
    }

    #[test]
    fn a_caret_at_the_very_end_steps_back_one_to_find_its_paragraph() {
        // Upstream: "Use extent.offset - 1 when `extent` is at the end of the
        // text to retrieve the previous text boundary's location." Without it
        // the leading lookup at the end answers the end, and the range is
        // empty.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        let length = text.len() as isize; // 7
        let range = BoundarySelection::move_to_boundary(length, length, &paragraphs);
        assert_eq!(
            (range.start, range.end),
            (4, 7),
            "the last paragraph, not an empty range at 7"
        );
    }

    #[test]
    fn the_step_back_happens_only_at_the_end_of_the_text() {
        // One character earlier there is no adjustment, and the answer is the
        // same because that position is inside the same paragraph -- so the
        // rule is about the boundary case alone.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        let inside = BoundarySelection::move_to_boundary(5, 7, &paragraphs);
        assert_eq!((inside.start, inside.end), (4, 7));
    }

    #[test]
    fn a_caret_on_the_first_character_of_a_paragraph_is_not_stepped_back() {
        // The adjustment is for the end of the text and nowhere else. Offset 4
        // is the first character of the second paragraph and offset 3 is the
        // terminator that ends the first, so this is the position where a
        // step back would change the answer -- and it must not.
        let text = "one
two";
        let paragraphs = paragraphs(text);
        let range = BoundarySelection::move_to_boundary(4, 7, &paragraphs);
        assert_eq!(range.start, 4, "the second paragraph, not the first");
    }

    #[test]
    fn only_the_leading_lookup_is_adjusted() {
        // The trailing lookup walks forwards, and forwards from the end is the
        // end. Adjusting it too would cut the last character off every
        // triple tap at the end of the text.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        let range = BoundarySelection::move_to_boundary(7, 7, &paragraphs);
        assert_eq!(range.end, 7, "the whole of the last paragraph");
    }

    #[test]
    fn the_two_fallbacks_go_to_opposite_ends_of_the_text() {
        // Leading falls back to 0 and trailing to the text's length. A
        // boundary that answers nothing at either side should select
        // everything, not collapse.
        struct Silent;
        impl crate::services::text_boundary::TextBoundary for Silent {
            fn text_boundary_at(
                &self,
                _position: isize,
            ) -> crate::services::text_boundary::TextRange {
                crate::services::text_boundary::TextRange { start: -1, end: -1 }
            }
        }
        let range = BoundarySelection::move_to_boundary(3, 7, &Silent);
        assert_eq!((range.start, range.end), (0, 7));
    }

    #[test]
    fn a_tap_that_never_became_a_drag_selects_one_boundary() {
        // `to: null` makes the far end the near end.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        assert_eq!(
            BoundarySelection::in_range(1, None, 7, &paragraphs),
            (0, 4),
            "the first paragraph, its terminator included"
        );
    }

    #[test]
    fn a_drag_across_paragraphs_takes_both_whole() {
        // From inside the first to inside the second: the first's start to the
        // second's end, so neither is cut in half.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        assert_eq!(
            BoundarySelection::in_range(1, Some(5), 7, &paragraphs),
            (0, 7)
        );
    }

    #[test]
    fn a_backwards_drag_selects_the_same_span_the_other_way_about() {
        // `fromRange.start < toRange.end` again, one boundary kind up from
        // selectWordsInRange: the handles stay on the ends the finger put
        // them.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        let forwards = BoundarySelection::in_range(1, Some(5), 7, &paragraphs);
        let backwards = BoundarySelection::in_range(5, Some(1), 7, &paragraphs);
        assert_eq!(forwards, (0, 7));
        assert_eq!(backwards, (7, 0), "the same span, base and extent swapped");
    }

    #[test]
    fn the_far_end_landing_in_the_same_boundary_is_the_same_selection() {
        // Dragging within one paragraph selects that paragraph and no more,
        // however far the finger moves inside it.
        let text = "one\ntwo";
        let paragraphs = paragraphs(text);
        assert_eq!(
            BoundarySelection::in_range(0, Some(2), 7, &paragraphs),
            (0, 4)
        );
        assert_eq!(
            BoundarySelection::in_range(2, Some(0), 7, &paragraphs),
            (0, 4)
        );
    }

    // -- Dragging a handle needs two flags, tick 282 -------------------------

    use crate::editable_text::TargetPlatform;
    const START: SelectionHandleEnd = SelectionHandleEnd::Start;
    const END: SelectionHandleEnd = SelectionHandleEnd::End;
    const TOUCH: bool = true;
    const MOUSE: bool = false;
    const HANDLES: bool = true;

    #[test]
    fn a_mouse_drag_is_in_progress_without_being_a_handle_drag() {
        // `_startHandleDragInProgress` is set for any pointer;
        // `_isDraggingStartHandle` only for a touch. The public getter is the
        // or of the two, so from outside both read as dragging -- and only
        // inside can the platform's one-at-a-time rule tell them apart.
        let mut mouse = HandleDragState::new();
        mouse.drag_start(START, HANDLES, MOUSE, TargetPlatform::IOS, false);
        assert!(mouse.is_dragging(START), "a drag, as far as anyone outside");
        // The end handle is still free, which it would not be if the mouse
        // drag had set the inner flag.
        assert!(
            mouse.can_drag(END, TargetPlatform::IOS, false),
            "a mouse on one handle does not block the other, even on iOS"
        );

        let mut touch = HandleDragState::new();
        touch.drag_start(START, HANDLES, TOUCH, TargetPlatform::IOS, false);
        assert!(
            !touch.can_drag(END, TargetPlatform::IOS, false),
            "a touch does block it"
        );
    }

    #[test]
    fn only_apple_and_the_web_allow_one_handle_at_a_time() {
        let mut state = HandleDragState::new();
        state.drag_start(END, HANDLES, TOUCH, TargetPlatform::Android, false);
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(
                !state.can_drag(START, platform, false),
                "{platform:?} blocks the other handle"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert!(
                state.can_drag(START, platform, false),
                "{platform:?} lets both move"
            );
            assert!(
                !state.can_drag(START, platform, true),
                "{platform:?} on the web does not"
            );
        }
    }

    #[test]
    fn an_idle_opposite_handle_leaves_the_guard_open_on_every_platform() {
        // The guard's first clause is `!_isDraggingEndHandle`, so the platform
        // question only arises when the other handle is actually down.
        let state = HandleDragState::new();
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(state.can_drag(START, platform, true), "{platform:?} on web");
        }
    }

    #[test]
    fn a_blocked_start_raises_nothing_but_still_records_the_gesture() {
        // `_startHandleDragInProgress = true` happens *before* the guard, so a
        // blocked drag is still a drag in progress -- which is what the public
        // getter reports.
        let mut state = HandleDragState::new();
        state.drag_start(END, HANDLES, TOUCH, TargetPlatform::IOS, false);
        let raised = state.drag_start(START, HANDLES, TOUCH, TargetPlatform::IOS, false);
        assert_eq!(raised, None, "blocked, so no callback");
        assert!(state.is_dragging(START), "and yet in progress");
    }

    #[test]
    fn an_update_synthesises_the_start_a_blocked_drag_never_got() {
        // Upstream: "The handle drag may have been blocked before on Apple
        // platforms and the web while the opposite handle was being dragged.
        // Ensure that any logic that was meant to be run in
        // onStartHandleDragStart is still run." Without this the drag would
        // move without ever having begun.
        let mut state = HandleDragState::new();
        state.drag_start(END, HANDLES, TOUCH, TargetPlatform::IOS, false);
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::IOS, false);
        // The end handle is released, so the start handle is free again.
        state.drag_end(END, HANDLES, TargetPlatform::IOS, false);

        let raised = state.drag_update(START, HANDLES, TOUCH, TargetPlatform::IOS, false);
        assert_eq!(
            raised,
            vec![
                HandleDragCallback::Start(START),
                HandleDragCallback::Update(START)
            ],
            "the start first, then the update"
        );
    }

    #[test]
    fn an_update_on_a_drag_that_did_begin_raises_only_the_update() {
        let mut state = HandleDragState::new();
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::Android, false);
        assert_eq!(
            state.drag_update(START, HANDLES, TOUCH, TargetPlatform::Android, false),
            vec![HandleDragCallback::Update(START)],
            "no second start"
        );
    }

    #[test]
    fn a_blocked_update_raises_nothing_at_all() {
        let mut state = HandleDragState::new();
        state.drag_start(END, HANDLES, TOUCH, TargetPlatform::IOS, false);
        assert_eq!(
            state.drag_update(START, HANDLES, TOUCH, TargetPlatform::IOS, false),
            Vec::new(),
            "not even the synthesised start"
        );
    }

    #[test]
    fn a_gesture_after_the_handles_are_gone_is_dropped_and_clears_the_flag() {
        // Upstream: "Calling OverlayEntry.remove may not happen until the
        // following frame, so it's possible for the handles to receive a
        // gesture after calling remove."
        let mut state = HandleDragState::new();
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::Android, false);
        assert_eq!(
            state.drag_update(START, false, TOUCH, TargetPlatform::Android, false),
            Vec::new()
        );
        assert!(
            state.can_drag(END, TargetPlatform::IOS, false),
            "and the inner flag went down, so the other handle is free"
        );
    }

    #[test]
    fn a_start_after_the_handles_are_gone_never_records_the_gesture() {
        // The null check comes before `in_progress` is set, so nothing is left
        // behind to be cleared later.
        let mut state = HandleDragState::new();
        assert_eq!(
            state.drag_start(START, false, TOUCH, TargetPlatform::Android, false),
            None
        );
        assert!(!state.is_dragging(START));
    }

    #[test]
    fn an_end_clears_the_inner_flag_even_with_the_handles_gone() {
        // `_isDraggingStartHandle = false` is the *first* line of the end
        // handler, before the null check, and `_startHandleDragInProgress` is
        // cleared only after it. That asymmetry is upstream's.
        let mut state = HandleDragState::new();
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::IOS, false);
        assert!(!state.can_drag(END, TargetPlatform::IOS, false), "blocked");

        assert_eq!(
            state.drag_end(START, false, TargetPlatform::IOS, false),
            None,
            "no callback, the handles are gone"
        );
        assert!(
            state.can_drag(END, TargetPlatform::IOS, false),
            "and yet the other handle was released"
        );
        assert!(
            state.is_dragging(START),
            "while the gesture is still recorded as in progress"
        );
    }

    #[test]
    fn an_ordinary_end_clears_both_flags() {
        let mut state = HandleDragState::new();
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::Android, false);
        assert_eq!(
            state.drag_end(START, HANDLES, TargetPlatform::Android, false),
            Some(HandleDragCallback::End(START))
        );
        assert!(!state.is_dragging(START));
    }

    #[test]
    fn a_blocked_end_still_lets_go_of_both_flags() {
        // The guard is checked *after* both are cleared, so only the callback
        // is skipped -- a handle blocked at the moment it is released does not
        // stay stuck down.
        let mut state = HandleDragState::new();
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::IOS, false);
        state.drag_start(END, HANDLES, TOUCH, TargetPlatform::IOS, false);
        // START is down, so END was blocked; now release END.
        assert_eq!(
            state.drag_end(END, HANDLES, TargetPlatform::IOS, false),
            None,
            "blocked, so no callback"
        );
        assert!(!state.is_dragging(END), "and yet fully released");
    }

    #[test]
    fn the_two_handles_keep_their_own_flags() {
        let mut state = HandleDragState::new();
        state.drag_start(START, HANDLES, TOUCH, TargetPlatform::Android, false);
        state.drag_start(END, HANDLES, TOUCH, TargetPlatform::Android, false);
        assert!(state.is_dragging(START) && state.is_dragging(END));
        state.drag_end(START, HANDLES, TargetPlatform::Android, false);
        assert!(!state.is_dragging(START));
        assert!(state.is_dragging(END), "untouched by the other's end");
    }

    // -- Half a line up and one and a half down, tick 279 --------------------

    /// Four lines of uneven height, so a pixel move cannot be a division.
    /// Baselines at 10, 30, 90, 110: a tall third line in the middle.
    const BASELINES: [f32; 4] = [10.0, 30.0, 90.0, 110.0];

    #[test]
    fn the_step_up_and_the_step_down_are_not_the_same_size() {
        // Upstream: "The caret offset gives a location in the upper left hand
        // corner of the caret so the middle of the line above is a half line
        // above that point and the line below is 1.5 lines below that point."
        //
        // A caret's offset is its *top*, so the two directions are measured
        // from different ends of it. Minus one and plus one land on a line
        // boundary instead of inside a line.
        assert_eq!(VerticalCaretStep::ABOVE, -0.5);
        assert_eq!(VerticalCaretStep::BELOW, 1.5);
        assert_ne!(
            VerticalCaretStep::BELOW,
            -VerticalCaretStep::ABOVE,
            "not a mirror of each other"
        );
    }

    #[test]
    fn a_step_moves_a_whole_line_between_the_two_directions() {
        // Half a line up and one and a half down is two whole lines apart,
        // which is what puts one point inside the line above and the other
        // inside the line below rather than both on a boundary.
        let caret = Offset::new(40.0, 100.0);
        let up = VerticalCaretStep::above(caret, 20.0);
        let down = VerticalCaretStep::below(caret, 20.0);
        assert_eq!(up.dy, 90.0, "into the middle of the line above");
        assert_eq!(down.dy, 130.0, "into the middle of the line below");
        assert_eq!(down.dy - up.dy, 40.0, "two lines apart");
    }

    #[test]
    fn a_step_leaves_the_column_alone() {
        // Only the y moves. Moving the x here would fight the sticky column
        // the run exists to keep.
        let caret = Offset::new(40.0, 100.0);
        assert_eq!(VerticalCaretStep::above(caret, 20.0).dx, 40.0);
        assert_eq!(VerticalCaretStep::below(caret, 20.0).dx, 40.0);
    }

    #[test]
    fn a_line_is_found_at_its_baseline_and_at_the_sticky_column() {
        let run = VerticalCaretMovementRun::new(200.0, 0, 4, 1);
        assert_eq!(run.offset_for_line(2, &BASELINES), Some((200.0, 90.0)));
        assert_eq!(run.offset_for_line(9, &BASELINES), None, "no such line");
    }

    #[test]
    fn moving_by_zero_pixels_moves_nothing_and_says_so() {
        // The loop's condition is already false on entry, so nothing happens
        // -- and the caller is told nothing happened, which is what lets a
        // key press fall through to whatever handles it next.
        let mut run = VerticalCaretMovementRun::new(200.0, 1, 4, 1);
        assert!(!run.move_by_offset(0.0, &BASELINES));
        assert_eq!(run.current_line(), 1);
    }

    #[test]
    fn a_pixel_move_stops_at_the_first_line_at_or_past_the_target() {
        // From line 0 (baseline 10) by 50 pixels: the target is 60. Line 1 is
        // at 30, still short; line 2 is at 90, past it. It stops there rather
        // than at line 1, the last one *before* the target.
        let mut run = VerticalCaretMovementRun::new(200.0, 0, 4, 1);
        assert!(run.move_by_offset(50.0, &BASELINES));
        assert_eq!(run.current_line(), 2);
    }

    #[test]
    fn a_pixel_move_is_a_walk_and_not_a_division() {
        // The lines are 20, 60 and 20 apart. Fifty pixels from line 0 reaches
        // line 2; fifty pixels from line 2 reaches only line 3. A port that
        // divided by a line height would move the same number of lines both
        // times.
        let mut from_top = VerticalCaretMovementRun::new(200.0, 0, 4, 1);
        from_top.move_by_offset(50.0, &BASELINES);
        assert_eq!(from_top.current_line(), 2, "two lines");

        let mut from_middle = VerticalCaretMovementRun::new(200.0, 2, 4, 1);
        from_middle.move_by_offset(50.0, &BASELINES);
        assert_eq!(from_middle.current_line(), 3, "one line, same pixels");
    }

    #[test]
    fn a_negative_offset_walks_the_other_way() {
        let mut run = VerticalCaretMovementRun::new(200.0, 3, 4, 1);
        assert!(run.move_by_offset(-50.0, &BASELINES));
        assert_eq!(run.current_line(), 1, "110 - 50 is 60; line 1 is at 30");
    }

    #[test]
    fn running_into_the_end_moves_as_far_as_it_can_and_reports_success() {
        // The loop breaks rather than returning false. A page down near the
        // bottom of a field should land on the last line, not refuse.
        let mut run = VerticalCaretMovementRun::new(200.0, 0, 4, 1);
        assert!(
            run.move_by_offset(10_000.0, &BASELINES),
            "it did move, even though not the whole way"
        );
        assert_eq!(run.current_line(), 3, "as far as there is");
    }

    #[test]
    fn a_move_from_the_last_line_downwards_fails_without_moving() {
        // Already at the end: nothing moved, so false -- the distinction the
        // break above does *not* erase.
        let mut run = VerticalCaretMovementRun::new(200.0, 3, 4, 1);
        assert!(!run.move_by_offset(10_000.0, &BASELINES));
        assert_eq!(run.current_line(), 3);

        let mut top = VerticalCaretMovementRun::new(200.0, 0, 4, 1);
        assert!(!top.move_by_offset(-10_000.0, &BASELINES));
        assert_eq!(top.current_line(), 0);
    }

    #[test]
    fn a_pixel_move_keeps_the_sticky_column() {
        // The whole reason the run is a type: every line is measured against
        // the column the run started at, however it got there.
        let mut run = VerticalCaretMovementRun::new(200.0, 0, 4, 1);
        run.move_by_offset(50.0, &BASELINES);
        assert_eq!(run.origin_x(), 200.0);
        assert_eq!(
            run.offset_for_line(run.current_line(), &BASELINES)
                .unwrap()
                .0,
            200.0
        );
    }

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

    fn shown(handles_visible: bool, toolbar: bool) -> SelectionOverlay {
        let mut overlay = SelectionOverlay::new().with_handles_visible(handles_visible);
        overlay.set_toolbar_visible(toolbar);
        overlay
    }

    fn rect(l: f32, t: f32, r: f32, b: f32) -> crate::engine::Rect {
        crate::engine::Rect::ltrb(l, t, r, b)
    }

    fn point(dx: f32, dy: f32) -> TextSelectionPoint {
        TextSelectionPoint::new(Offset::new(dx, dy), None)
    }

    #[test]
    fn each_end_of_the_selection_is_measured_on_its_own() {
        // A selection dragged from a heading into the paragraph below it.
        let heights = glyph_heights(
            true,
            true,
            false,
            Some(rect(0.0, 0.0, 10.0, 32.0)),
            Some(rect(0.0, 0.0, 10.0, 14.0)),
            20.0,
        );
        assert_eq!(heights.start, 32.0);
        assert_eq!(heights.end, 14.0);
        assert_ne!(heights.start, heights.end, "not one height used twice");
    }

    #[test]
    fn one_end_may_measure_while_the_other_does_not() {
        // `startCharacterRect?.height ?? preferredLineHeight` is written per
        // end, so the fallback is per end too.
        let heights = glyph_heights(
            true,
            true,
            false,
            None,
            Some(rect(0.0, 0.0, 10.0, 14.0)),
            20.0,
        );
        assert_eq!(heights.start, 20.0, "the field's");
        assert_eq!(heights.end, 14.0, "its own");
    }

    #[test]
    fn three_separate_reasons_to_refuse_to_measure_at_all() {
        // Stale text, an invalid selection, and a caret. Each clause on its
        // own, with the other two satisfied, so none of them can be carried by
        // its neighbour.
        let tall = Some(rect(0.0, 0.0, 10.0, 32.0));
        let field = 20.0;
        for (name, heights) in [
            (
                "last frame's text",
                glyph_heights(false, true, false, tall, tall, field),
            ),
            (
                "never placed in",
                glyph_heights(true, false, false, tall, tall, field),
            ),
            (
                "a caret selects no glyph",
                glyph_heights(true, true, true, tall, tall, field),
            ),
        ] {
            assert_eq!(heights.start, field, "{name}");
            assert_eq!(heights.end, field, "{name}");
        }
        // And with all three satisfied the same rects are measured, so the
        // test above is watching the guard and not an unreachable path.
        assert_eq!(
            glyph_heights(true, true, false, tall, tall, field).start,
            32.0
        );
    }

    #[test]
    fn a_wrapped_selection_is_as_wide_as_the_field() {
        // The lines between the two endpoints run edge to edge, so neither
        // endpoint's dx says anything about the selection's width.
        let region = rect(100.0, 50.0, 400.0, 250.0);
        let ends = [point(200.0, 20.0), point(30.0, 60.0)];
        let wide = selection_rect(
            region,
            &ends,
            GlyphHeights {
                start: 14.0,
                end: 14.0,
            },
        );
        assert_eq!(wide.left, 100.0, "the region's left, not 100 + 200");
        assert_eq!(wide.right, 400.0, "the region's right, not 100 + 30");
        // Note the endpoints even run backwards here (200 then 30), which on
        // one line would give a rectangle with negative width.
        assert!(wide.width() > 0.0);
    }

    #[test]
    fn a_selection_on_one_line_is_as_wide_as_its_two_ends() {
        let region = rect(100.0, 50.0, 400.0, 250.0);
        let ends = [point(30.0, 20.0), point(200.0, 20.0)];
        let narrow = selection_rect(
            region,
            &ends,
            GlyphHeights {
                start: 14.0,
                end: 14.0,
            },
        );
        assert_eq!(
            narrow.left, 130.0,
            "the region's left plus the endpoint's dx"
        );
        assert_eq!(narrow.right, 300.0);
    }

    #[test]
    fn multiline_is_half_the_end_glyph_and_not_any_drop_at_all() {
        // A superscript or a taller run nudges one endpoint's dy without
        // starting a new line, so a bare `> 0` would call a one-line
        // selection multiline.
        let region = rect(100.0, 50.0, 400.0, 250.0);
        let nudged = [point(30.0, 20.0), point(200.0, 24.0)];
        let heights = GlyphHeights {
            start: 14.0,
            end: 14.0,
        };
        assert!(4.0 > 0.0 && 4.0 <= 14.0 / 2.0, "inside the tolerance");
        assert_eq!(
            selection_rect(region, &nudged, heights).left,
            130.0,
            "still one line"
        );

        let wrapped = [point(30.0, 20.0), point(200.0, 28.0)];
        assert!(8.0 > 14.0 / 2.0);
        assert_eq!(selection_rect(region, &wrapped, heights).left, 100.0);
    }

    #[test]
    fn the_end_glyph_decides_the_wrap_and_the_start_glyph_does_not() {
        // Same two points, same pair of heights, swapped between the ends.
        let region = rect(100.0, 50.0, 400.0, 250.0);
        let ends = [point(30.0, 20.0), point(200.0, 26.0)];
        let small_end = selection_rect(
            region,
            &ends,
            GlyphHeights {
                start: 40.0,
                end: 8.0,
            },
        );
        let big_end = selection_rect(
            region,
            &ends,
            GlyphHeights {
                start: 8.0,
                end: 40.0,
            },
        );
        assert_eq!(small_end.left, 100.0, "6 > 4: multiline");
        assert_eq!(big_end.left, 130.0, "6 < 20: one line");
    }

    #[test]
    fn the_top_climbs_a_glyph_and_the_bottom_stays_put() {
        // An endpoint's dy is the bottom of its line, so only the top has to
        // walk up -- and it walks up by the start's height, not the end's.
        let region = rect(100.0, 50.0, 400.0, 250.0);
        let ends = [point(30.0, 20.0), point(200.0, 20.0)];
        let box_ = selection_rect(
            region,
            &ends,
            GlyphHeights {
                start: 14.0,
                end: 9.0,
            },
        );
        assert_eq!(box_.top, 50.0 + 20.0 - 14.0, "the start's height");
        assert_eq!(box_.bottom, 50.0 + 20.0, "no height subtracted or added");
    }

    #[test]
    fn a_region_with_a_nan_edge_has_no_rectangle_rather_than_a_poisoned_one() {
        // Left to propagate, the NaN would reach every comparison the toolbar
        // layout makes. Rect::ZERO is what `from_selection` already reads as
        // "nothing to point at".
        let ends = [point(30.0, 20.0), point(200.0, 20.0)];
        let heights = GlyphHeights {
            start: 14.0,
            end: 14.0,
        };
        let zero = rect(0.0, 0.0, 0.0, 0.0);
        for region in [
            rect(f32::NAN, 50.0, 400.0, 250.0),
            rect(100.0, f32::NAN, 400.0, 250.0),
            rect(100.0, 50.0, f32::NAN, 250.0),
            rect(100.0, 50.0, 400.0, f32::NAN),
        ] {
            assert_eq!(selection_rect(region, &ends, heights), zero);
        }
        assert_ne!(
            selection_rect(rect(100.0, 50.0, 400.0, 250.0), &ends, heights),
            zero,
            "and a whole region does answer"
        );
    }

    #[test]
    fn a_selection_with_no_endpoints_has_no_rectangle() {
        assert_eq!(
            selection_rect(
                rect(100.0, 50.0, 400.0, 250.0),
                &[],
                GlyphHeights {
                    start: 14.0,
                    end: 14.0
                }
            ),
            rect(0.0, 0.0, 0.0, 0.0)
        );
    }

    fn types(
        collapsed: bool,
        platform: TargetPlatform,
        field: TextDirection,
        endpoints: Option<(Option<TextDirection>, Option<TextDirection>)>,
    ) -> HandleTypes {
        handle_types(collapsed, platform, field, endpoints)
    }

    #[test]
    fn the_start_handle_is_on_the_left_only_while_the_text_runs_that_way() {
        // "start" and "end" are about the text; "left" and "right" are about
        // the screen, and in right-to-left they swap.
        let ltr = types(
            false,
            TargetPlatform::Android,
            TextDirection::Ltr,
            Some((None, None)),
        );
        assert_eq!(ltr.start, TextSelectionHandleType::Left);
        assert_eq!(ltr.end, TextSelectionHandleType::Right);

        let rtl = types(
            false,
            TargetPlatform::Android,
            TextDirection::Rtl,
            Some((None, None)),
        );
        assert_eq!(rtl.start, TextSelectionHandleType::Right);
        assert_eq!(rtl.end, TextSelectionHandleType::Left);
    }

    #[test]
    fn the_two_ends_never_take_the_same_shape_in_a_range() {
        // The two switches are opposites. A port that wrote one and reused it
        // for the other would give a selection two identical handles.
        for field in [TextDirection::Ltr, TextDirection::Rtl] {
            for platform in [TargetPlatform::IOS, TargetPlatform::Android] {
                let both = types(false, platform, field, Some((None, None)));
                assert_ne!(both.start, both.end, "{platform:?} {field:?}");
            }
        }
    }

    #[test]
    fn an_insertion_point_gets_a_shape_of_its_own() {
        // It has no left and no right to be on, so upstream keeps a third
        // value rather than picking one of the two.
        for field in [TextDirection::Ltr, TextDirection::Rtl] {
            let caret = types(true, TargetPlatform::Android, field, Some((None, None)));
            assert_eq!(caret.start, TextSelectionHandleType::Collapsed);
            assert_eq!(caret.end, TextSelectionHandleType::Collapsed);
        }
    }

    #[test]
    fn ios_orients_both_handles_by_the_field_and_the_others_by_each_endpoint() {
        // "UIKit keeps selection handles aligned with the field direction."
        // A selection running through mixed-direction text: the field is ltr,
        // its start endpoint is rtl.
        let mixed = Some((Some(TextDirection::Rtl), Some(TextDirection::Ltr)));

        let ios = types(false, TargetPlatform::IOS, TextDirection::Ltr, mixed);
        assert_eq!(ios.start, TextSelectionHandleType::Left, "the field's ltr");

        // macOS is the near miss. It draws the same Cupertino handles as iOS,
        // so a port that reached for "the Apple platforms" would fold it in
        // here — but upstream asks `== TargetPlatform.iOS`, and the reason is
        // UIKit, which macOS is not running.
        for other in [
            TargetPlatform::Android,
            TargetPlatform::MacOS,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            let picked = types(false, other, TextDirection::Ltr, mixed);
            assert_eq!(
                picked.start,
                TextSelectionHandleType::Right,
                "{other:?} takes the endpoint's own rtl"
            );
            assert_ne!(ios.start, picked.start, "same text, two different shapes");
        }
    }

    #[test]
    fn an_endpoint_with_no_direction_of_its_own_borrows_the_fields() {
        // `endpoints.first.direction ?? textDirection`.
        let half = Some((None, Some(TextDirection::Rtl)));
        let picked = types(false, TargetPlatform::Android, TextDirection::Ltr, half);
        assert_eq!(picked.start, TextSelectionHandleType::Left, "borrowed ltr");
        assert_eq!(picked.end, TextSelectionHandleType::Left, "its own rtl");
    }

    #[test]
    fn fewer_than_two_endpoints_falls_back_to_the_field_as_ios_does() {
        // Render lag, a boundary inside an emoji, a squashed layout mid-fold:
        // the endpoint directions are not there, so the field's stands in.
        let none = types(false, TargetPlatform::Android, TextDirection::Rtl, None);
        let ios = types(
            false,
            TargetPlatform::IOS,
            TextDirection::Rtl,
            Some((Some(TextDirection::Ltr), Some(TextDirection::Ltr))),
        );
        assert_eq!(none, ios, "the same two shapes by two different routes");
        assert_eq!(none.start, TextSelectionHandleType::Right);
    }

    #[test]
    fn a_value_that_did_not_move_does_no_work() {
        // Every edit funnels through `update`, so the equality guard is what
        // lets it be called freely.
        let mut overlay = SelectionOverlay::new();
        let unchanged = overlay.update(false);
        assert!(!unchanged.refreshed);
        assert!(!unchanged.rebuilt);
    }

    #[test]
    fn a_changed_value_refreshes_and_then_asks_for_a_build_anyway() {
        // Text can change without the metrics or the selection changing --
        // swap a word for another of the same width -- so nothing
        // `_updateSelectionOverlay` writes comes out different, and a menu
        // offering "Look Up" on the old word would stay as it was.
        let mut overlay = SelectionOverlay::new();
        let changed = overlay.update(true);
        assert!(changed.refreshed);
        assert!(changed.rebuilt, "the toolbar needs the new text");
    }

    #[test]
    fn a_scroll_has_no_value_to_compare_and_so_no_guard() {
        // The same two lines as `update` and no early return: what moved is
        // the render object's metrics, and this is told only that something
        // did.
        let mut overlay = SelectionOverlay::new();
        let scrolled = overlay.update_for_scroll();
        assert!(scrolled.refreshed);
        assert!(scrolled.rebuilt);
        // Twice in a row still works, where `update` would have stopped.
        assert_eq!(overlay.update_for_scroll(), scrolled);
        assert_ne!(overlay.update(false), scrolled);
    }

    #[test]
    fn both_paths_ask_for_the_build_rather_than_leaving_it_to_the_properties() {
        // Writing a property that already holds that value rebuilds nothing,
        // and each caller has a case where none of them changed: the toolbar
        // after an equal-metrics edit, and the window after a metrics change.
        let mut overlay = SelectionOverlay::new();
        for outcome in [overlay.update(true), overlay.update_for_scroll()] {
            assert!(outcome.rebuilt, "asked for outright");
        }
    }

    #[test]
    fn a_toolbar_nobody_put_up_is_not_on_screen_however_visible_the_selection() {
        // Upstream's effective-visibility line is the two viewport readings
        // alone; existence is tracked separately, and what reaches the screen
        // is both. Here `toolbar_visible` carries both, so it has to be asked.
        let overlay = shown(true, false);
        assert!(!overlay.visibilities(true, (true, true)).toolbar);
        // The handles are unaffected by it in either direction.
        let visible = overlay.visibilities(true, (true, true));
        assert!(visible.start_handle && visible.end_handle);
    }

    #[test]
    fn each_handle_is_gated_by_its_own_end_of_the_selection() {
        // Scroll a selection until only its beginning is in the field and the
        // start handle stays while the end handle goes. They are not one
        // control that comes and goes together -- the one still on screen is
        // still draggable.
        let overlay = shown(true, true);
        let only_start = overlay.visibilities(true, (true, false));
        assert!(only_start.start_handle);
        assert!(!only_start.end_handle);

        let only_end = overlay.visibilities(true, (false, true));
        assert!(!only_end.start_handle);
        assert!(only_end.end_handle);
    }

    #[test]
    fn the_toolbar_takes_either_end_where_the_handles_take_their_own() {
        // `||` against the handles' `&&`: it acts on the selection as a whole,
        // and a selection with one end scrolled away is still worth copying.
        let overlay = shown(true, true);
        for ends in [(true, false), (false, true), (true, true)] {
            assert!(overlay.visibilities(true, ends).toolbar, "{ends:?}");
        }
        assert!(
            !overlay.visibilities(true, (false, false)).toolbar,
            "both ends gone and the menu goes with them"
        );
    }

    #[test]
    fn turning_the_handles_off_leaves_the_toolbar_alone() {
        // The toolbar's line does not mention `handlesVisible`. That is what
        // the property is for -- showing and hiding the handles without
        // touching anything else.
        let overlay = shown(false, true);
        let visible = overlay.visibilities(true, (true, true));
        assert!(!visible.start_handle && !visible.end_handle);
        assert!(visible.toolbar, "the menu stays");
    }

    #[test]
    fn handles_that_were_never_built_are_not_visible_however_much_they_are_wanted() {
        // `handlesAreVisible` is the conjunction `_handles != null &&
        // handlesVisible`, and this is the half the port used to be missing.
        let overlay = shown(true, true);
        let unbuilt = overlay.visibilities(false, (true, true));
        assert!(!unbuilt.start_handle && !unbuilt.end_handle);
        assert!(unbuilt.toolbar, "and the toolbar does not care either way");
    }

    #[test]
    fn building_and_showing_are_different_verbs_on_different_axes() {
        // `showHandles` builds and returns early if they are there;
        // `hideHandles` destroys and returns early if they are not. Neither
        // touches `handlesVisible`.
        assert!(SelectionOverlay::show_handles(false), "nothing there yet");
        assert!(
            !SelectionOverlay::show_handles(true),
            "a second call would insert a second pair"
        );
        assert!(
            SelectionOverlay::hide_handles(true),
            "there is a pair to destroy"
        );
        assert!(!SelectionOverlay::hide_handles(false));
    }

    #[test]
    fn a_magnifier_can_exist_without_being_shown() {
        // Upstream's own words: "the magnifier may exist in the overlay, but
        // not be shown". One boolean cannot say both.
        let mut magnifier = OverlayMagnifier::default();
        assert!(!magnifier.exists() && !magnifier.is_visible());
        magnifier.show(false, true);
        assert!(magnifier.exists() && magnifier.is_visible());

        // A magnifier that hid itself -- which upstream says they do.
        magnifier.shown = false;
        assert!(magnifier.exists(), "still in the overlay");
        assert!(!magnifier.is_visible(), "and not on screen");
    }

    #[test]
    fn showing_again_is_refused_by_existence_and_not_by_visibility() {
        // Keying the guard off `shown` would try to insert a second magnifier
        // on top of the one that is already there.
        let mut magnifier = OverlayMagnifier::default();
        magnifier.show(false, true);
        magnifier.shown = false;

        let again = magnifier.show(true, true);
        assert!(again.already_there);
        assert!(!again.inserts, "nothing added on top of the one there");
        assert!(
            !again.hides_toolbar,
            "and it returns before it would have touched the toolbar"
        );
    }

    #[test]
    fn hiding_is_refused_by_existence_too_or_the_entry_would_never_go() {
        // "This cannot be a check on `MagnifierController.shown`, since it's
        // possible that the magnifier is still in the overlay, but not shown."
        let mut magnifier = OverlayMagnifier::default();
        assert!(!magnifier.hide(), "nothing to take down");

        magnifier.show(false, true);
        magnifier.shown = false;
        assert!(magnifier.hide(), "the entry goes even though it was hidden");
        assert!(!magnifier.exists());
    }

    #[test]
    fn a_magnifier_and_a_toolbar_are_never_up_together() {
        let mut magnifier = OverlayMagnifier::default();
        let shown = magnifier.show(true, true);
        assert!(shown.hides_toolbar);
        assert!(shown.inserts);
    }

    #[test]
    fn the_toolbar_goes_before_it_is_known_a_magnifier_will_be_built() {
        // The builder is consulted two statements later, so a platform that
        // has none takes the toolbar down and puts nothing up.
        let mut magnifier = OverlayMagnifier::default();
        let shown = magnifier.show(true, false);
        assert!(shown.hides_toolbar, "the toolbar has already gone");
        assert!(!shown.inserts, "and nothing replaced it");
        assert!(!magnifier.exists(), "not even an entry");
    }

    #[test]
    fn hiding_the_overlay_takes_the_magnifier_with_it() {
        // `hide()` opens with `_magnifierController.hide()`.
        let mut overlay = SelectionOverlay::new().with_handles_visible(true);
        overlay.set_toolbar_visible(true);
        overlay.magnifier.show(false, true);
        overlay.hide();
        assert!(!overlay.handles_visible);
        assert!(!overlay.toolbar_visible);
        assert!(!overlay.magnifier.exists());
    }

    #[test]
    fn all_nineteen_callbacks_are_accounted_for() {
        // Every walk below is over this list, so a row lost from it would not
        // fail anything -- the walks would just stop looking. The count is
        // what upstream's `buildGestureDetector` passes, minus the four
        // arguments that are not callbacks.
        assert_eq!(GestureHandler::ALL.len(), 19);
        let mut seen = GestureHandler::ALL.to_vec();
        let before = seen.len();
        seen.sort_by_key(|handler| format!("{handler:?}"));
        seen.dedup();
        assert_eq!(seen.len(), before, "and none of them listed twice");
    }

    #[test]
    fn only_the_force_press_pair_is_gated_at_the_wiring() {
        // Twenty callbacks, and exactly two are conditional. Everything else
        // is connected whatever the delegate says and declines the work
        // inside.
        let does_not = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: false,
            selectable: false,
        });
        let withheld: Vec<GestureHandler> = GestureHandler::ALL
            .into_iter()
            .filter(|handler| !does_not.wires(*handler))
            .collect();
        assert_eq!(
            withheld,
            vec![
                GestureHandler::ForcePressStart,
                GestureHandler::ForcePressEnd
            ],
            "and not one of the selection handlers"
        );
    }

    #[test]
    fn a_field_that_wants_force_press_is_wired_for_everything() {
        let wants = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: true,
            selectable: false,
        });
        for handler in GestureHandler::ALL {
            assert!(wants.wires(handler), "{handler:?}");
        }
    }

    #[test]
    fn turning_selection_off_withholds_no_callback_at_all() {
        // A field with selection off still has to take a tap -- tapping it
        // moves focus and opens the keyboard -- so those recognizers stay in
        // the arena and decline the work one at a time.
        let no_selection = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: true,
            selectable: false,
        });
        let with_selection = TextSelectionGestureDetectorBuilder::new(Field {
            force_press: true,
            selectable: true,
        });
        for handler in GestureHandler::ALL {
            assert_eq!(
                no_selection.wires(handler),
                with_selection.wires(handler),
                "{handler:?}"
            );
        }
    }

    #[test]
    fn the_force_press_assert_is_about_the_wiring_and_can_never_fire() {
        // `assert(delegate.forcePressEnabled)` opens both force-press
        // handlers, and the only caller is a wiring that exists only when the
        // flag is true. Stated as the invariant it is.
        for force_press in [false, true] {
            let builder = TextSelectionGestureDetectorBuilder::new(Field {
                force_press,
                selectable: true,
            });
            for handler in [
                GestureHandler::ForcePressStart,
                GestureHandler::ForcePressEnd,
            ] {
                assert_eq!(
                    builder.wires(handler),
                    builder.handles_force_press(),
                    "reached only when the field wants it"
                );
            }
        }
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
    fn a_handle_grabbed_near_its_edge_does_not_jump_to_its_middle() {
        // Upstream's `_handleSelectionStartHandleDragStart` records where in
        // the handle the finger landed and keeps it for the whole drag. The
        // rule was ported here and had nothing calling it; the field used a
        // constant half a line instead, so a handle taken by its top edge
        // moved the selection as though it had been taken by its middle.
        let mut overlay = TextSelectionOverlay::new();
        assert_eq!(overlay.grab_offset(), None, "nothing grabbed yet");

        overlay.begin_handle_drag(Offset::new(3.0, 2.0));
        assert_eq!(overlay.grab_offset(), Some(Offset::new(3.0, 2.0)));

        // The same finger position means two different selection points
        // depending on where the handle was grabbed -- which is the whole
        // difference between this and a constant.
        let finger = Offset::new(100.0, 50.0);
        assert_eq!(
            overlay.handle_drag_position(finger),
            Offset::new(97.0, 48.0)
        );
        overlay.begin_handle_drag(Offset::new(3.0, 18.0));
        assert_eq!(
            overlay.handle_drag_position(finger),
            Offset::new(97.0, 32.0),
            "grabbed lower down, the selection point is higher up"
        );

        overlay.end_handle_drag();
        assert_eq!(overlay.grab_offset(), None, "and the grab is let go");
        assert_eq!(
            overlay.handle_drag_position(finger),
            finger,
            "with nothing grabbed the finger is taken as it is"
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

    // -- Where the shift flag comes from -------------------------------------

    use super::TapTrackShift;

    #[test]
    fn shift_is_sampled_once_when_the_tap_sequence_starts() {
        // `onTapTrackStart` asks the keyboard and `onTapTrackReset` clears it;
        // nothing asks again in between. So the whole of a double- or
        // triple-tap runs on the answer the first press gave.
        let mut shift = TapTrackShift::new();
        assert!(!shift.is_held(), "nothing held before a sequence starts");

        shift.track_started(true);
        assert!(shift.is_held());
        // A reader who lets go part way through still gets the sequence they
        // began: changing its mind between the second and third tap would
        // select something nobody asked for.
        assert!(shift.is_held(), "and it is not re-read for the second tap");

        shift.track_reset();
        assert!(!shift.is_held());
    }

    #[test]
    fn shift_pressed_after_the_sequence_began_does_not_join_it() {
        // The other way round, and the reason this is a rule rather than an
        // accident: a reader who presses shift *after* starting a multi-tap
        // does not get a shift-extend from it.
        let mut shift = TapTrackShift::new();
        shift.track_started(false);
        assert!(!shift.is_held());
        // The keyboard has changed under it; nothing asks.
        assert!(!shift.is_held(), "still the answer the first press gave");

        // Only the next sequence picks it up.
        shift.track_reset();
        shift.track_started(true);
        assert!(shift.is_held());
    }

    #[test]
    fn what_the_shift_rules_are_told_is_what_was_sampled() {
        // The join: every shift rule in this file takes a `shift_pressed` it
        // does not decide, and this is where that value comes from. A tap-down
        // during a sequence that began with shift held expands or extends;
        // the same tap in a sequence that did not, does nothing.
        use crate::editable_text::TargetPlatform;
        let mut shift = TapTrackShift::new();
        shift.track_started(true);
        assert_eq!(
            shift_tap_down(TargetPlatform::Linux, shift.is_held(), true, true),
            ShiftTapDown::Extend
        );

        shift.track_reset();
        assert_eq!(
            shift_tap_down(TargetPlatform::Linux, shift.is_held(), true, true),
            ShiftTapDown::Nothing,
            "the same tap, in a sequence that began without shift"
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

    /// `drag_selection_update` with the arguments in the order the tests care
    /// about, so a row reads as the question it is asking.
    struct TextSelectionGestures2;
    impl TextSelectionGestures2 {
        fn drag(
            platform: TargetPlatform,
            kind: PointerKind,
            taps: u32,
            has_focus: bool,
        ) -> DragSelectionUpdate {
            drag_selection_update(platform, true, kind, taps, has_focus)
        }
    }

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
    fn a_primary_tap_gives_the_handles_and_the_toolbar_the_same_answer() {
        // Upstream assigns one from the other on the next line -- but only in
        // `onTapDown` and `onDragSelectionStart`. This test was called
        // `the_handles_cannot_disagree_with_the_toolbar`, which is a claim
        // about the class and is false; see the secondary-tap tests below.
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
    fn the_menu_after_a_drag_belongs_to_the_double_tap_alone() {
        // `== 2`, not `>= 2`. Reading it as "two or more" would put a menu up
        // after a triple-tap drag, which upstream does not do.
        assert!(drag_selection_end(true, 2, false).shows_toolbar);
        for taps in [1, 3, 4] {
            assert!(
                !drag_selection_end(true, taps, false).shows_toolbar,
                "{taps} taps"
            );
        }
    }

    #[test]
    fn a_pointer_that_never_earned_a_toolbar_gets_none_from_a_drag_either() {
        // The flag is asked first, so a mouse that dragged out words gets no
        // toolbar -- it has a right-click menu instead.
        assert!(!drag_selection_end(false, 2, false).shows_toolbar);
    }

    #[test]
    fn the_drag_start_selection_is_released_only_on_the_path_that_read_it() {
        // Taken by every drag start, read by exactly one thing -- the shift
        // branch -- and so released on exactly that path.
        assert!(drag_selection_end(true, 2, true).clears_the_drag_start_selection);
        assert!(!drag_selection_end(true, 2, false).clears_the_drag_start_selection);
    }

    #[test]
    fn a_drag_hides_the_magnifier_whatever_else_it_did() {
        // Outside both conditions, at the end.
        for taps in 1..=3 {
            for shift in [false, true] {
                assert!(drag_selection_end(false, taps, shift).hides_magnifier);
            }
        }
    }

    #[test]
    fn the_two_gestures_end_differently_and_the_anchors_are_where() {
        // The long press zeroes both scroll anchors; the drag does not. Both
        // starts take them afresh, so this is upstream's housekeeping rather
        // than a difference in behaviour -- named here rather than smoothed
        // into agreement.
        let press = long_press_finish(
            LongPressFinish::Ended,
            TargetPlatform::IOS,
            true,
            true,
            true,
            true,
        );
        let drag = drag_selection_end(true, 2, false);
        assert!(press.resets_the_drag_anchors);
        assert!(!drag.resets_the_drag_anchors);
        // And both hide the magnifier, which is the part they agree on.
        assert!(press.hides_magnifier && drag.hides_magnifier);
    }

    #[test]
    fn a_cancelled_press_tidies_up_exactly_as_a_finished_one_does() {
        // The reason the tail is factored out. A press taken away by the
        // arena must not leave the magnifier on screen or the anchors
        // half-set.
        let ended = long_press_finish(
            LongPressFinish::Ended,
            TargetPlatform::IOS,
            true,
            true,
            true,
            true,
        );
        let cancelled = long_press_finish(
            LongPressFinish::Cancelled,
            TargetPlatform::IOS,
            true,
            true,
            true,
            true,
        );
        assert_eq!(ended.hides_magnifier, cancelled.hides_magnifier);
        assert_eq!(ended.clears_the_flag, cancelled.clears_the_flag);
        assert_eq!(
            ended.resets_the_drag_anchors,
            cancelled.resets_the_drag_anchors
        );
        assert_eq!(ended.ends_floating_cursor, cancelled.ends_floating_cursor);
    }

    #[test]
    fn and_the_toolbar_is_the_whole_difference() {
        let both = |finish| long_press_finish(finish, TargetPlatform::IOS, true, true, true, true);
        assert!(both(LongPressFinish::Ended).shows_toolbar);
        assert!(
            !both(LongPressFinish::Cancelled).shows_toolbar,
            "no menu for a gesture the reader never completed"
        );
    }

    #[test]
    fn a_finished_press_still_asks_whether_the_pointer_earned_a_toolbar() {
        // `if (shouldShowSelectionToolbar)` -- the flag the tap that began
        // this press set from its pointer kind.
        assert!(
            !long_press_finish(
                LongPressFinish::Ended,
                TargetPlatform::IOS,
                true,
                true,
                true,
                false,
            )
            .shows_toolbar
        );
    }

    #[test]
    fn the_drag_anchors_are_zeroed_whichever_way_it_stopped() {
        // They are what `drag_anchor_correction` subtracts against. Left
        // behind, the next press would correct its anchor against a scroll
        // position belonging to the last one.
        for finish in [LongPressFinish::Ended, LongPressFinish::Cancelled] {
            for platform in [TargetPlatform::IOS, TargetPlatform::Android] {
                let end = long_press_finish(finish, platform, false, false, false, false);
                assert!(end.resets_the_drag_anchors, "{finish:?} {platform:?}");
                assert!(end.hides_magnifier, "{finish:?} {platform:?}");
            }
        }
    }

    #[test]
    fn a_field_that_is_already_gone_gets_no_floating_cursor_message() {
        // `_isEditableTextMounted`. A cancel arriving after the field was
        // disposed is a normal way for one to arrive, and the one step that
        // talks to the field is the one that has to ask.
        assert!(
            !long_press_finish(
                LongPressFinish::Cancelled,
                TargetPlatform::IOS,
                true,
                true,
                false,
                true,
            )
            .ends_floating_cursor
        );
        // ... while the cleanup this object owns happens anyway.
        let gone = long_press_finish(
            LongPressFinish::Cancelled,
            TargetPlatform::IOS,
            true,
            true,
            false,
            true,
        );
        assert!(gone.hides_magnifier && gone.resets_the_drag_anchors);
    }

    #[test]
    fn only_apple_pivots_and_only_from_a_range() {
        // Two guards, either one alone sending the drag down the ordinary
        // path. `(4, 9)` is a forward range; dragging to 1 crosses its base.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                shift_drag_update(platform, (4, 9), 1, false),
                ShiftDragUpdate::PivotToTheFarEnd,
                "{platform:?}"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_eq!(
                shift_drag_update(platform, (4, 9), 1, false),
                ShiftDragUpdate::Extend,
                "{platform:?}"
            );
        }
        // A caret rather than a range: nothing to pivot around. The side
        // matters -- dragging *left* of a caret answers `Extend` with or
        // without the guard, because a collapsed selection reads as
        // backwards and going left is not inverting it. Dragging right is
        // what the guard is actually for.
        for next_extent in [1, 7] {
            assert_eq!(
                shift_drag_update(TargetPlatform::MacOS, (4, 4), next_extent, false),
                ShiftDragUpdate::Extend,
                "caret, dragged to {next_extent}"
            );
        }
    }

    #[test]
    fn a_backwards_selection_inverts_by_going_the_other_way() {
        // Measured against the base the drag started with, whichever way that
        // selection ran. A port testing `next < base` unconditionally would
        // have this backwards for every selection made right-to-left.
        let forward = (4, 9);
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, forward, 1, false),
            ShiftDragUpdate::PivotToTheFarEnd,
            "left past the base of a forward selection"
        );
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, forward, 12, false),
            ShiftDragUpdate::Extend,
            "and right of it is just extending"
        );

        let backward = (9, 4);
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, backward, 12, false),
            ShiftDragUpdate::PivotToTheFarEnd,
            "right past the base of a backward selection"
        );
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, backward, 1, false),
            ShiftDragUpdate::Extend
        );
    }

    #[test]
    fn the_pivot_keeps_the_original_range_whole_and_swings_it() {
        // Ordinary extending would drop everything past the crossing and grow
        // a fresh range from there. The pivot anchors on the original
        // selection's *other* end instead, so 4..9 dragged back to 1 becomes
        // 9..1 -- the whole of it still selected, plus the new part.
        let outcome = shift_drag_update(TargetPlatform::MacOS, (4, 9), 1, false);
        assert_eq!(
            shift_drag_selection(outcome, (4, 9), 1),
            Some((9, 1)),
            "anchored on the far end"
        );
    }

    #[test]
    fn crossing_back_pivots_back_to_the_original_base() {
        let outcome = shift_drag_update(TargetPlatform::MacOS, (4, 9), 12, true);
        assert_eq!(outcome, ShiftDragUpdate::PivotBack);
        assert_eq!(shift_drag_selection(outcome, (4, 9), 12), Some((4, 12)));
    }

    #[test]
    fn the_pivot_fires_on_the_crossing_and_not_on_every_move_after_it() {
        // `selection.baseOffset == _dragStartSelection.baseOffset` is what
        // makes this idempotent: once pivoted the base is no longer the
        // original one, so the arm stops matching and further movement is
        // ordinary extending.
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, (4, 9), 1, false),
            ShiftDragUpdate::PivotToTheFarEnd
        );
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, (4, 9), 0, true),
            ShiftDragUpdate::Extend,
            "still inverted, but already pivoted"
        );
    }

    #[test]
    fn landing_exactly_on_the_original_base_is_not_a_pivot_back() {
        // `nextExtent.offset != _dragStartSelection!.baseOffset` -- pivoting
        // back to a selection of zero length would collapse it, and upstream
        // falls through to extending instead.
        assert_eq!(
            shift_drag_update(TargetPlatform::MacOS, (4, 9), 4, true),
            ShiftDragUpdate::Extend
        );
    }

    #[test]
    fn an_extend_asks_for_no_selection_of_its_own() {
        // `_extendSelection` moves the loose end and leaves the anchor alone;
        // the pivoting arms are the only two that name a whole selection.
        assert_eq!(
            shift_drag_selection(ShiftDragUpdate::Extend, (4, 9), 1),
            None
        );
    }

    #[test]
    fn how_many_times_you_tapped_first_decides_what_a_drag_selects() {
        // One marks out a range, two grows it a word at a time, three a
        // paragraph. The ladder is what the count is for.
        let at = |taps| {
            TextSelectionGestures2::drag(TargetPlatform::MacOS, PointerKind::Mouse, taps, true)
        };
        assert_eq!(at(1).selects, DragSelects::RangeFromTheAnchor);
        assert_eq!(at(2).selects, DragSelects::WordsFromTheAnchor);
        assert_eq!(at(3).selects, DragSelects::ParagraphsFromTheAnchor);
    }

    #[test]
    fn linux_drags_by_the_line_where_the_other_desktops_drag_by_the_paragraph() {
        // One arm of one switch, and the difference between selecting a
        // wrapped paragraph and selecting the visual row under the pointer.
        assert_eq!(
            TextSelectionGestures2::drag(TargetPlatform::Linux, PointerKind::Mouse, 3, true)
                .selects,
            DragSelects::LinesFromTheAnchor
        );
        for platform in [TargetPlatform::Windows, TargetPlatform::MacOS] {
            assert_eq!(
                TextSelectionGestures2::drag(platform, PointerKind::Mouse, 3, true).selects,
                DragSelects::ParagraphsFromTheAnchor,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_triple_tap_drag_needs_a_precise_pointer_on_the_touch_platforms() {
        // "Triple tap to drag is not present on these platforms when using
        // non-precise pointer devices at the moment."
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
        ] {
            assert_eq!(
                TextSelectionGestures2::drag(platform, PointerKind::Mouse, 3, true).selects,
                DragSelects::ParagraphsFromTheAnchor,
                "{platform:?}"
            );
            assert_eq!(
                TextSelectionGestures2::drag(platform, PointerKind::Touch, 3, true).selects,
                DragSelects::Nothing,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_finger_on_android_drags_the_caret_and_selects_nothing() {
        // `selectPositionAt(from: globalPosition)` with no `to` -- there is no
        // anchor, so nothing is selected. Reading it as a range of length zero
        // loses the distinction: one can grow into a selection and the other
        // cannot.
        let touch =
            TextSelectionGestures2::drag(TargetPlatform::Android, PointerKind::Touch, 1, true);
        assert_eq!(touch.selects, DragSelects::CaretAtTheFingerOnly);
        assert!(touch.shows_magnifier, "with the magnifier over it");

        // The same drag with a mouse marks out a range instead.
        assert_eq!(
            TextSelectionGestures2::drag(TargetPlatform::Android, PointerKind::Mouse, 1, true)
                .selects,
            DragSelects::RangeFromTheAnchor
        );
    }

    #[test]
    fn ios_gives_a_finger_drag_nothing_where_it_gives_a_mouse_a_range() {
        // "With a mouse device, a drag should select the range from the origin
        // of the drag to the current position of the drag. With a touch
        // device, nothing should happen." -- and unlike Android there is no
        // caret-follows-finger consolation.
        assert_eq!(
            TextSelectionGestures2::drag(TargetPlatform::IOS, PointerKind::Mouse, 1, true).selects,
            DragSelects::RangeFromTheAnchor
        );
        for kind in [
            PointerKind::Touch,
            PointerKind::Stylus,
            PointerKind::Unknown,
        ] {
            let update = TextSelectionGestures2::drag(TargetPlatform::IOS, kind, 1, true);
            assert_eq!(update.selects, DragSelects::Nothing, "{kind:?}");
            assert!(!update.shows_magnifier, "{kind:?}");
        }
    }

    #[test]
    fn an_unfocused_android_field_does_nothing_at_all_under_a_finger() {
        // The drag that would focus it is this same gesture, and it has not
        // finished.
        assert_eq!(
            TextSelectionGestures2::drag(TargetPlatform::Android, PointerKind::Touch, 1, false)
                .selects,
            DragSelects::Nothing
        );
    }

    #[test]
    fn a_stylus_counts_as_precise_for_a_plain_drag_and_not_for_a_double_tap_drag() {
        // At one tap Android lists the stylus beside the mouse; at two the
        // magnifier arm lists it beside the finger. The same device, sorted
        // two ways by two different questions.
        assert_eq!(
            TextSelectionGestures2::drag(TargetPlatform::Android, PointerKind::Stylus, 1, true)
                .selects,
            DragSelects::RangeFromTheAnchor
        );
        assert!(
            TextSelectionGestures2::drag(TargetPlatform::Android, PointerKind::Stylus, 2, true)
                .shows_magnifier
        );
    }

    #[test]
    fn the_magnifier_is_for_a_fingertip_and_not_for_a_pointer() {
        // It exists to show what a fingertip is covering; a pointer covers
        // nothing.
        for kind in [
            PointerKind::Touch,
            PointerKind::Stylus,
            PointerKind::Unknown,
        ] {
            assert!(
                TextSelectionGestures2::drag(TargetPlatform::MacOS, kind, 2, true).shows_magnifier,
                "{kind:?}"
            );
        }
        for kind in [PointerKind::Mouse, PointerKind::Trackpad] {
            assert!(
                !TextSelectionGestures2::drag(TargetPlatform::MacOS, kind, 2, true).shows_magnifier,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_field_that_does_not_select_ignores_the_drag_whatever_the_count() {
        for taps in 1..=3 {
            let update =
                drag_selection_update(TargetPlatform::MacOS, false, PointerKind::Mouse, taps, true);
            assert_eq!(update.selects, DragSelects::Nothing, "{taps} taps");
            assert!(!update.shows_magnifier);
        }
    }

    #[test]
    fn a_right_click_earns_a_toolbar_it_has_not_earned_handles_for() {
        // The one place the two flags genuinely part company. A mouse asking
        // for the context menu gets it, and gets no draggable handles: it has
        // a pointer precise enough to select with, and handles would be
        // furniture in the way.
        let mouse = TextSelectionGestures::flags_for(
            SelectionGesture::SecondaryTapDown,
            PointerKind::Mouse,
        );
        assert_eq!(mouse.toolbar, Some(true));
        assert_eq!(mouse.handles, Some(false), "and no handles for a mouse");

        // A finger long-pressing to the same menu still needs them.
        let finger = TextSelectionGestures::flags_for(
            SelectionGesture::SecondaryTapDown,
            PointerKind::Touch,
        );
        assert_eq!(finger.toolbar, Some(true));
        assert_eq!(finger.handles, Some(true));
    }

    #[test]
    fn a_mouse_gets_no_toolbar_from_a_primary_tap_and_one_from_a_secondary() {
        // The same pointer, two gestures, opposite answers -- which is what
        // makes `flags_for` take the gesture and not just the kind.
        assert_eq!(
            TextSelectionGestures::flags_for(SelectionGesture::TapDown, PointerKind::Mouse).toolbar,
            Some(false)
        );
        assert_eq!(
            TextSelectionGestures::flags_for(
                SelectionGesture::SecondaryTapDown,
                PointerKind::Mouse
            )
            .toolbar,
            Some(true)
        );
    }

    #[test]
    fn a_force_press_leaves_the_handles_where_the_tap_before_it_left_them() {
        // `onForcePressStart` writes the toolbar flag and does not touch the
        // handles flag at all. `None` is that absence, and it is a different
        // thing from `Some(false)`: the previous gesture's answer stands.
        for kind in [PointerKind::Touch, PointerKind::Stylus, PointerKind::Mouse] {
            let flags = TextSelectionGestures::flags_for(SelectionGesture::ForcePressStart, kind);
            assert_eq!(flags.toolbar, Some(true), "{kind:?}");
            assert_eq!(flags.handles, None, "{kind:?}");
        }
    }

    #[test]
    fn a_tap_and_a_drag_start_write_both_flags_together() {
        // The three-of-four case that makes the two look like one field.
        for gesture in [
            SelectionGesture::TapDown,
            SelectionGesture::DragSelectionStart,
        ] {
            for kind in [PointerKind::Touch, PointerKind::Mouse, PointerKind::Stylus] {
                let flags = TextSelectionGestures::flags_for(gesture, kind);
                assert_eq!(flags.toolbar, flags.handles, "{gesture:?} {kind:?}");
                assert_eq!(
                    flags.toolbar,
                    Some(TextSelectionGestures::shows_selection_toolbar(kind))
                );
            }
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

    // -- What a tap on a focused field means --------------------------------

    /// A tap with everything else in the ordinary state: focused, editable,
    /// the affinity unchanged, the word spelled correctly.
    fn tap(selection: (i32, i32), offset: i32) -> TapOutcome {
        tap_outcome(false, selection, offset, true, false, true)
    }

    #[test]
    fn a_tap_inside_a_highlighted_run_toggles_the_toolbar_and_a_tap_on_its_end_does_not() {
        // Exclusive, for a reason: the ends are where the handles are, so a
        // tap there is aimed at the handle or at putting the caret outside the
        // selection. Inside is the only place that means "this selection".
        let run = (4, 9);
        assert_eq!(tap(run, 6), TapOutcome::ToggleToolbar, "inside");
        assert_eq!(tap(run, 4), TapOutcome::SelectWordEdge, "on the start");
        assert_eq!(tap(run, 9), TapOutcome::SelectWordEdge, "on the end");
        assert_eq!(tap(run, 12), TapOutcome::SelectWordEdge, "past it");
    }

    #[test]
    fn but_a_caret_is_tested_inclusively_or_the_rule_could_never_fire() {
        // A collapsed selection has no inside. An exclusive test on it is
        // false for every offset there is, so the second-tap-on-your-own-caret
        // case would never happen -- which is the case the rule exists for.
        let caret = (7, 7);
        assert!(!position_was_on_selection_exclusive(caret, 7));
        assert!(position_was_on_selection_inclusive(caret, 7));
        assert_eq!(tap(caret, 7), TapOutcome::ToggleToolbar);
        assert_eq!(tap(caret, 8), TapOutcome::SelectWordEdge, "not the caret");
    }

    #[test]
    fn a_tap_at_a_line_wrap_that_means_the_other_line_moves_rather_than_toggles() {
        // One offset is two places where the text wraps. If the affinity is
        // not the one the selection already had, the reader is asking for the
        // following line, and the toolbar appearing where they did not tap
        // would be the wrong answer.
        let caret = (7, 7);
        assert_eq!(
            tap_outcome(false, caret, 7, true, false, true),
            TapOutcome::ToggleToolbar,
            "same affinity: the same place"
        );
        assert_eq!(
            tap_outcome(false, caret, 7, false, false, true),
            TapOutcome::SelectWordEdge,
            "a different affinity is a different place"
        );

        // The affinity only qualifies the collapsed arm. A tap inside a run
        // toggles regardless, because a run has an inside and there is nothing
        // ambiguous about being in it.
        assert_eq!(
            tap_outcome(false, (4, 9), 6, false, false, true),
            TapOutcome::ToggleToolbar
        );
    }

    #[test]
    fn a_read_only_field_takes_a_tap_on_its_caret_as_a_request_to_select() {
        // The caret in a read-only field is not something the reader put
        // there, so tapping it is not a second tap on their own work.
        let caret = (7, 7);
        assert_eq!(
            tap_outcome(false, caret, 7, true, true, true),
            TapOutcome::SelectWordEdge
        );
        // And again only the collapsed arm: a highlighted run in a read-only
        // field still toggles, because the reader made that selection.
        assert_eq!(
            tap_outcome(false, (4, 9), 6, true, true, true),
            TapOutcome::ToggleToolbar
        );
    }

    #[test]
    fn an_unfocused_field_places_the_caret_whatever_the_tap_landed_on() {
        // The `hasFocus` term gates the whole condition rather than one arm of
        // it: a first tap is about taking focus.
        for selection in [(4, 9), (7, 7)] {
            let offset = if selection.0 == selection.1 { 7 } else { 6 };
            assert_eq!(
                tap_outcome(false, selection, offset, true, false, false),
                TapOutcome::SelectWordEdge,
                "{selection:?}"
            );
            assert_eq!(
                tap_outcome(false, selection, offset, true, false, true),
                TapOutcome::ToggleToolbar,
                "{selection:?} focused"
            );
        }
    }

    #[test]
    fn a_misspelled_word_is_checked_before_any_of_it() {
        // The suggestions are what the tap was for, and it does not matter
        // where the selection was or whether the field has focus.
        for has_focus in [false, true] {
            for selection in [(4, 9), (7, 7)] {
                assert_eq!(
                    tap_outcome(true, selection, 6, true, false, has_focus),
                    TapOutcome::SelectWordAndOfferSpelling,
                    "{selection:?} {has_focus}"
                );
            }
        }
    }

    #[test]
    fn the_toolbar_comes_back_only_when_the_word_edge_moved_nothing() {
        // A tap that moved the caret hides the toolbar: a toolbar belongs to
        // the selection it was raised for. A tap that changed nothing was a
        // second tap in the same place, which is a request for the toolbar.
        assert_eq!(
            after_selecting_the_word_edge(false, false, true),
            AfterWordEdge::ToggleToolbar
        );
        assert_eq!(
            after_selecting_the_word_edge(true, false, true),
            AfterWordEdge::HideToolbar,
            "it moved"
        );
        assert_eq!(
            after_selecting_the_word_edge(false, true, true),
            AfterWordEdge::HideToolbar,
            "read-only"
        );
        assert_eq!(
            after_selecting_the_word_edge(false, false, false),
            AfterWordEdge::HideToolbar,
            "unfocused"
        );
    }

    #[test]
    fn a_selection_given_backwards_is_the_same_selection() {
        // A field's selection is a base and an extent, and dragging right to
        // left puts the larger number first. Every predicate here has to read
        // it the same way round.
        assert!(position_was_on_selection_exclusive((9, 4), 6));
        assert!(position_was_on_selection_inclusive((9, 4), 4));
        assert_eq!(tap((9, 4), 6), TapOutcome::ToggleToolbar);
    }

    // -- Expand and extend, which differ in one case ------------------------

    #[test]
    fn beyond_the_loose_end_the_two_agree() {
        // Shift-clicking further along in the direction you were already
        // going is the ordinary case, and it is the one where the names do
        // not matter.
        let run = (4, 9);
        assert_eq!(extend_selection(run, 12), (4, 12));
        assert_eq!(expand_selection(run, 12), (4, 12));
    }

    #[test]
    fn but_past_the_anchor_extend_throws_the_selection_away_and_expand_keeps_it() {
        // The case the two names are about. With 4..9 selected and a
        // shift-click at 1: extend drags the loose end to 1 and the run from 4
        // to 9 is gone, so the reader has lost what they were adding to.
        // Expand notices 1 is nearer the base, anchors on the extent instead,
        // and the original run is still inside the answer.
        let run = (4, 9);
        assert_eq!(extend_selection(run, 1), (4, 1), "4..9 became 1..4");
        assert_eq!(expand_selection(run, 1), (9, 1), "and here 1..9");

        let (base, extent) = expand_selection(run, 1);
        let (low, high) = (base.min(extent), base.max(extent));
        assert!(low <= 4 && high >= 9, "the whole of the old run survives");
    }

    #[test]
    fn expand_anchors_on_whichever_end_is_further_away() {
        // Stated directly, because the `baseIsCloser` spelling hides it: the
        // end that stays is the far one, whichever it happens to be.
        assert_eq!(expand_selection((4, 9), 5), (9, 5), "5 is nearer 4");
        assert_eq!(expand_selection((4, 9), 8), (4, 8), "8 is nearer 9");
        // A backwards selection is the same rule, not a special case.
        assert_eq!(expand_selection((9, 4), 5), (9, 5));
        assert_eq!(expand_selection((9, 4), 8), (4, 8));
    }

    #[test]
    fn extend_never_looks_at_where_the_tap_is() {
        // Which is the other half of the contrast: it keeps the base whatever
        // the tap says, so it is the same one line for every position.
        for tapped in [-3, 0, 4, 6, 9, 20] {
            assert_eq!(extend_selection((4, 9), tapped), (4, tapped));
        }
    }

    // -- Which of the two a shift-click gets --------------------------------

    #[test]
    fn macos_expands_and_the_other_desktops_extend() {
        assert_eq!(
            shift_tap_down(TargetPlatform::MacOS, true, true, true),
            ShiftTapDown::Expand
        );
        for platform in [TargetPlatform::Linux, TargetPlatform::Windows] {
            assert_eq!(
                shift_tap_down(platform, true, true, true),
                ShiftTapDown::Extend,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn and_a_cold_macos_field_expands_from_the_start_of_the_text() {
        // Upstream's comment: "On macOS, a shift-tapped unfocused field
        // expands from 0, not from the previous selection." A shift-click into
        // a field nobody was in selects from the beginning to the click, which
        // is what every text view on that platform does.
        assert_eq!(
            shift_tap_down(TargetPlatform::MacOS, true, true, false),
            ShiftTapDown::ExpandFromTheStart
        );
        // Only macOS: the others do not have a second answer here.
        for platform in [TargetPlatform::Linux, TargetPlatform::Windows] {
            assert_eq!(
                shift_tap_down(platform, true, true, false),
                ShiftTapDown::Extend,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_mobile_platforms_decide_on_the_way_up_instead() {
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
        ] {
            for has_focus in [false, true] {
                assert_eq!(
                    shift_tap_down(platform, true, true, has_focus),
                    ShiftTapDown::Nothing,
                    "{platform:?}"
                );
            }
        }
    }

    #[test]
    fn shift_with_nothing_to_extend_from_does_nothing() {
        // Upstream's own words: "It is impossible to extend the selection when
        // the shift key is pressed, if the renderEditable.selection is
        // invalid."
        for platform in TargetPlatform::ALL {
            assert_eq!(
                shift_tap_down(platform, true, false, true),
                ShiftTapDown::Nothing,
                "{platform:?}: no selection"
            );
            assert_eq!(
                shift_tap_down(platform, false, true, true),
                ShiftTapDown::Nothing,
                "{platform:?}: no shift"
            );
        }
    }

    // -- Starting a drag-selection -----------------------------------------

    /// A plain drag: no shift, one tap, focused.
    fn drag(platform: TargetPlatform, kind: PointerKind) -> DragSelectionStart {
        drag_selection_start(platform, true, kind, false, true, 1, true)
    }

    #[test]
    fn a_desktop_places_the_caret_whatever_the_pointer_is() {
        // There is nothing else a drag on a desktop could mean.
        for platform in [
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            for kind in [PointerKind::Mouse, PointerKind::Touch, PointerKind::Stylus] {
                assert_eq!(
                    drag(platform, kind).selects,
                    DragStartSelects::CaretAtTheFinger,
                    "{platform:?} {kind:?}"
                );
            }
        }
    }

    #[test]
    fn ios_starts_nothing_under_a_finger_even_in_a_focused_field() {
        // Android and Fuchsia do start there; iOS's touch case is empty, so
        // there is no focus test on that platform because there is no path.
        // Upstream's comment names three platforms over a branch serving two.
        for kind in [PointerKind::Touch, PointerKind::Unknown] {
            assert_eq!(
                drag(TargetPlatform::IOS, kind).selects,
                DragStartSelects::Nothing,
                "{kind:?}"
            );
            assert_eq!(
                drag_selection_start(TargetPlatform::IOS, true, kind, false, true, 1, false)
                    .selects,
                DragStartSelects::Nothing,
                "{kind:?}: and unfocused is the same nothing"
            );
        }
        // A mouse on the same platform does place the caret -- and so does a
        // trackpad. Upstream's precise pair is `mouse || trackpad` in every
        // one of these branches, so a port that read only `mouse` would leave
        // a trackpad drag doing nothing on the platforms where it matters.
        for kind in [PointerKind::Mouse, PointerKind::Trackpad] {
            assert_eq!(
                drag(TargetPlatform::IOS, kind).selects,
                DragStartSelects::CaretAtTheFinger,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn android_starts_under_a_finger_only_in_a_field_that_already_has_focus() {
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            // A trackpad is precise and goes the other way, focus or not.
            for kind in [PointerKind::Mouse, PointerKind::Trackpad] {
                assert_eq!(
                    drag_selection_start(platform, true, kind, false, true, 1, false).selects,
                    DragStartSelects::CaretAtTheFinger,
                    "{platform:?} {kind:?}"
                );
            }

            let focused = drag(platform, PointerKind::Touch);
            assert_eq!(
                focused.selects,
                DragStartSelects::CaretAtTheFinger,
                "{platform:?}"
            );
            assert!(
                focused.shows_magnifier,
                "{platform:?}: and this is the only path in the method that raises it"
            );

            let cold =
                drag_selection_start(platform, true, PointerKind::Touch, false, true, 1, false);
            assert_eq!(cold.selects, DragStartSelects::Nothing, "{platform:?}");
            assert!(!cold.shows_magnifier, "{platform:?}");
        }
    }

    #[test]
    fn the_magnifier_belongs_to_that_path_and_no_other() {
        // Not a mouse, not a desktop, not a shift-drag.
        for platform in TargetPlatform::ALL {
            assert!(
                !drag(platform, PointerKind::Mouse).shows_magnifier,
                "{platform:?} mouse"
            );
        }
        for platform in [
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            assert!(
                !drag(platform, PointerKind::Touch).shows_magnifier,
                "{platform:?} touch"
            );
        }
        assert!(
            !drag_selection_start(
                TargetPlatform::Android,
                true,
                PointerKind::Touch,
                true,
                true,
                1,
                true
            )
            .shows_magnifier,
            "a shift-drag takes the other branch entirely"
        );
    }

    #[test]
    fn a_stylus_starts_nothing_anywhere_but_still_sets_the_flags() {
        // The flags are assigned before every branch, so the gesture that
        // does nothing still says what kind of pointer it was.
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
        ] {
            let start = drag(platform, PointerKind::Stylus);
            assert_eq!(start.selects, DragStartSelects::Nothing, "{platform:?}");
            assert!(start.sets_the_overlay_flags, "{platform:?}");
        }
    }

    #[test]
    fn a_double_tap_that_becomes_a_drag_keeps_the_word_it_selected() {
        // The second tap already selected a word and the drag grows the
        // selection word by word. Placing a caret here would throw that away
        // at the moment the reader started to drag.
        for platform in TargetPlatform::ALL {
            let start =
                drag_selection_start(platform, true, PointerKind::Mouse, false, true, 2, true);
            assert_eq!(start.selects, DragStartSelects::Nothing, "{platform:?}");
            assert!(
                start.sets_the_overlay_flags,
                "{platform:?}: but the return is after the flags"
            );
        }
    }

    #[test]
    fn a_shift_drag_splits_the_way_every_other_shift_gesture_does() {
        // Apple expands, everyone else extends -- the same division as
        // `shift_tap_down`, so the shift key means one thing per platform
        // rather than one thing per gesture.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                drag_selection_start(platform, true, PointerKind::Mouse, true, true, 1, true)
                    .selects,
                DragStartSelects::Expand,
                "{platform:?}"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert_eq!(
                drag_selection_start(platform, true, PointerKind::Mouse, true, true, 1, true)
                    .selects,
                DragStartSelects::Extend,
                "{platform:?}"
            );
        }
        // And with nothing selected the shift branch is not taken at all.
        assert_eq!(
            drag_selection_start(
                TargetPlatform::MacOS,
                true,
                PointerKind::Mouse,
                true,
                false,
                1,
                true
            )
            .selects,
            DragStartSelects::CaretAtTheFinger
        );
    }

    #[test]
    fn a_field_that_does_not_select_does_not_even_set_the_flags() {
        // The only early return above the assignments.
        for platform in TargetPlatform::ALL {
            assert_eq!(
                drag_selection_start(platform, false, PointerKind::Touch, false, true, 1, true),
                DragSelectionStart {
                    selects: DragStartSelects::Nothing,
                    shows_magnifier: false,
                    sets_the_overlay_flags: false,
                },
                "{platform:?}"
            );
        }
    }

    // -- And the drag that grows out of the press ---------------------------

    #[test]
    fn the_drag_keeps_doing_what_the_press_started_doing() {
        // The Apple branch does not ask whether the field has focus **now** --
        // by this point it does, because the press took it. It asks the
        // question the press answered, preserved in the flag. That is the
        // whole reason the flag exists.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert_eq!(
                long_press_move_update(platform, true, true, false),
                LongPressMove::SelectWordsInRange,
                "{platform:?}: the press began unfocused, so the drag selects words"
            );
            assert_eq!(
                long_press_move_update(platform, true, false, false),
                LongPressMove::MoveCaretAndFloatingCursor,
                "{platform:?}: it began in a live field, so the drag places the caret"
            );
            assert_eq!(
                long_press_move_update(platform, true, false, true),
                LongPressMove::SelectWordsInRange,
                "{platform:?}: read-only is the other way in"
            );
        }
    }

    #[test]
    fn and_the_start_and_the_move_agree_about_which_it_is() {
        // The pair only works if the two answer the same question. Written as
        // one claim, because a port where the press placed a caret and the
        // drag selected words would be worse than either alone.
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            for read_only in [false, true] {
                let start = long_press_start(platform, true, false, read_only);
                let moved = long_press_move_update(
                    platform,
                    true,
                    start.remembers_it_began_unfocused,
                    read_only,
                );
                // The press began unfocused, so both should be word-shaped.
                assert_eq!(start.selects, LongPressSelects::Word, "{platform:?}");
                assert_eq!(moved, LongPressMove::SelectWordsInRange, "{platform:?}");
            }

            // And from a focused editable field, both are caret-shaped.
            let start = long_press_start(platform, true, true, false);
            assert_eq!(start.selects, LongPressSelects::CaretAtTheFinger);
            assert_eq!(
                long_press_move_update(platform, true, start.remembers_it_began_unfocused, false),
                LongPressMove::MoveCaretAndFloatingCursor
            );
        }
    }

    #[test]
    fn everywhere_else_a_drag_grows_the_selection_by_words() {
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            for started_unfocused in [false, true] {
                for read_only in [false, true] {
                    assert_eq!(
                        long_press_move_update(platform, true, started_unfocused, read_only),
                        LongPressMove::SelectWordsInRange,
                        "{platform:?}"
                    );
                }
            }
        }
        for platform in TargetPlatform::ALL {
            assert_eq!(
                long_press_move_update(platform, false, true, false),
                LongPressMove::Nothing,
                "{platform:?}"
            );
        }
    }

    // -- The anchor moved because the text did ------------------------------

    #[test]
    fn a_single_line_field_corrects_sideways_and_a_multiline_one_downwards() {
        // A single-line field scrolls its text sideways; a multi-line one
        // scrolls up and down. The correction follows the field's own axis,
        // which is not a choice about the page.
        let single = drag_anchor_correction(true, 30.0, 10.0, 0.0, 0.0, None);
        assert_eq!(single, Offset::new(20.0, 0.0));

        let multi = drag_anchor_correction(false, 30.0, 10.0, 0.0, 0.0, None);
        assert_eq!(multi, Offset::new(0.0, 20.0));
    }

    #[test]
    fn the_page_around_it_answers_on_its_own_axis() {
        // A single-line field inside a vertically scrolling page corrects on
        // x for the field and on y for the page, and the two are added.
        let both = drag_anchor_correction(
            true,
            30.0,
            10.0,
            75.0,
            50.0,
            Some(crate::render::Axis::Vertical),
        );
        assert_eq!(both, Offset::new(20.0, 25.0), "one axis each");

        // And a multi-line field in a horizontally scrolling one is the
        // mirror image.
        let mirrored = drag_anchor_correction(
            false,
            30.0,
            10.0,
            75.0,
            50.0,
            Some(crate::render::Axis::Horizontal),
        );
        assert_eq!(mirrored, Offset::new(25.0, 20.0));
    }

    #[test]
    fn nothing_having_scrolled_is_no_correction_at_all() {
        // Which is the ordinary case, and it has to cost nothing: the anchor
        // is where the reader pressed.
        for single_line in [false, true] {
            for axis in [
                None,
                Some(crate::render::Axis::Horizontal),
                Some(crate::render::Axis::Vertical),
            ] {
                assert_eq!(
                    drag_anchor_correction(single_line, 12.0, 12.0, 40.0, 40.0, axis),
                    Offset::ZERO,
                    "{single_line} {axis:?}"
                );
            }
        }
    }

    #[test]
    fn with_no_scrollable_above_the_fallback_axis_cannot_matter() {
        // Upstream falls back to `AxisDirection.left`, which is horizontal.
        // It makes no difference: with no scrollable both pixel readings are
        // zero, so the term is zero whichever axis it lands on. Worth a test
        // rather than a shrug, because a future reader will wonder why the
        // fallback is not the vertical one that most pages scroll.
        let fallback = drag_anchor_correction(true, 30.0, 10.0, 0.0, 0.0, None);
        for axis in [
            crate::render::Axis::Horizontal,
            crate::render::Axis::Vertical,
        ] {
            assert_eq!(
                drag_anchor_correction(true, 30.0, 10.0, 0.0, 0.0, Some(axis)),
                fallback,
                "{axis:?}"
            );
        }
    }

    #[test]
    fn scrolling_backwards_pulls_the_anchor_the_other_way() {
        // The correction is a signed difference, not a distance: a field
        // scrolled back to where it was undoes it exactly.
        assert_eq!(
            drag_anchor_correction(true, 5.0, 20.0, 0.0, 0.0, None),
            Offset::new(-15.0, 0.0)
        );
    }

    // -- A long press means two different things ---------------------------

    const APPLE_PAIR: [TargetPlatform; 2] = [TargetPlatform::IOS, TargetPlatform::MacOS];
    const THE_REST: [TargetPlatform; 4] = [
        TargetPlatform::Android,
        TargetPlatform::Fuchsia,
        TargetPlatform::Linux,
        TargetPlatform::Windows,
    ];

    #[test]
    fn a_long_press_selects_a_word_everywhere_except_a_live_apple_field() {
        // Where it means "let me put the caret here" instead. A port that
        // treated the two alike would get one of them wrong, and which one
        // depends on which it copied.
        for platform in THE_REST {
            let start = long_press_start(platform, true, true, false);
            assert_eq!(start.selects, LongPressSelects::Word, "{platform:?}");
            assert!(!start.starts_floating_cursor, "{platform:?}");
        }
        for platform in APPLE_PAIR {
            let start = long_press_start(platform, true, true, false);
            assert_eq!(
                start.selects,
                LongPressSelects::CaretAtTheFinger,
                "{platform:?}"
            );
            assert!(start.starts_floating_cursor, "{platform:?}");
        }
    }

    #[test]
    fn and_it_is_back_to_a_word_when_the_apple_field_cannot_be_typed_in() {
        // Both ways of not being typeable: no focus yet, or read-only. The
        // caret-placing gesture is for a field the reader is already in.
        for platform in APPLE_PAIR {
            for (has_focus, read_only) in [(false, false), (true, true), (false, true)] {
                let start = long_press_start(platform, true, has_focus, read_only);
                assert_eq!(
                    start.selects,
                    LongPressSelects::Word,
                    "{platform:?} focus={has_focus} read_only={read_only}"
                );
                assert!(!start.starts_floating_cursor);
            }
        }
    }

    #[test]
    fn the_haptic_is_not_given_on_every_path() {
        // Three Apple branches and only the middle one buzzes.
        //
        // Unfocused: the field is about to take focus and put a keyboard on
        // the screen, which is feedback enough. Editable: the floating cursor
        // is its own feedback, and a buzz would announce a selection that is
        // not happening. Read-only and focused: nothing else is about to
        // happen, so the buzz is what says the press landed.
        for platform in APPLE_PAIR {
            assert!(
                !long_press_start(platform, true, false, false).haptic,
                "{platform:?} unfocused"
            );
            assert!(
                long_press_start(platform, true, true, true).haptic,
                "{platform:?} focused and read-only"
            );
            assert!(
                !long_press_start(platform, true, true, false).haptic,
                "{platform:?} focused and editable"
            );
        }
        // Everywhere else it buzzes whatever the field is doing.
        for platform in THE_REST {
            for (has_focus, read_only) in [(false, false), (true, false), (true, true)] {
                assert!(
                    long_press_start(platform, true, has_focus, read_only).haptic,
                    "{platform:?}"
                );
            }
        }
    }

    #[test]
    fn only_an_unfocused_apple_press_is_remembered_as_such() {
        // The flag exists for the drag that follows: on Apple, dragging after
        // a long press extends by words when the press began unfocused, and
        // by then the field has focus -- the very fact the drag needs.
        for platform in APPLE_PAIR {
            assert!(long_press_start(platform, true, false, false).remembers_it_began_unfocused);
            assert!(!long_press_start(platform, true, true, false).remembers_it_began_unfocused);
        }
        for platform in THE_REST {
            assert!(
                !long_press_start(platform, true, false, false).remembers_it_began_unfocused,
                "{platform:?}: the drag there does not ask"
            );
        }
    }

    #[test]
    fn the_magnifier_comes_up_after_the_switch_and_so_on_every_path() {
        for platform in TargetPlatform::ALL {
            for (has_focus, read_only) in [(false, false), (true, false), (true, true)] {
                assert!(
                    long_press_start(platform, true, has_focus, read_only).shows_magnifier,
                    "{platform:?}"
                );
            }
        }
    }

    #[test]
    fn a_field_that_does_not_select_returns_before_any_of_it() {
        for platform in TargetPlatform::ALL {
            assert_eq!(
                long_press_start(platform, false, true, false),
                LongPressStart {
                    selects: LongPressSelects::Nothing,
                    haptic: false,
                    starts_floating_cursor: false,
                    remembers_it_began_unfocused: false,
                    shows_magnifier: false,
                },
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_end_puts_the_magnifier_and_the_flag_away_on_every_platform() {
        // The end and the cancel share this: a press taken away has to leave
        // the same way one that finished does.
        for platform in TargetPlatform::ALL {
            for enabled in [false, true] {
                let end = long_press_end(platform, enabled, true);
                assert!(end.hides_magnifier, "{platform:?}");
                assert!(end.clears_the_flag, "{platform:?}");
            }
        }
    }

    #[test]
    fn but_only_ios_ends_the_floating_cursor_although_macos_starts_one() {
        // Upstream's asymmetry, ported as written. `onSingleLongTapStart`
        // begins a floating cursor on `iOS || macOS`; the end gates on
        // `defaultTargetPlatform == TargetPlatform.iOS` alone.
        assert!(long_press_start(TargetPlatform::MacOS, true, true, false).starts_floating_cursor);
        assert!(
            !long_press_end(TargetPlatform::MacOS, true, true).ends_floating_cursor,
            "and nothing ends it"
        );
        assert!(long_press_end(TargetPlatform::IOS, true, true).ends_floating_cursor);
    }

    #[test]
    fn and_a_press_that_ended_on_a_range_was_never_placing_a_caret() {
        // The collapsed half of the same gate.
        assert!(!long_press_end(TargetPlatform::IOS, true, false).ends_floating_cursor);
        assert!(!long_press_end(TargetPlatform::IOS, false, true).ends_floating_cursor);
    }

    // -- The heavier gestures, and where they disagree with each other ------

    #[test]
    fn a_double_tap_selects_a_word_but_only_raises_a_menu_if_one_was_wanted() {
        // The gate is what keeps a double-click with a mouse from popping a
        // menu nobody asked for: a double tap goes with whatever the tap
        // before it decided, and it does not set the flag itself.
        assert_eq!(
            double_tap_down(true, true),
            PressAction {
                selects: true,
                shows_toolbar: true,
                raises_the_flag: false
            }
        );
        assert_eq!(
            double_tap_down(true, false),
            PressAction {
                selects: true,
                shows_toolbar: false,
                raises_the_flag: false
            },
            "selects, and says nothing"
        );
    }

    #[test]
    fn and_a_field_that_does_not_select_does_neither() {
        // Both halves are inside upstream's `selectionEnabled` check.
        for should_show in [false, true] {
            assert_eq!(
                double_tap_down(false, should_show),
                PressAction {
                    selects: false,
                    shows_toolbar: false,
                    raises_the_flag: false
                }
            );
        }
    }

    #[test]
    fn a_force_press_raises_the_flag_before_it_asks_whether_the_field_selects() {
        // The assignment is above upstream's early return, so a field that
        // does not select still leaves it true. Nobody presses hard by
        // accident, and that is the claim the flag carries forward whatever
        // this particular field does with it.
        assert!(force_press_start(false).raises_the_flag);
        assert!(!force_press_start(false).selects, "and does nothing else");
        assert!(force_press_start(true).raises_the_flag);
    }

    #[test]
    fn the_start_shows_the_toolbar_without_asking_and_the_end_asks() {
        // The asymmetry between the two. The start has just set the flag
        // itself; asking would be asking its own question back.
        assert!(force_press_start(true).shows_toolbar);

        // The end asks, and ordinarily the answer is yes because the start
        // set it. The one thing that closes the gate in between is a drag: a
        // force press the reader turned into a scroll clears the flag, and
        // then letting go does not pop a toolbar over the text they were
        // scrolling to.
        assert!(force_press_end(true).shows_toolbar);
        assert!(
            !force_press_end(false).shows_toolbar,
            "the press became a scroll"
        );
        assert!(
            force_press_end(false).selects,
            "but the selection still lands where the finger left"
        );
    }

    #[test]
    fn neither_end_of_a_force_press_is_the_double_tap_rule() {
        // Three handlers, three different answers to the same two questions,
        // which is why they are three functions rather than one with flags.
        let disabled_gate = (
            double_tap_down(true, false),
            force_press_start(true),
            force_press_end(false),
        );
        assert!(!disabled_gate.0.shows_toolbar, "double tap obeys the gate");
        assert!(disabled_gate.1.shows_toolbar, "the start ignores it");
        assert!(!disabled_gate.2.shows_toolbar, "the end obeys it");
    }

    // -- And the other half: what the phones decide on the way up -----------

    /// A plain tap with a finger: no shift, a selection to shift from, focused.
    fn up(platform: TargetPlatform) -> TapUp {
        single_tap_up(platform, true, false, true, true, PointerKind::Touch)
    }

    #[test]
    fn the_desktops_have_already_decided_by_the_time_the_button_comes_up() {
        // The mirror of `shift_tap_down`: each list is the other's
        // complement, and between them every platform is answered exactly
        // once.
        for platform in [
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
        ] {
            assert_eq!(up(platform), TapUp::Nothing, "{platform:?}");
            assert_ne!(
                shift_tap_down(platform, true, true, true),
                ShiftTapDown::Nothing,
                "{platform:?}: and it did decide on the way down"
            );
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::IOS,
        ] {
            assert_ne!(up(platform), TapUp::Nothing, "{platform:?}");
            assert_eq!(
                shift_tap_down(platform, true, true, true),
                ShiftTapDown::Nothing,
                "{platform:?}: and it did not decide on the way down"
            );
        }
    }

    #[test]
    fn android_offers_spelling_after_a_plain_tap_and_fuchsia_does_not() {
        // The two branches are otherwise the same five lines, and this is the
        // only difference between them. Folding them together would lose the
        // spell-check on Android or invent it on Fuchsia.
        assert_eq!(
            up(TargetPlatform::Android),
            TapUp::PlaceCaretAndOfferSpelling
        );
        assert_eq!(up(TargetPlatform::Fuchsia), TapUp::PlaceCaret);
    }

    #[test]
    fn ios_has_the_shift_from_zero_rule_too_but_on_the_way_up() {
        // The two Apple platforms agree about the behaviour and disagree
        // about when: macOS settles it on tap down, iOS on tap up. Each
        // carries its own copy of upstream's comment.
        assert_eq!(
            single_tap_up(
                TargetPlatform::IOS,
                true,
                true,
                true,
                false,
                PointerKind::Touch
            ),
            TapUp::ExpandFromTheStart
        );
        assert_eq!(
            single_tap_up(
                TargetPlatform::IOS,
                true,
                true,
                true,
                true,
                PointerKind::Touch
            ),
            TapUp::Expand
        );
        assert_eq!(
            shift_tap_down(TargetPlatform::MacOS, true, true, false),
            ShiftTapDown::ExpandFromTheStart,
            "the same answer at the other end of the tap"
        );
        assert_eq!(
            shift_tap_down(TargetPlatform::IOS, true, true, false),
            ShiftTapDown::Nothing,
            "and iOS says nothing on the way down"
        );
    }

    #[test]
    fn a_shift_tap_on_android_extends_rather_than_expanding() {
        // The phones split the same way the desktops do: Apple expands,
        // everyone else extends.
        for platform in [TargetPlatform::Android, TargetPlatform::Fuchsia] {
            assert_eq!(
                single_tap_up(platform, true, true, true, true, PointerKind::Touch),
                TapUp::Extend,
                "{platform:?}"
            );
        }

        // And the same `isShiftPressedValid` half as the tap-down twin, for
        // the same reason upstream gives: there is nothing to extend from.
        // The two ends of one tap carry one copy of this rule each, so both
        // need saying.
        for platform in TargetPlatform::ALL {
            let held = single_tap_up(platform, true, true, false, true, PointerKind::Touch);
            let released = single_tap_up(platform, true, false, false, true, PointerKind::Touch);
            assert_eq!(
                held, released,
                "{platform:?}: shift with no selection is shift not pressed"
            );
        }
    }

    #[test]
    fn a_precise_device_on_ios_places_the_caret_and_takes_the_menu_down() {
        // The long touch rule is only reached under a finger. A mouse can aim
        // and does not need a second tap to say where it meant, so it gets a
        // precise caret and the menu goes away.
        for kind in [
            PointerKind::Mouse,
            PointerKind::Trackpad,
            PointerKind::Stylus,
            PointerKind::InvertedStylus,
        ] {
            assert_eq!(
                single_tap_up(TargetPlatform::IOS, true, false, true, true, kind),
                TapUp::PlaceCaretAndHideToolbar,
                "{kind:?}"
            );
        }
        for kind in [PointerKind::Touch, PointerKind::Unknown] {
            assert_eq!(
                single_tap_up(TargetPlatform::IOS, true, false, true, true, kind),
                TapUp::AskTheTouchRule,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn the_device_kind_only_matters_on_ios() {
        // Everywhere else the branch is the same for a mouse and a finger, so
        // a port that consulted the kind generally would be inventing a rule.
        for platform in TargetPlatform::ALL {
            if platform == TargetPlatform::IOS {
                continue;
            }
            assert_eq!(
                single_tap_up(platform, true, false, true, true, PointerKind::Mouse),
                up(platform),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_field_that_cannot_be_selected_in_still_takes_the_keyboard() {
        // Upstream's `requestKeyboard()` sits after the switch and the
        // disabled branch returns through it. The reader tapped a text field,
        // and typing is the other thing they might have meant.
        for platform in TargetPlatform::ALL {
            assert_eq!(
                single_tap_up(platform, false, false, true, true, PointerKind::Touch),
                TapUp::SelectionDisabled,
                "{platform:?}"
            );
        }
        assert!(tap_up_requests_keyboard());
    }

    // -- What a right-click means, which is not what a left-click means -----

    use crate::editable_text::TargetPlatform;

    const APPLE: [TargetPlatform; 2] = [TargetPlatform::IOS, TargetPlatform::MacOS];
    const REST: [TargetPlatform; 4] = [
        TargetPlatform::Android,
        TargetPlatform::Fuchsia,
        TargetPlatform::Linux,
        TargetPlatform::Windows,
    ];

    #[test]
    fn a_right_click_selects_a_word_on_apple_and_only_a_caret_elsewhere() {
        // Right-clicking a word on macOS selects it, so the menu's `Copy` and
        // `Look Up` have something to act on. On Windows a right-click is
        // about opening the menu, and moving the caret is as much as it does.
        for platform in APPLE {
            assert_eq!(
                secondary_tap(platform, true, false, false, true).selects,
                SecondarySelects::Word,
                "{platform:?}"
            );
        }
        for platform in REST {
            assert_eq!(
                secondary_tap(platform, true, false, false, true).selects,
                SecondarySelects::Position,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn apple_keeps_a_selection_the_click_landed_inside() {
        // `_lastSecondaryTapWasOnSelection`. Right-clicking within a
        // highlighted run leaves it alone, so `Copy` copies what the reader
        // highlighted rather than the one word under the pointer.
        for platform in APPLE {
            assert_eq!(
                secondary_tap(platform, true, true, true, true).selects,
                SecondarySelects::Nothing,
                "{platform:?}: inside the selection, and focused"
            );
            assert_eq!(
                secondary_tap(platform, true, true, false, true).selects,
                SecondarySelects::Word,
                "{platform:?}: an unfocused field has no selection to keep"
            );
            // The case the rule is actually for, and the one that tells the
            // condition apart from a plain `!hasFocus`: a focused field,
            // right-clicked **outside** whatever was selected. That selects
            // the word under the pointer, which is what the menu will act on.
            assert_eq!(
                secondary_tap(platform, true, false, true, true).selects,
                SecondarySelects::Word,
                "{platform:?}: focused, but not on the selection"
            );
        }
    }

    #[test]
    fn and_the_others_only_move_the_caret_when_the_field_was_not_focused() {
        // A focused field keeps whatever it had, wherever the click landed --
        // so the position of the click does not enter into it at all.
        for platform in REST {
            for on_selection in [false, true] {
                assert_eq!(
                    secondary_tap(platform, true, on_selection, true, true).selects,
                    SecondarySelects::Nothing,
                    "{platform:?} focused"
                );
                assert_eq!(
                    secondary_tap(platform, true, on_selection, false, true).selects,
                    SecondarySelects::Position,
                    "{platform:?} unfocused"
                );
            }
        }
    }

    #[test]
    fn apple_re_shows_the_toolbar_where_the_others_toggle_it() {
        // Hide-then-show means a second right-click somewhere else *moves* the
        // menu there. Toggling means a second right-click dismisses it. A port
        // that used one everywhere would be wrong on four platforms or on two.
        for platform in APPLE {
            assert_eq!(
                secondary_tap(platform, true, false, true, true).toolbar,
                SecondaryToolbar::Reshow,
                "{platform:?}"
            );
        }
        for platform in REST {
            assert_eq!(
                secondary_tap(platform, true, false, true, true).toolbar,
                SecondaryToolbar::Toggle,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_toolbar_flag_gates_only_the_apple_branch() {
        // Upstream's shape, and not an oversight: a toggle the flag
        // suppressed would leave a menu up with no way to dismiss it.
        for platform in APPLE {
            assert_eq!(
                secondary_tap(platform, true, false, true, false).toolbar,
                SecondaryToolbar::Nothing,
                "{platform:?}"
            );
        }
        for platform in REST {
            assert_eq!(
                secondary_tap(platform, true, false, true, false).toolbar,
                SecondaryToolbar::Toggle,
                "{platform:?}: still toggles"
            );
        }
    }

    #[test]
    fn a_field_that_does_not_select_does_nothing_at_all() {
        // Upstream's first line, before the switch.
        for platform in TargetPlatform::ALL {
            assert_eq!(
                secondary_tap(platform, false, true, true, true),
                SecondaryTap {
                    selects: SecondarySelects::Nothing,
                    toolbar: SecondaryToolbar::Nothing,
                },
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_right_click_on_the_edge_of_a_selection_counts_as_on_it() {
        // Where a left-click does not. The two are asking different
        // questions: a left-click on the edge is aimed at the handle, and
        // there are no handles under a right-click.
        let run = (4, 9);
        assert!(last_secondary_tap_was_on_selection(Some(run), 4));
        assert!(last_secondary_tap_was_on_selection(Some(run), 9));
        assert_eq!(
            tap_outcome(false, run, 4, true, false, true),
            TapOutcome::SelectWordEdge,
            "the left-click on the same spot does not"
        );

        assert!(!last_secondary_tap_was_on_selection(Some(run), 10));
        assert!(
            !last_secondary_tap_was_on_selection(None, 6),
            "no selection is nothing to have clicked inside of"
        );
    }

    #[test]
    fn the_two_affinities_carry_the_names_the_wire_already_used() {
        // The strings were being sent as literals before the type existed.
        assert_eq!(
            TextAffinity::Downstream.as_wire(),
            "TextAffinity.downstream"
        );
        assert_eq!(TextAffinity::Upstream.as_wire(), "TextAffinity.upstream");
        assert_eq!(TextAffinity::default(), TextAffinity::Downstream);
        // Item by item: `ALL.len() == 2` is a claim about an array literal
        // and would hold whatever the two were.
        assert_eq!(
            TextAffinity::ALL,
            [TextAffinity::Upstream, TextAffinity::Downstream]
        );
    }
}
