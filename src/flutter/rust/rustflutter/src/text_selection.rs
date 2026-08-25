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
        assert_eq!(TextAffinity::Downstream.as_wire(), "TextAffinity.downstream");
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

