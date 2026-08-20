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
//! ## What is not here
//!
//! [`TextSelectionOverlay`] and [`SelectionOverlay`] position handles and a
//! toolbar in an `Overlay`, which this crate does not have; what is ported is
//! their configuration. The gesture builder's own recognisers are the
//! `tap_and_drag` family, already ported.

use crate::render::Offset;

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

    /// Whether a tap that changed nothing should still reach `onUserTap`.
    pub fn reports_tap(&self, changed_something: bool) -> bool {
        self.on_user_tap_always_called || changed_something
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
/// entry list and its ordering; nothing hosts the widgets yet, so what is
/// ported here is the configuration and the visibility rules.
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
    fn a_tap_that_changed_nothing_is_reported_only_when_asked_for() {
        // Which is what a form that scrolls to the focused field needs.
        let ordinary = TextSelectionGestureDetector::new();
        assert!(ordinary.reports_tap(true));
        assert!(!ordinary.reports_tap(false));

        let always = TextSelectionGestureDetector::new().with_on_user_tap_always_called(true);
        assert!(always.reports_tap(true));
        assert!(always.reports_tap(false));
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
