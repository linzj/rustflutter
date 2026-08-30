//! The live messenger: `scaffold_messenger.rs`'s queue, hosted.
//!
//! `scaffold_messenger.rs` decides everything about *which* message is showing
//! -- the queue, the closed reasons, that `clearSnackBars` keeps the one on
//! screen and drops the rest, that accessible navigation skips the closing
//! animation -- and has never had anywhere to put one. This is the anywhere.
//!
//! # A snack bar is not a modal
//!
//! It goes over the page and it does **not** take the page's presses: you can
//! keep working while it is up, which is the whole difference between a message
//! and a dialog. So it is an overlay entry with no barrier, and the only thing
//! it swallows is a tap on the bar itself.
//!
//! It is also not a portal. A tooltip belongs to the button it names and builds
//! in that button's context; a snack bar belongs to the application and is put
//! up from anywhere, so it builds in the overlay's -- which is exactly what
//! `OverlayEntry` is for. The two mechanisms sit side by side in the same
//! theatre and this is the case that needs the second one.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, many};
use crate::render::{Alignment, RenderAlign};
use crate::scaffold_messenger::{ScaffoldMessengerState, SnackBarClosedReason};
use crate::theatre::OverlayHandle;

/// What a queued message is built from.
type MessageBuilder = Rc<dyn Fn() -> AnyWidget>;

struct MessengerInner {
    /// The decisions. Untouched from `scaffold_messenger.rs`.
    state: ScaffoldMessengerState,
    /// The widgets, in the same order as the queue. Kept apart for the same
    /// reason the overlay keeps its builders apart from `OverlayState`: the
    /// queue can decide everything it decides without seeing a widget.
    queue: Vec<(SnackBarDisplay, MessageBuilder)>,
    /// The overlay entry the front of the queue is currently occupying.
    showing: Option<u64>,
    /// When the bar on screen went up, on the frame clock. `None` until the
    /// first `advance` after it did -- upstream starts its timer once the
    /// entry animation has completed, and with no entry animation here the
    /// first frame the bar is up is that moment.
    shown_micros: Option<i64>,
}

/// A snack bar's own display rules. Upstream's `SnackBar.duration` and
/// `SnackBar.persist`.
///
/// # A bar with an action does not time out
///
/// Upstream's constructor ends `persist = persist ?? action != null`, and the
/// doc says why: a persisting bar "remains visible even after the timeout,
/// until the user taps the action button or the close icon". A message the
/// reader is being asked to *act* on must not leave while they are reading it,
/// because leaving takes the action with it.
///
/// So [`SnackBarDisplay::with_action`] is what a caller uses when the bar has
/// one, and it turns persistence on -- rather than the caller remembering to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnackBarDisplay {
    /// Upstream's `_snackBarDisplayDuration`, four seconds.
    pub duration_micros: i64,
    pub persist: bool,
}

impl SnackBarDisplay {
    /// Upstream's `_snackBarDisplayDuration`.
    pub const DEFAULT_DURATION_MICROS: i64 = 4_000_000;

    /// Four seconds, and it times out. Upstream's defaults for a bar with no
    /// action.
    pub fn new() -> SnackBarDisplay {
        SnackBarDisplay {
            duration_micros: SnackBarDisplay::DEFAULT_DURATION_MICROS,
            persist: false,
        }
    }

    /// A bar with an action, which therefore persists.
    pub fn with_action() -> SnackBarDisplay {
        SnackBarDisplay {
            persist: true,
            ..SnackBarDisplay::new()
        }
    }

    pub fn with_duration_micros(mut self, micros: i64) -> Self {
        self.duration_micros = micros;
        self
    }

    /// Upstream's explicit `persist:`, which overrides the action rule in
    /// either direction.
    pub fn with_persist(mut self, persist: bool) -> Self {
        self.persist = persist;
        self
    }
}

impl Default for SnackBarDisplay {
    fn default() -> SnackBarDisplay {
        SnackBarDisplay::new()
    }
}

/// A live `ScaffoldMessenger`.
#[derive(Clone)]
pub struct Messenger {
    inner: Rc<RefCell<MessengerInner>>,
    overlay: Rc<OverlayHandle>,
}

impl Messenger {
    /// A messenger that presents into `overlay`.
    ///
    /// `scaffolds` stands in for upstream's registered descendant scaffolds --
    /// upstream asserts there is at least one, because a bar shown to nobody
    /// would sit at the head of the queue for ever and block every later one.
    pub fn new(overlay: Rc<OverlayHandle>, scaffold_id: u64) -> Messenger {
        let mut state = ScaffoldMessengerState::new();
        state.register(crate::scaffold_messenger::ScaffoldState::new(scaffold_id).registration());
        Messenger {
            inner: Rc::new(RefCell::new(MessengerInner {
                state,
                queue: Vec::new(),
                showing: None,
                shown_micros: None,
            })),
            overlay,
        }
    }

    /// Upstream `ScaffoldMessengerState.showSnackBar`, with the bar's own
    /// display rules.
    ///
    /// Queued rather than shown when one is already up: two messages at once
    /// would mean one of them was not read.
    pub fn show_snack_bar_with(
        &self,
        display: SnackBarDisplay,
        bar: impl Fn() -> AnyWidget + 'static,
    ) {
        {
            let inner = &mut *self.inner.borrow_mut();
            inner.state.show_snack_bar();
            inner.queue.push((display, Rc::new(bar)));
        }
        self.present();
    }

    /// A snack bar with upstream's defaults: four seconds, and no action, so it
    /// times out.
    pub fn show_snack_bar(&self, bar: impl Fn() -> AnyWidget + 'static) {
        self.show_snack_bar_with(SnackBarDisplay::new(), bar)
    }

    /// Upstream's `_snackBarTimer`, on the frame clock: the bar on screen goes
    /// once its `duration` is up.
    ///
    /// Answers whether another frame is wanted, so a caller can stop asking
    /// when nothing is counting down.
    ///
    /// A **persisting** bar is not counted. That is upstream's
    /// `if (snackBar.persist) return;` inside the timer's callback -- the timer
    /// still runs, and it still declines to hide the bar. See
    /// [`SnackBarDisplay::persist`], which defaults to *true* for a bar with an
    /// action.
    pub fn advance(&self, frame_time_micros: i64) -> bool {
        let expired = {
            let inner = &mut *self.inner.borrow_mut();
            let Some(display) = inner.queue.first().map(|(display, _)| *display) else {
                inner.shown_micros = None;
                return false;
            };
            if inner.showing.is_none() {
                return false;
            }
            let shown = *inner.shown_micros.get_or_insert(frame_time_micros);
            if display.persist {
                // Upstream's timer is armed either way; it is the callback that
                // declines. Modelled the same way so a bar that is later made
                // non-persisting does not get a fresh four seconds.
                return false;
            }
            frame_time_micros - shown >= display.duration_micros
        };
        if expired {
            self.hide_current(SnackBarClosedReason::Timeout);
            return false;
        }
        true
    }

    /// Upstream `hideCurrentSnackBar`: close the front one politely and let the
    /// next take its place.
    pub fn hide_current(&self, reason: SnackBarClosedReason) -> bool {
        let hidden = {
            let inner = &mut *self.inner.borrow_mut();
            if inner.queue.is_empty() {
                return false;
            }
            // `hide_current_snack_bar` answers whether a closing animation
            // should play, and with accessible navigation on it has already
            // dropped the bar. Either way this port has no closing animation
            // yet, so the bar goes now -- and `remove_current_snack_bar` is
            // idempotent about an empty queue, which is what makes calling both
            // safe rather than clever.
            inner.state.hide_current_snack_bar(reason);
            inner.state.remove_current_snack_bar(reason);
            inner.queue.remove(0);
            inner.shown_micros = None;
            inner.showing.take()
        };
        if let Some(entry) = hidden {
            self.overlay.remove(entry);
        }
        self.present();
        true
    }

    /// Upstream `clearSnackBars`.
    ///
    /// It drops everything waiting and then **hides** the one on screen -- the
    /// pure module's `clear_snack_bars` ends with `hide_current_snack_bar`, and
    /// so does upstream's. The distinction it is drawing is between *hiding*
    /// the current bar and *removing* it: hidden, it plays its closing
    /// animation and the reader sees the message end; removed, it is gone
    /// mid-word.
    ///
    /// **That distinction is invisible here**, because this port has no closing
    /// animation yet: hiding and removing both take the bar off the screen at
    /// once. It is written the upstream way so that the difference appears on
    /// its own when the animation does, rather than needing to be remembered.
    pub fn clear(&self) -> bool {
        let hidden = {
            let inner = &mut *self.inner.borrow_mut();
            let cleared = inner.state.clear_snack_bars();
            if !cleared {
                return false;
            }
            inner.queue.clear();
            inner
                .state
                .remove_current_snack_bar(SnackBarClosedReason::Remove);
            inner.shown_micros = None;
            inner.showing.take()
        };
        if let Some(entry) = hidden {
            self.overlay.remove(entry);
        }
        true
    }

    /// How many messages are waiting, including the one on screen.
    pub fn queued(&self) -> usize {
        self.inner.borrow().queue.len()
    }

    /// Whether a message is on screen.
    pub fn is_showing(&self) -> bool {
        self.inner.borrow().showing.is_some()
    }

    /// How many entries the **overlay** is actually holding.
    ///
    /// Asked of the overlay rather than of this messenger's own bookkeeping,
    /// which could only ever agree with itself: the assertion worth making is
    /// that a queue of three put one bar on screen, and a count of what this
    /// object thinks it did cannot fail that way.
    pub fn overlay_entries(&self) -> usize {
        self.overlay.entry_count()
    }

    /// Puts the front of the queue on screen, if nothing is there already.
    fn present(&self) {
        let builder = {
            let inner = self.inner.borrow();
            if inner.showing.is_some() {
                return;
            }
            match inner.queue.first() {
                Some((_, builder)) => Rc::clone(builder),
                None => return,
            }
        };
        let entry = self.overlay.insert(move || snack_bar_slot((builder)()));
        self.inner.borrow_mut().showing = entry;
    }
}

/// Where a snack bar sits: along the bottom, its own height.
///
/// Upstream's `Scaffold` puts it in the bottom slot of its layout; here the
/// overlay is the surface, so an alignment is the whole of it.
pub fn snack_bar_slot(bar: AnyWidget) -> AnyWidget {
    many(vec![bar], |mut rendered| {
        RenderAlign::new(
            Alignment::new(0.0, 1.0),
            // **No announcement here.** It used to be, and that was the
            // wrong place: upstream puts it in `_SnackBarState.build`, on the
            // widget, and four of the gallery's demos build a `Snackbar`
            // without going anywhere near this door -- so those bars appeared
            // silently. `Snackbar` announces itself now.
            //
            // The first draft of this note said a wrapper here would announce
            // the bar *twice*, and that is **not true** -- a mutation putting
            // it back turns nothing red. Nesting one merging box inside
            // another still yields one stop (see
            // `a_merge_inside_a_merge_is_still_one_stop`): the outer node folds
            // the inner one in, and the inner never opens. So the reason to
            // leave it out is not that it would double anything but that it is
            // the wrong place and would be dead weight -- which is a smaller
            // claim, and the true one.
            //
            // What remains is what this slot is actually for: where the bar
            // sits.
            rendered.pop().expect("the snack bar"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext, Component, ElementTree};
    use crate::render::{
        BoxConstraints, HitTestResult, Offset, RenderBox, RenderConstrainedBox,
        RenderPointerRegion, RenderRef, Size,
    };
    use crate::theatre::overlay;

    #[test]
    fn a_bar_shown_through_the_messenger_is_announced_exactly_once() {
        // The announcement lives on the `Snackbar` widget, where upstream puts
        // it, so this door adds nothing. That is the point: a bar built
        // directly into a column -- which four of the gallery's demos do --
        // is announced too, and a bar that came through here is announced
        // once rather than twice.
        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            snack_bar_slot(crate::framework::component(crate::controls::Snackbar::new(
                7, "Saved",
            ))),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        RenderBox::layout(&mut root, BoxConstraints::loose(300.0, 300.0));
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(Size::new(300.0, 300.0), &root).unwrap_or_default();
        crate::semantics::set_enabled(false);

        let live: Vec<&crate::semantics::SemanticsNode> = nodes
            .iter()
            .filter(|node| node.properties.flags.is_live_region)
            .collect();
        assert_eq!(live.len(), 1, "announced once, not twice: {live:?}");
        assert_eq!(live[0].properties.label, "Saved");
    }

    const PAGE_TARGET: u64 = 7001;
    const BAR_TARGET: u64 = 7002;

    fn bar() -> AnyWidget {
        crate::framework::leaf(|| {
            RenderPointerRegion::new(BAR_TARGET, RenderConstrainedBox::tight(400.0, 48.0))
                .with_behavior(crate::render::HitTestBehavior::Opaque)
        })
    }

    fn page() -> AnyWidget {
        crate::framework::leaf(|| {
            RenderPointerRegion::new(PAGE_TARGET, RenderConstrainedBox::tight(800.0, 600.0))
                .with_behavior(crate::render::HitTestBehavior::Opaque)
        })
    }

    fn mounted() -> (ElementTree, Messenger) {
        let slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&slot);

        struct Finder(Rc<RefCell<Option<Rc<OverlayHandle>>>>);
        impl Component for Finder {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = OverlayHandle::of(context);
                page()
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Finder(sink))));
        tree.build_render_tree();
        let handle = slot.borrow().clone().expect("an overlay");
        let messenger = Messenger::new(handle, 1);
        (tree, messenger)
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        root
    }

    fn targets_at(tree: &mut ElementTree, at: Offset) -> Vec<u64> {
        let root = laid_out(tree);
        let mut result = HitTestResult::new();
        root.hit_test(at, &mut result);
        result.path.iter().map(|entry| entry.target).collect()
    }

    /// Where the bar ended up, by finding it in the render tree.
    fn bar_bottom(tree: &mut ElementTree) -> Option<f32> {
        let root = laid_out(tree);
        let mut found = None;
        fn walk(handle: &RenderRef, at: Offset, found: &mut Option<f32>) {
            let children: Vec<(RenderRef, Offset)> = handle.with(|object| {
                let mut kids = Vec::new();
                object.visit_children(&mut |child, offset| {
                    if let Some(child) = child.as_any().downcast_ref::<RenderRef>() {
                        kids.push((child.clone(), at.plus(offset)));
                    }
                });
                if object.size() == Size::new(400.0, 48.0) {
                    *found = Some(at.dy + 48.0);
                }
                kids
            });
            for (child, offset) in children {
                walk(&child, offset, found);
            }
        }
        walk(&root, Offset::ZERO, &mut found);
        found
    }

    #[test]
    fn nothing_is_showing_until_something_is_said() {
        let (mut tree, messenger) = mounted();
        assert!(!messenger.is_showing());
        assert_eq!(messenger.queued(), 0);
        assert!(targets_at(&mut tree, Offset::new(400.0, 570.0)).contains(&PAGE_TARGET));
    }

    #[test]
    fn a_snack_bar_goes_up_along_the_bottom() {
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();
        assert!(messenger.is_showing());

        assert_eq!(
            bar_bottom(&mut tree),
            Some(600.0),
            "flush with the bottom of the overlay"
        );
    }

    // -- A message is not a modal -------------------------------------------------

    #[test]
    fn the_page_is_still_reachable_beside_the_bar() {
        // The whole difference between a message and a dialog: you can keep
        // working while it is up.
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();

        let above = targets_at(&mut tree, Offset::new(400.0, 100.0));
        assert!(
            above.contains(&PAGE_TARGET),
            "a snack bar puts up no barrier: {above:?}"
        );
        assert_eq!(crate::theatre::modal_count(), 0);
    }

    #[test]
    fn but_the_bar_itself_takes_its_own_presses() {
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();

        let on_bar = targets_at(&mut tree, Offset::new(400.0, 580.0));
        assert!(on_bar.contains(&BAR_TARGET), "{on_bar:?}");
    }

    // -- The queue ------------------------------------------------------------------

    #[test]
    fn a_second_message_waits_rather_than_covering_the_first() {
        // Two at once would mean one of them was not read.
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();

        assert_eq!(messenger.queued(), 2);
        assert_eq!(messenger.overlay_entries(), 1, "one on screen, one waiting");
    }

    #[test]
    fn hiding_one_lets_the_next_take_its_place() {
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();
        assert_eq!(messenger.queued(), 2);

        assert!(messenger.hide_current(SnackBarClosedReason::Hide));
        tree.rebuild_dirty();
        assert_eq!(messenger.queued(), 1);
        assert!(messenger.is_showing(), "the next one came up");

        assert!(messenger.hide_current(SnackBarClosedReason::Hide));
        tree.rebuild_dirty();
        assert_eq!(messenger.queued(), 0);
        assert!(!messenger.is_showing());
    }

    #[test]
    fn hiding_when_nothing_is_up_is_not_an_error() {
        let (_tree, messenger) = mounted();
        assert!(!messenger.hide_current(SnackBarClosedReason::Hide));
    }

    #[test]
    fn clearing_takes_the_queue_and_the_bar_on_screen_with_it() {
        // Upstream drops everything waiting and then *hides* the current one,
        // which is a politer end than removing it -- the reader sees the
        // message finish rather than vanish mid-word. With no closing animation
        // in this port the two look the same, and the divergence is recorded on
        // `clear` rather than left to be rediscovered.
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        messenger.show_snack_bar(bar);
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();
        assert_eq!(messenger.queued(), 3);

        assert!(messenger.clear());
        tree.rebuild_dirty();
        assert_eq!(messenger.queued(), 0);
        assert!(!messenger.is_showing());
    }

    #[test]
    fn clearing_an_empty_queue_reports_nothing_done() {
        let (_tree, messenger) = mounted();
        assert!(!messenger.clear());
    }

    #[test]
    fn clearing_one_bar_still_counts_as_something_done() {
        let (mut tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        tree.rebuild_dirty();
        assert!(messenger.clear(), "the bar on screen is hidden by it");
        assert!(!messenger.is_showing());
    }

    #[test]
    fn the_display_duration_is_upstreams() {
        assert_eq!(
            crate::snack_bar::SNACK_BAR_DISPLAY_DURATION_MICROS,
            4_000_000,
            "long enough to read a sentence"
        );
    }

    // -- The timer, and the rule that suspends it ---------------------------------

    #[test]
    fn a_bar_with_no_action_leaves_after_four_seconds() {
        let (tree, messenger) = mounted();
        let before = messenger.overlay_entries();
        messenger.show_snack_bar(bar);
        assert_eq!(messenger.overlay_entries(), before + 1);

        // The first advance is when the clock is first known -- upstream starts
        // its timer once the entry animation completes, and there is none here.
        assert!(messenger.advance(1_000_000), "counting");
        assert!(
            messenger.advance(1_000_000 + SnackBarDisplay::DEFAULT_DURATION_MICROS - 1),
            "not yet"
        );
        assert!(!messenger.advance(1_000_000 + SnackBarDisplay::DEFAULT_DURATION_MICROS));
        assert_eq!(
            messenger.overlay_entries(),
            before,
            "and it is off the screen"
        );
        drop(tree);
    }

    #[test]
    fn a_bar_with_an_action_stays_put() {
        // Upstream's `persist = persist ?? action != null`, and its reason: a
        // message the reader is being asked to act on must not leave while they
        // are reading it, because leaving takes the action with it.
        let (tree, messenger) = mounted();
        let before = messenger.overlay_entries();
        messenger.show_snack_bar_with(SnackBarDisplay::with_action(), bar);

        messenger.advance(1_000_000);
        messenger.advance(1_000_000 + SnackBarDisplay::DEFAULT_DURATION_MICROS * 10);
        assert_eq!(
            messenger.overlay_entries(),
            before + 1,
            "ten times its duration later it is still there"
        );
        assert!(messenger.is_showing());
        drop(tree);
    }

    #[test]
    fn persist_can_be_said_either_way() {
        // Upstream's explicit `persist:` overrides the action rule in both
        // directions, so a bar with an action can still be told to go.
        assert!(!SnackBarDisplay::with_action().with_persist(false).persist);
        assert!(SnackBarDisplay::new().with_persist(true).persist);
    }

    #[test]
    fn the_next_bar_gets_its_own_four_seconds() {
        // The clock restarts when the queue moves on. A second bar that
        // inherited the first one's start would flash past.
        let (tree, messenger) = mounted();
        messenger.show_snack_bar(bar);
        messenger.show_snack_bar(bar);

        messenger.advance(0);
        assert!(!messenger.advance(SnackBarDisplay::DEFAULT_DURATION_MICROS));
        assert!(messenger.is_showing(), "the second one took its place");

        // Right after the swap the second bar has not started counting.
        let at = SnackBarDisplay::DEFAULT_DURATION_MICROS;
        assert!(messenger.advance(at), "counting again from here");
        assert!(
            messenger.advance(at + SnackBarDisplay::DEFAULT_DURATION_MICROS - 1),
            "still up, on its own clock rather than the first one's"
        );
        assert!(!messenger.advance(at + SnackBarDisplay::DEFAULT_DURATION_MICROS));
        assert!(!messenger.is_showing());
        drop(tree);
    }

    #[test]
    fn a_shorter_duration_is_honoured() {
        let (tree, messenger) = mounted();
        messenger.show_snack_bar_with(SnackBarDisplay::new().with_duration_micros(1_000_000), bar);
        messenger.advance(0);
        assert!(messenger.advance(999_999));
        assert!(!messenger.advance(1_000_000));
        assert!(!messenger.is_showing());
        drop(tree);
    }

    #[test]
    fn advancing_with_nothing_up_asks_for_no_more_frames() {
        let (tree, messenger) = mounted();
        assert!(!messenger.advance(1_000_000));
        drop(tree);
    }

    #[test]
    fn a_bar_hidden_by_hand_does_not_also_time_out() {
        let (tree, messenger) = mounted();
        let before = messenger.overlay_entries();
        messenger.show_snack_bar(bar);
        messenger.advance(0);
        assert!(messenger.hide_current(SnackBarClosedReason::Action));
        assert_eq!(messenger.overlay_entries(), before);
        assert!(!messenger.advance(SnackBarDisplay::DEFAULT_DURATION_MICROS * 2));
        assert_eq!(messenger.overlay_entries(), before, "and nothing came back");
        drop(tree);
    }
}
