//! A port of `widgets/raw_tooltip.dart`.
//!
//! A tooltip is mostly a clock. Almost nothing in this file is about drawing:
//! it is about *when* -- how long a pointer must rest before one appears, how
//! long it stays, what cancels it, and what the second one in a row should do
//! differently from the first.

use std::collections::BTreeSet;

/// Upstream `TooltipTriggerMode`.
///
/// Touch only. A mouse hovering shows a tooltip whatever this says, because
/// hovering is not a gesture anyone has to be taught.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipTriggerMode {
    /// Never by touch; only [`RawTooltipState::ensure_tooltip_visible`].
    Manual,
    #[default]
    LongPress,
    Tap,
}

/// Upstream `TooltipPositionContext`: everything a custom position delegate is
/// told, gathered into one value so the signature does not grow a field every
/// time the layout learns something new.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipPositionContext {
    /// The centre of the target, in global coordinates.
    pub target: (f32, f32),
    pub target_size: (f32, f32),
    pub tooltip_size: (f32, f32),
    pub vertical_offset: f32,
    pub prefer_below: bool,
    /// Defaults to infinite, which is upstream's way of saying "unconstrained
    /// until somebody measures the overlay".
    pub overlay_size: (f32, f32),
}

impl TooltipPositionContext {
    pub fn new(
        target: (f32, f32),
        target_size: (f32, f32),
        tooltip_size: (f32, f32),
    ) -> TooltipPositionContext {
        TooltipPositionContext {
            target,
            target_size,
            tooltip_size,
            vertical_offset: 0.0,
            prefer_below: true,
            overlay_size: (f32::INFINITY, f32::INFINITY),
        }
    }

    pub fn with_overlay(mut self, overlay_size: (f32, f32)) -> Self {
        self.overlay_size = overlay_size;
        self
    }

    pub fn with_vertical_offset(mut self, offset: f32) -> Self {
        self.vertical_offset = offset;
        self
    }

    pub fn with_prefer_below(mut self, prefer_below: bool) -> Self {
        self.prefer_below = prefer_below;
        self
    }
}

/// Upstream `positionDependentBox`, which the default delegate calls.
///
/// The preference is a preference: it goes below if it fits, above if it does
/// not, and takes whichever side has more room if neither fits. Then it is
/// pushed horizontally to stay on screen -- **a tooltip that is off the edge is
/// worse than one that is not quite where it was asked to be.**
pub fn position_dependent_box(context: &TooltipPositionContext) -> (f32, f32) {
    let (overlay_w, overlay_h) = context.overlay_size;
    let (child_w, child_h) = context.tooltip_size;
    let (target_x, target_y) = context.target;
    let margin = 10.0;

    let fits_below = target_y + context.vertical_offset + child_h <= overlay_h - margin;
    let fits_above = target_y - context.vertical_offset - child_h >= margin;
    let below = if context.prefer_below {
        fits_below || !fits_above
    } else {
        !(fits_above || !fits_below)
    };

    let y = if below {
        (target_y + context.vertical_offset).min(overlay_h - margin - child_h)
    } else {
        (target_y - context.vertical_offset - child_h).max(margin)
    };

    let x = if overlay_w - margin * 2.0 < child_w {
        // Nowhere to put it: centre it and let it overflow evenly.
        (overlay_w - child_w) / 2.0
    } else {
        let normalised = target_x - child_w / 2.0;
        normalised.clamp(margin, overlay_w - margin - child_w)
    };
    (x, y)
}

/// Upstream's `_ExclusiveMouseRegion`, as its rule.
///
/// Nested tooltips would otherwise both light up. Upstream solves it with two
/// **static mutable flags** carried across a whole hit-test pass, so only the
/// first region hit -- child over parent, last sibling over first -- is added to
/// the result. The outermost region resets the flags on its way back out, which
/// is what makes a static safe here: the pass is synchronous and single
/// threaded, and it always begins and ends at the same place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExclusiveMouseRegion;

impl ExclusiveMouseRegion {
    /// `regions` is given in hit-test order (innermost and last-painted first).
    /// Returns the one that receives the enter and exit events, if any.
    pub fn hit(regions: &[u64]) -> Option<u64> {
        regions.first().copied()
    }
}

/// Where a tooltip's animation is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipAnimationStatus {
    #[default]
    Dismissed,
    Forward,
    Completed,
    Reverse,
}

impl TooltipAnimationStatus {
    pub fn is_dismissed(self) -> bool {
        self == TooltipAnimationStatus::Dismissed
    }

    /// Upstream's `isForwardOrCompleted`, which is the test for "the tooltip is
    /// on its way in or already there".
    pub fn is_forward_or_completed(self) -> bool {
        matches!(
            self,
            TooltipAnimationStatus::Forward | TooltipAnimationStatus::Completed
        )
    }
}

/// The one timer a tooltip keeps. Only ever one, which is why the asserts
/// exist: two of these outstanding would race.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TooltipTimer {
    /// Waiting out the hover delay before showing. The touch delay travels
    /// with it: upstream's `show` closure captures the one it was scheduled
    /// with, so a tap that will need a self-destruct timer still gets one after
    /// waiting.
    Show {
        at_ms: f32,
        touch_delay_ms: Option<f32>,
    },
    /// The touch tooltip's own lifetime, after which it hides itself.
    AutoHide { at_ms: f32 },
    /// Waiting out the dismiss delay before hiding.
    Dismiss { at_ms: f32 },
}

impl TooltipTimer {
    pub fn at_ms(self) -> f32 {
        match self {
            TooltipTimer::Show { at_ms, .. }
            | TooltipTimer::AutoHide { at_ms }
            | TooltipTimer::Dismiss { at_ms } => at_ms,
        }
    }
}

/// Upstream `RawTooltip`, as its configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct RawTooltip {
    pub semantics_tooltip: Option<String>,
    /// How long a mouse must rest before the tooltip appears. Defaults to
    /// **zero**: a mouse pointer that has come to rest on something has already
    /// waited.
    pub hover_delay_ms: f32,
    /// How long a touch-triggered tooltip stays up on its own. 1500ms -- long
    /// enough to read, and it has to end by itself because a finger that lifted
    /// has no way to say "still looking".
    pub touch_delay_ms: f32,
    /// The grace period after a mouse leaves. 100ms, which is enough to cross a
    /// one-pixel gap between a button and the tooltip it opened.
    pub dismiss_delay_ms: f32,
    pub enable_tap_to_dismiss: bool,
    pub trigger_mode: TooltipTriggerMode,
    pub enable_feedback: bool,
    pub ignore_pointer: bool,
}

impl RawTooltip {
    pub const DEFAULT_HOVER_DELAY_MS: f32 = 0.0;
    pub const DEFAULT_TOUCH_DELAY_MS: f32 = 1500.0;
    pub const DEFAULT_DISMISS_DELAY_MS: f32 = 100.0;

    pub fn new() -> RawTooltip {
        RawTooltip {
            semantics_tooltip: None,
            hover_delay_ms: RawTooltip::DEFAULT_HOVER_DELAY_MS,
            touch_delay_ms: RawTooltip::DEFAULT_TOUCH_DELAY_MS,
            dismiss_delay_ms: RawTooltip::DEFAULT_DISMISS_DELAY_MS,
            enable_tap_to_dismiss: true,
            trigger_mode: TooltipTriggerMode::LongPress,
            enable_feedback: true,
            ignore_pointer: false,
        }
    }

    pub fn with_trigger_mode(mut self, mode: TooltipTriggerMode) -> Self {
        self.trigger_mode = mode;
        self
    }

    pub fn with_hover_delay_ms(mut self, delay: f32) -> Self {
        self.hover_delay_ms = delay;
        self
    }
}

impl Default for RawTooltip {
    fn default() -> Self {
        RawTooltip::new()
    }
}

/// Upstream `RawTooltipState`.
#[derive(Clone, Debug, PartialEq)]
pub struct RawTooltipState {
    pub id: u64,
    pub widget: RawTooltip,
    status: TooltipAnimationStatus,
    timer: Option<TooltipTimer>,
    /// The **device ids** keeping this tooltip open, not a flag: two mice can
    /// hover the same thing, and the tooltip goes when the last one leaves.
    hovering_devices: BTreeSet<i32>,
    overlay_shown: bool,
    announcements: Vec<String>,
    now_ms: f32,
    /// Whether the animation controller has ever been built. Upstream's dismiss
    /// path reads the *backing* field rather than the lazy getter, so asking
    /// whether a tooltip is showing does not build the machinery for one that
    /// never has.
    controller_built: bool,
}

impl RawTooltipState {
    pub fn new(id: u64, widget: RawTooltip) -> RawTooltipState {
        RawTooltipState {
            id,
            widget,
            status: TooltipAnimationStatus::Dismissed,
            timer: None,
            hovering_devices: BTreeSet::new(),
            overlay_shown: false,
            announcements: Vec::new(),
            now_ms: 0.0,
            controller_built: false,
        }
    }

    pub fn status(&self) -> TooltipAnimationStatus {
        self.status
    }

    pub fn timer(&self) -> Option<TooltipTimer> {
        self.timer
    }

    pub fn is_showing(&self) -> bool {
        self.overlay_shown
    }

    pub fn hovering_devices(&self) -> usize {
        self.hovering_devices.len()
    }

    pub fn controller_built(&self) -> bool {
        self.controller_built
    }

    /// What has been announced to a screen reader, in order. Upstream calls
    /// `SemanticsService.tooltip` at the moment the overlay goes up.
    pub fn announcements(&self) -> &[String] {
        &self.announcements
    }

    /// Upstream `_handleStatusChanged`, which switches on the pair `(was
    /// dismissed, is dismissed)`. Two of the four cases are deliberately
    /// nothing: the overlay is put up and taken down on the *edges* only.
    fn set_status(&mut self, status: TooltipAnimationStatus) {
        match (self.status.is_dismissed(), status.is_dismissed()) {
            (false, true) => self.overlay_shown = false,
            (true, false) => {
                self.overlay_shown = true;
                self.announcements
                    .push(self.widget.semantics_tooltip.clone().unwrap_or_default());
            }
            _ => {}
        }
        self.status = status;
    }

    fn forward(&mut self) {
        self.controller_built = true;
        self.set_status(TooltipAnimationStatus::Forward);
    }

    fn reverse(&mut self) {
        if !self.controller_built {
            return;
        }
        self.set_status(TooltipAnimationStatus::Reverse);
    }

    /// Lets a test settle an in-flight animation.
    pub fn finish_animation(&mut self) {
        match self.status {
            TooltipAnimationStatus::Forward => self.set_status(TooltipAnimationStatus::Completed),
            TooltipAnimationStatus::Reverse => self.set_status(TooltipAnimationStatus::Dismissed),
            _ => {}
        }
    }

    /// Upstream `_scheduleShowTooltip`.
    ///
    /// The `else` branch carries the interesting rule: if the tooltip is
    /// already animating in or fully visible, the delay is **skipped and it
    /// shows at once**. A delay is there to keep a tooltip from appearing while
    /// the pointer is merely passing over; once it is up, there is nothing left
    /// to wait for.
    pub fn schedule_show(&mut self, with_delay_ms: f32, touch_delay_ms: Option<f32>) {
        debug_assert!(
            self.timer.is_none() || self.status != TooltipAnimationStatus::Reverse,
            "timer must not be active when the tooltip is animating out"
        );
        if self.status.is_dismissed() && with_delay_ms > 0.0 {
            self.timer = Some(TooltipTimer::Show {
                at_ms: self.now_ms + with_delay_ms,
                touch_delay_ms,
            });
            return;
        }
        self.show_now(touch_delay_ms);
    }

    fn show_now(&mut self, touch_delay_ms: Option<f32>) {
        self.forward();
        self.timer = touch_delay_ms.map(|delay| TooltipTimer::AutoHide {
            at_ms: self.now_ms + delay,
        });
    }

    /// Upstream `_scheduleDismissTooltip`.
    pub fn schedule_dismiss(&mut self, with_delay_ms: f32) {
        debug_assert!(
            self.timer.is_none() || self.status != TooltipAnimationStatus::Reverse,
            "timer must not be active when the tooltip is animating out"
        );
        self.timer = None;
        // Read the backing controller, not the lazy getter: asking whether a
        // tooltip is showing must not build the machinery for one that never
        // has been.
        if !self.controller_built || !self.status.is_forward_or_completed() {
            return;
        }
        if with_delay_ms > 0.0 {
            // Dismissing while it is still animating in: the animation is
            // allowed to finish arriving before the delay fires.
            self.timer = Some(TooltipTimer::Dismiss {
                at_ms: self.now_ms + with_delay_ms,
            });
        } else {
            self.reverse();
        }
    }

    /// Advances the clock, firing the timer if it is due.
    pub fn advance_ms(&mut self, delta: f32) {
        self.now_ms += delta;
        let Some(timer) = self.timer else {
            return;
        };
        if timer.at_ms() > self.now_ms {
            return;
        }
        self.timer = None;
        match timer {
            TooltipTimer::Show { touch_delay_ms, .. } => self.show_now(touch_delay_ms),
            TooltipTimer::AutoHide { .. } | TooltipTimer::Dismiss { .. } => self.reverse(),
        }
    }

    pub fn now_ms(&self) -> f32 {
        self.now_ms
    }

    /// Upstream `_handleTapToDismiss`, reached when a pointer turns out not to
    /// be part of a trigger gesture -- including a pointer down anywhere else in
    /// the application, which arrives through a **global** pointer route.
    ///
    /// Upstream's reason for the global route is written down: global routes
    /// are dispatched *after* other routes, so the tooltip hears about a click
    /// on some other control without having taken the click away from it.
    pub fn handle_tap_to_dismiss(&mut self) {
        if !self.widget.enable_tap_to_dismiss {
            return;
        }
        self.schedule_dismiss(0.0);
        // The hovering devices are forgotten too: a click elsewhere ends the
        // tooltip even if the mouse never moved off it.
        self.hovering_devices.clear();
    }

    /// Upstream `_handleTap`.
    pub fn handle_tap(&mut self) -> bool {
        let created = self.status.is_dismissed();
        let feedback = created && self.widget.enable_feedback;
        self.schedule_show(
            0.0,
            // A mouse resting on it keeps it up, so no self-destruct timer is
            // set: the auto-hide is for a finger that has nothing left to say.
            if self.hovering_devices.is_empty() {
                Some(self.widget.touch_delay_ms)
            } else {
                None
            },
        );
        feedback
    }

    /// Upstream `_handleLongPress`. No touch delay: the tooltip lives as long
    /// as the press does, and [`RawTooltipState::handle_press_up`] ends it.
    pub fn handle_long_press(&mut self) -> bool {
        let created = self.status.is_dismissed();
        let feedback = created && self.widget.enable_feedback;
        self.schedule_show(0.0, None);
        feedback
    }

    /// Upstream `_handlePressUp`, which does nothing at all while a mouse is
    /// still hovering -- lifting a finger should not close a tooltip somebody
    /// else is holding open.
    pub fn handle_press_up(&mut self) {
        if !self.hovering_devices.is_empty() {
            return;
        }
        self.schedule_dismiss(self.widget.touch_delay_ms);
    }

    /// Upstream `_handleMouseExit`. The tooltip goes when the **last** device
    /// leaves, after the dismiss delay.
    pub fn handle_mouse_exit(&mut self, device: i32) {
        if self.hovering_devices.is_empty() {
            return;
        }
        self.hovering_devices.remove(&device);
        if self.hovering_devices.is_empty() {
            self.schedule_dismiss(self.widget.dismiss_delay_ms);
        }
    }

    /// Upstream `ensureTooltipVisible`.
    ///
    /// It cancels the timer and does not set a new one, so a tooltip shown this
    /// way **stays** until something dismisses it. Returns false when it was
    /// already on its way in or up.
    pub fn ensure_tooltip_visible(&mut self) -> bool {
        self.timer = None;
        if self.controller_built && self.status.is_forward_or_completed() {
            return false;
        }
        self.schedule_show(0.0, None);
        true
    }
}

/// Upstream's static `RawTooltip._openedTooltips` set and the cross-tooltip
/// rules that need it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TooltipScope {
    tooltips: Vec<RawTooltipState>,
}

impl TooltipScope {
    pub fn new() -> TooltipScope {
        TooltipScope::default()
    }

    pub fn add(&mut self, state: RawTooltipState) {
        self.tooltips.push(state);
    }

    pub fn get(&self, id: u64) -> Option<&RawTooltipState> {
        self.tooltips.iter().find(|state| state.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut RawTooltipState> {
        self.tooltips.iter_mut().find(|state| state.id == id)
    }

    /// The tooltips currently up, in the order they opened.
    pub fn opened(&self) -> Vec<u64> {
        self.tooltips
            .iter()
            .filter(|state| state.is_showing())
            .map(|state| state.id)
            .collect()
    }

    pub fn advance_ms(&mut self, delta: f32) {
        for state in &mut self.tooltips {
            state.advance_ms(delta);
        }
    }

    pub fn finish_animations(&mut self) {
        for state in &mut self.tooltips {
            state.finish_animation();
        }
    }

    /// Upstream `_handleMouseEnter`, and the one line in this file most worth
    /// keeping:
    ///
    /// ```dart
    /// _scheduleShowTooltip(withDelay: tooltipsToDismiss.isNotEmpty ? Duration.zero : widget.hoverDelay);
    /// ```
    ///
    /// **Moving from one tooltip to the next skips the delay.** The delay is
    /// there to stop a tooltip appearing at a pointer merely passing through;
    /// a reader who has just read one has already shown they are reading
    /// tooltips, and making them wait again for the next would be answering a
    /// question they have stopped asking.
    ///
    /// The tooltips dismissed are only those no mouse is still hovering, and
    /// upstream notes why that is safe to check here: the mouse tracker
    /// dispatches every `onExit` before any `onEnter`, so a device that has
    /// left somewhere else has already been removed from its set.
    pub fn handle_mouse_enter(&mut self, id: u64, device: i32) -> Vec<u64> {
        if let Some(state) = self.get_mut(id) {
            state.hovering_devices.insert(device);
        }
        let to_dismiss: Vec<u64> = self
            .tooltips
            .iter()
            .filter(|state| {
                state.id != id && state.is_showing() && state.hovering_devices.is_empty()
            })
            .map(|state| state.id)
            .collect();
        for other in &to_dismiss {
            if let Some(state) = self.get_mut(*other) {
                state.schedule_dismiss(0.0);
            }
        }
        let skip_delay = !to_dismiss.is_empty();
        if let Some(state) = self.get_mut(id) {
            let delay = if skip_delay {
                0.0
            } else {
                state.widget.hover_delay_ms
            };
            state.schedule_show(delay, None);
        }
        to_dismiss
    }

    /// Upstream `RawTooltip.dismissAllToolTips`, which dismisses even the ones
    /// still being hovered.
    pub fn dismiss_all(&mut self) -> bool {
        let mut any = false;
        for state in &mut self.tooltips {
            if state.is_showing() {
                state.schedule_dismiss(0.0);
                any = true;
            }
        }
        any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> RawTooltipState {
        RawTooltipState::new(1, RawTooltip::new())
    }

    /// Shows a tooltip and settles its animation.
    fn shown() -> RawTooltipState {
        let mut state = state();
        state.ensure_tooltip_visible();
        state.finish_animation();
        state
    }

    // -- The defaults are the design ------------------------------------------

    #[test]
    fn a_mouse_that_has_come_to_rest_has_already_waited() {
        // Which is why the hover delay is zero and the touch one is not: a
        // finger that lifted has no way to say "still looking", so its tooltip
        // has to end by itself.
        let widget = RawTooltip::new();
        assert_eq!(widget.hover_delay_ms, 0.0);
        assert_eq!(widget.touch_delay_ms, 1500.0);
        assert_eq!(widget.dismiss_delay_ms, 100.0);
        assert_eq!(widget.trigger_mode, TooltipTriggerMode::LongPress);
    }

    // -- The status edges ------------------------------------------------------

    #[test]
    fn the_overlay_goes_up_and_down_on_the_edges_only() {
        // Upstream switches on the pair (was dismissed, is dismissed) and two
        // of the four cases are deliberately nothing.
        let mut state = state();
        assert!(!state.is_showing());

        state.ensure_tooltip_visible();
        assert!(state.is_showing(), "up the moment it starts arriving");
        assert_eq!(state.announcements().len(), 1);

        state.finish_animation();
        assert!(state.is_showing());
        assert_eq!(
            state.announcements().len(),
            1,
            "arriving fully is not a second arrival"
        );

        state.schedule_dismiss(0.0);
        assert!(
            state.is_showing(),
            "still up while it animates out -- it is still on screen"
        );

        state.finish_animation();
        assert!(!state.is_showing());
    }

    #[test]
    fn asking_a_tooltip_to_go_away_does_not_build_the_machinery_to_show_it() {
        // Upstream reads the backing controller rather than the lazy getter,
        // and says so.
        let mut state = state();
        state.schedule_dismiss(0.0);
        assert!(!state.controller_built());

        state.ensure_tooltip_visible();
        assert!(state.controller_built());
    }

    // -- The delays -------------------------------------------------------------

    #[test]
    fn a_hover_delay_holds_the_tooltip_back_until_it_elapses() {
        let mut state = RawTooltipState::new(1, RawTooltip::new().with_hover_delay_ms(500.0));
        state.schedule_show(500.0, None);
        assert!(!state.is_showing());
        assert!(matches!(state.timer(), Some(TooltipTimer::Show { .. })));

        state.advance_ms(499.0);
        assert!(!state.is_showing());

        state.advance_ms(1.0);
        assert!(state.is_showing());
        assert_eq!(state.timer(), None);
    }

    #[test]
    fn a_tooltip_already_on_its_way_in_skips_the_delay_entirely() {
        // A delay is there to keep a tooltip from appearing at a pointer merely
        // passing over. Once it is up there is nothing left to wait for.
        let mut state = state();
        state.ensure_tooltip_visible();
        assert!(state.is_showing());

        state.schedule_show(5000.0, None);
        assert_eq!(state.timer(), None, "shown at once rather than rescheduled");
        assert!(state.is_showing());
    }

    #[test]
    fn a_deferred_show_still_gets_the_self_destruct_timer_it_was_promised() {
        // Upstream's show closure captures the touch delay it was scheduled
        // with; dropping it would leave a tapped tooltip up forever.
        let mut state = state();
        state.schedule_show(200.0, Some(1500.0));
        state.advance_ms(200.0);
        assert!(state.is_showing());
        assert!(matches!(state.timer(), Some(TooltipTimer::AutoHide { .. })));

        state.advance_ms(1500.0);
        state.finish_animation();
        assert!(!state.is_showing());
    }

    #[test]
    fn a_dismiss_delay_lets_the_arrival_finish_before_the_departure_starts() {
        let mut state = shown();
        state.schedule_dismiss(100.0);
        assert!(matches!(state.timer(), Some(TooltipTimer::Dismiss { .. })));
        assert_eq!(state.status(), TooltipAnimationStatus::Completed);

        state.advance_ms(100.0);
        assert_eq!(state.status(), TooltipAnimationStatus::Reverse);
    }

    #[test]
    fn dismissing_a_tooltip_that_is_not_up_does_nothing_at_all() {
        let mut state = state();
        state.schedule_dismiss(100.0);
        assert_eq!(state.timer(), None, "not even a timer");
        assert_eq!(state.status(), TooltipAnimationStatus::Dismissed);
    }

    // -- Touch -------------------------------------------------------------------

    #[test]
    fn a_tap_sets_a_self_destruct_timer_and_a_long_press_does_not() {
        // The press is the timer: the tooltip lives as long as the finger is
        // down.
        let mut tapped = state();
        tapped.handle_tap();
        assert!(matches!(
            tapped.timer(),
            Some(TooltipTimer::AutoHide { .. })
        ));

        let mut pressed = state();
        pressed.handle_long_press();
        assert_eq!(pressed.timer(), None);
    }

    #[test]
    fn a_mouse_resting_on_it_cancels_the_taps_self_destruct() {
        let mut scope = TooltipScope::new();
        scope.add(state());
        scope.handle_mouse_enter(1, 7);

        let state = scope.get_mut(1).unwrap();
        state.handle_tap();
        assert_eq!(
            state.timer(),
            None,
            "the mouse will say when it is finished"
        );
    }

    #[test]
    fn lifting_a_finger_does_not_close_a_tooltip_a_mouse_is_holding_open() {
        let mut scope = TooltipScope::new();
        scope.add(state());
        scope.handle_mouse_enter(1, 7);
        let state = scope.get_mut(1).unwrap();
        state.handle_long_press();
        state.handle_press_up();
        assert_eq!(state.timer(), None);
        assert_eq!(state.status(), TooltipAnimationStatus::Forward);
    }

    #[test]
    fn feedback_is_played_only_when_the_tooltip_is_actually_new() {
        let mut state = state();
        assert!(state.handle_long_press(), "the first press made one");
        assert!(!state.handle_long_press(), "the second found it already up");
    }

    #[test]
    fn a_click_elsewhere_forgets_the_hovering_mice_as_well() {
        // Otherwise the tooltip would be dismissed and immediately kept alive
        // by a mouse that never moved.
        let mut scope = TooltipScope::new();
        scope.add(state());
        scope.handle_mouse_enter(1, 7);
        let state = scope.get_mut(1).unwrap();
        assert_eq!(state.hovering_devices(), 1);

        state.handle_tap_to_dismiss();
        assert_eq!(state.hovering_devices(), 0);
        assert_eq!(state.status(), TooltipAnimationStatus::Reverse);
    }

    #[test]
    fn a_tooltip_that_refuses_tap_to_dismiss_stays_put() {
        let mut widget = RawTooltip::new();
        widget.enable_tap_to_dismiss = false;
        let mut state = RawTooltipState::new(1, widget);
        state.ensure_tooltip_visible();
        state.handle_tap_to_dismiss();
        assert_eq!(state.status(), TooltipAnimationStatus::Forward);
    }

    // -- Mice --------------------------------------------------------------------

    #[test]
    fn the_tooltip_goes_when_the_last_mouse_leaves_not_the_first() {
        // Two mice can hover the same thing, which is why it is a set of device
        // ids rather than a flag.
        let mut scope = TooltipScope::new();
        scope.add(state());
        scope.handle_mouse_enter(1, 7);
        scope.handle_mouse_enter(1, 8);
        scope.finish_animations();

        let state = scope.get_mut(1).unwrap();
        assert_eq!(state.hovering_devices(), 2);

        state.handle_mouse_exit(7);
        assert_eq!(state.timer(), None, "one is still there");

        state.handle_mouse_exit(8);
        assert!(matches!(state.timer(), Some(TooltipTimer::Dismiss { .. })));
    }

    #[test]
    fn an_exit_from_a_device_that_was_never_here_is_ignored() {
        let mut state = shown();
        state.handle_mouse_exit(7);
        assert_eq!(state.timer(), None);
    }

    // -- The rule this file is worth reading for -----------------------------------

    #[test]
    fn moving_from_one_tooltip_to_the_next_skips_the_delay() {
        // A reader who has just read one tooltip has shown they are reading
        // tooltips; making them wait again would be answering a question they
        // have stopped asking.
        let slow = || RawTooltip::new().with_hover_delay_ms(500.0);
        let mut scope = TooltipScope::new();
        scope.add(RawTooltipState::new(1, slow()));
        scope.add(RawTooltipState::new(2, slow()));

        scope.handle_mouse_enter(1, 7);
        assert!(!scope.get(1).unwrap().is_showing(), "the first one waits");
        scope.advance_ms(500.0);
        scope.finish_animations();
        assert!(scope.get(1).unwrap().is_showing());

        // The mouse leaves the first and arrives at the second. Exits are
        // dispatched before enters, which is what makes the check below sound.
        scope.get_mut(1).unwrap().handle_mouse_exit(7);
        scope.advance_ms(100.0);

        let dismissed = scope.handle_mouse_enter(2, 7);
        assert_eq!(dismissed, [1], "the first is taken down at once");
        assert!(
            scope.get(2).unwrap().is_showing(),
            "and the second appears with no wait at all"
        );
    }

    #[test]
    fn the_first_tooltip_of_a_session_still_waits_its_delay() {
        // Without this the rule above would be free, and would prove nothing.
        let mut scope = TooltipScope::new();
        scope.add(RawTooltipState::new(
            1,
            RawTooltip::new().with_hover_delay_ms(500.0),
        ));
        assert_eq!(scope.handle_mouse_enter(1, 7), Vec::<u64>::new());
        assert!(!scope.get(1).unwrap().is_showing());
    }

    #[test]
    fn a_tooltip_another_mouse_is_still_hovering_is_left_alone() {
        let mut scope = TooltipScope::new();
        scope.add(state());
        scope.add(state_with_id(2));
        scope.handle_mouse_enter(1, 7);
        scope.finish_animations();

        let dismissed = scope.handle_mouse_enter(2, 8);
        assert!(
            dismissed.is_empty(),
            "device 7 is still on the first one, so it stays"
        );
        assert_eq!(scope.opened().len(), 2);
    }

    fn state_with_id(id: u64) -> RawTooltipState {
        RawTooltipState::new(id, RawTooltip::new())
    }

    #[test]
    fn dismiss_all_takes_down_even_the_ones_still_being_hovered() {
        let mut scope = TooltipScope::new();
        scope.add(state());
        scope.add(state_with_id(2));
        scope.handle_mouse_enter(1, 7);
        scope.handle_mouse_enter(2, 8);
        scope.finish_animations();
        assert_eq!(scope.opened().len(), 2);

        assert!(scope.dismiss_all());
        scope.finish_animations();
        assert!(scope.opened().is_empty());
        assert!(!scope.dismiss_all(), "and there is nothing left to do");
    }

    // -- ensureTooltipVisible --------------------------------------------------------

    #[test]
    fn a_tooltip_shown_on_purpose_stays_until_something_dismisses_it() {
        // It cancels the timer and sets no new one.
        let mut state = state();
        state.schedule_show(500.0, Some(1500.0));
        assert!(state.timer().is_some());

        assert!(state.ensure_tooltip_visible());
        assert_eq!(state.timer(), None);
        assert!(state.is_showing());

        state.advance_ms(100_000.0);
        assert!(state.is_showing(), "and it is still there much later");
    }

    #[test]
    fn ensuring_a_visible_tooltip_is_visible_returns_false() {
        let mut state = shown();
        assert!(!state.ensure_tooltip_visible());
    }

    // -- Nested tooltips ---------------------------------------------------------------

    #[test]
    fn only_the_innermost_of_nested_tooltips_hears_the_mouse() {
        // A chip with a delete icon should not show both its own tooltip and
        // the icon's.
        assert_eq!(ExclusiveMouseRegion::hit(&[12, 11, 10]), Some(12));
        assert_eq!(ExclusiveMouseRegion::hit(&[]), None);
    }

    // -- Position --------------------------------------------------------------------

    #[test]
    fn a_tooltip_goes_below_when_it_fits_and_above_when_it_does_not() {
        let context = TooltipPositionContext::new((200.0, 100.0), (40.0, 40.0), (120.0, 30.0))
            .with_overlay((400.0, 800.0))
            .with_vertical_offset(24.0);
        assert_eq!(position_dependent_box(&context).1, 124.0, "below");

        let cramped = context.with_overlay((400.0, 140.0));
        assert_eq!(position_dependent_box(&cramped).1, 46.0, "above instead");
    }

    #[test]
    fn a_tooltip_is_pushed_back_on_screen_rather_than_left_hanging_off_it() {
        // One not quite where it was asked to be beats one you cannot read.
        let at_the_edge = TooltipPositionContext::new((10.0, 400.0), (20.0, 20.0), (120.0, 30.0))
            .with_overlay((400.0, 800.0));
        assert_eq!(position_dependent_box(&at_the_edge).0, 10.0);

        let far_side = TooltipPositionContext::new((395.0, 400.0), (20.0, 20.0), (120.0, 30.0))
            .with_overlay((400.0, 800.0));
        assert_eq!(position_dependent_box(&far_side).0, 270.0);

        let centred = TooltipPositionContext::new((200.0, 400.0), (20.0, 20.0), (120.0, 30.0))
            .with_overlay((400.0, 800.0));
        assert_eq!(
            position_dependent_box(&centred).0,
            140.0,
            "left where asked"
        );
    }

    #[test]
    fn a_tooltip_wider_than_the_screen_overflows_evenly_on_both_sides() {
        let huge = TooltipPositionContext::new((200.0, 400.0), (20.0, 20.0), (600.0, 30.0))
            .with_overlay((400.0, 800.0));
        assert_eq!(position_dependent_box(&huge).0, -100.0);
    }

    #[test]
    fn a_position_context_compares_by_all_of_its_parts() {
        let a = TooltipPositionContext::new((0.0, 0.0), (10.0, 10.0), (20.0, 20.0));
        assert_eq!(a, a);
        assert_ne!(a, a.with_vertical_offset(4.0));
        assert_ne!(a, a.with_prefer_below(false));
        assert_ne!(a, a.with_overlay((100.0, 100.0)));
    }
}
