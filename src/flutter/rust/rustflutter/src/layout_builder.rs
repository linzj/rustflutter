//! Building during layout -- the two pieces of upstream's
//! `widgets/layout_builder.dart` that were missing.
//!
//! An ordinary widget is built before layout and therefore cannot know how
//! much room it has. A layout builder inverts that: its child is built
//! **inside** the parent's `performLayout`, with the constraints in hand.
//!
//! Everything awkward about it follows from that inversion. Building during
//! layout means marking something dirty during layout, which the framework
//! otherwise forbids; so the rebuild goes through a layout callback rather
//! than the ordinary build scope, and scheduling one has to be careful about
//! *when* it happens.

/// Upstream `ConstrainedLayoutBuilder`: a layout builder whose layout
/// information **is** the constraints.
///
/// It is a thin subclass of `AbstractLayoutBuilder`, and the split exists
/// because the general form can hand the builder anything layout knows --
/// upstream's own `SliverLayoutBuilder` passes `SliverConstraints`. Constraints
/// are only the common case, not the definition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConstrainedLayoutBuilder;

impl ConstrainedLayoutBuilder {
    /// Upstream's `updateShouldRebuild`, whose default is **true**: a new
    /// builder function is assumed to build something different, because
    /// there is no way to compare two closures.
    ///
    /// A subclass that knows better -- because its builder depends only on a
    /// field it can compare -- overrides it, and that is the only way to stop
    /// a layout builder rebuilding its subtree on every parent rebuild.
    pub fn update_should_rebuild(&self) -> bool {
        true
    }
}

/// When a rebuild was asked for, which decides whether it can be acted on now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulerPhaseForRebuild {
    Idle,
    TransientCallbacks,
    MidFrameMicrotasks,
    PersistentCallbacks,
    PostFrameCallbacks,
}

impl SchedulerPhaseForRebuild {
    /// Upstream's `deferMarkNeedsLayout`, and its comment is the reason:
    ///
    /// > the render tree should typically be kept clean during the
    /// > postFrameCallbacks and the idle phase, so the layout data can be
    /// > safely read.
    ///
    /// A layout builder's rebuild dirties the render object, and dirtying it
    /// between frames would leave anyone inspecting the tree -- a test, a
    /// gesture arena working out what was hit, the inspector -- reading a tree
    /// that is mid-change. So outside a frame the request is deferred to the
    /// start of the next one.
    pub fn defers_mark_needs_layout(self) -> bool {
        matches!(
            self,
            SchedulerPhaseForRebuild::Idle | SchedulerPhaseForRebuild::PostFrameCallbacks
        )
    }
}

/// Upstream `RenderAbstractLayoutBuilderMixin`: the render object's half.
///
/// It holds the callback that rebuilds the widget subtree, and calls it from
/// `performLayout`. Upstream's instruction on that is specific -- "as soon as
/// possible in the class's performLayout implementation, **before any layout
/// work is done**" -- because the work about to be done depends on the
/// children the callback is going to produce.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderAbstractLayoutBuilderMixin {
    /// Which callback is installed, by identity. Upstream stores the closure;
    /// what matters here is whether it changed.
    callback: Option<u64>,
    scheduled: bool,
    deferred: bool,
    invocations: usize,
}

impl RenderAbstractLayoutBuilderMixin {
    pub fn new() -> RenderAbstractLayoutBuilderMixin {
        RenderAbstractLayoutBuilderMixin::default()
    }

    pub fn callback(&self) -> Option<u64> {
        self.callback
    }

    pub fn is_scheduled(&self) -> bool {
        self.scheduled
    }

    pub fn is_deferred(&self) -> bool {
        self.deferred
    }

    pub fn invocations(&self) -> usize {
        self.invocations
    }

    /// Upstream's `_updateCallback`, which schedules **only when the callback
    /// actually changed**. The widget is rebuilt every time its parent is, and
    /// re-scheduling a layout for an identical callback would make every
    /// ancestor rebuild cost a layout pass.
    pub fn update_callback(&mut self, callback: u64) {
        if self.callback == Some(callback) {
            return;
        }
        self.callback = Some(callback);
        self.schedule_layout_callback(SchedulerPhaseForRebuild::PersistentCallbacks);
    }

    /// Upstream's `_scheduleRebuild`.
    ///
    /// The early return on an already-scheduled deferral matters: a burst of
    /// rebuild requests between frames should cost one layout at the start of
    /// the next frame, not one each.
    pub fn schedule_layout_callback(&mut self, phase: SchedulerPhaseForRebuild) {
        if self.deferred {
            return;
        }
        if phase.defers_mark_needs_layout() {
            self.deferred = true;
        } else {
            self.scheduled = true;
        }
    }

    /// The deferred request being honoured at the start of the next frame.
    pub fn flush_deferred(&mut self) {
        if self.deferred {
            self.deferred = false;
            self.scheduled = true;
        }
    }

    /// Upstream's `layoutCallback`, invoked from `performLayout`. It asserts
    /// the callback is there -- laying out a layout builder that was never
    /// given one is a framework bug, not a state to tolerate.
    pub fn layout_callback(&mut self) {
        debug_assert!(self.callback.is_some(), "no layout callback installed");
        self.scheduled = false;
        self.invocations += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_builder_is_assumed_to_build_something_different() {
        // There is no way to compare two closures, so the default is true and
        // a subclass that knows better overrides it.
        assert!(ConstrainedLayoutBuilder.update_should_rebuild());
    }

    #[test]
    fn a_rebuild_between_frames_waits_for_the_next_one() {
        // The render tree is kept clean during idle and post-frame so anyone
        // inspecting it -- a test, the gesture arena, the inspector -- is not
        // reading a tree mid-change.
        for phase in [
            SchedulerPhaseForRebuild::Idle,
            SchedulerPhaseForRebuild::PostFrameCallbacks,
        ] {
            assert!(phase.defers_mark_needs_layout(), "{phase:?}");
        }
    }

    #[test]
    fn a_rebuild_inside_a_frame_is_acted_on_at_once() {
        for phase in [
            SchedulerPhaseForRebuild::TransientCallbacks,
            SchedulerPhaseForRebuild::MidFrameMicrotasks,
            SchedulerPhaseForRebuild::PersistentCallbacks,
        ] {
            assert!(!phase.defers_mark_needs_layout(), "{phase:?}");
        }
    }

    #[test]
    fn a_burst_of_requests_between_frames_costs_one_layout() {
        let mut render = RenderAbstractLayoutBuilderMixin::new();
        render.schedule_layout_callback(SchedulerPhaseForRebuild::Idle);
        render.schedule_layout_callback(SchedulerPhaseForRebuild::Idle);
        render.schedule_layout_callback(SchedulerPhaseForRebuild::Idle);
        assert!(render.is_deferred());
        assert!(!render.is_scheduled(), "nothing yet");

        render.flush_deferred();
        assert!(render.is_scheduled());
        assert!(!render.is_deferred());
    }

    #[test]
    fn a_request_inside_a_frame_schedules_straight_away() {
        let mut render = RenderAbstractLayoutBuilderMixin::new();
        render.schedule_layout_callback(SchedulerPhaseForRebuild::PersistentCallbacks);
        assert!(render.is_scheduled());
        assert!(!render.is_deferred());
    }

    #[test]
    fn an_unchanged_callback_does_not_cost_a_layout_pass() {
        // The widget is rebuilt whenever its parent is, and re-scheduling for
        // an identical callback would make every ancestor rebuild cost one.
        let mut render = RenderAbstractLayoutBuilderMixin::new();
        render.update_callback(7);
        assert!(render.is_scheduled());
        assert_eq!(render.callback(), Some(7));

        render.layout_callback();
        assert!(!render.is_scheduled());

        render.update_callback(7);
        assert!(!render.is_scheduled(), "the same one again");

        render.update_callback(8);
        assert!(render.is_scheduled());
    }

    #[test]
    fn running_the_callback_clears_the_schedule_and_counts() {
        let mut render = RenderAbstractLayoutBuilderMixin::new();
        render.update_callback(7);
        render.layout_callback();
        assert_eq!(render.invocations(), 1);
        assert!(!render.is_scheduled());
    }

    #[test]
    #[should_panic(expected = "no layout callback installed")]
    fn laying_out_with_no_callback_is_a_framework_bug_rather_than_a_state() {
        RenderAbstractLayoutBuilderMixin::new().layout_callback();
    }

    #[test]
    fn flushing_with_nothing_deferred_changes_nothing() {
        let mut render = RenderAbstractLayoutBuilderMixin::new();
        render.flush_deferred();
        assert!(!render.is_scheduled());
        assert!(!render.is_deferred());
    }
}
