//! Snapshotting a subtree -- a port of upstream's `widgets/snapshot_widget.dart`.
//!
//! Some effects are expensive per frame and cheap per pixel: a scale, a skew,
//! a blur. Applied to a complex subtree they cost a full repaint every frame;
//! applied to a **picture of that subtree** they cost one rasterisation and
//! then nothing. That trade is the whole widget.
//!
//! It is deliberately for **short** animations, and the reason is the same
//! trade read backwards: a snapshot is frozen, so anything animating inside
//! the child stops. Upstream's own example is Android Q's zoom page
//! transition, which lasts a few hundred milliseconds -- long enough to save
//! real work, short enough that nobody notices the child is a photograph.
//!
//! ## What is not here
//!
//! The rasterisation itself, the `ui.Image` it produces and the render
//! object that swaps one for the other are engine-side. What is ported is the
//! controller, the three modes and what each does about a platform view, and
//! the painter's contract.

/// Upstream `SnapshotMode`: what to do when the child **cannot** be
/// snapshotted.
///
/// The case that forces the question is a platform view -- a native map or web
/// view composited by the engine rather than drawn into Flutter's own layer.
/// It is not in the picture, so a snapshot of the subtree simply does not
/// contain it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapshotMode {
    /// Snapshot when possible, and fall back to painting the live child when
    /// not. The effect still runs -- through
    /// [`SnapshotPainter::paint`] rather than
    /// [`SnapshotPainter::paint_snapshot`] -- so the reader sees a correct if
    /// more expensive frame.
    Permissive,
    /// **The default**, and it throws. That reads harsh for a performance
    /// optimisation until the alternative is considered: silently not
    /// snapshotting means the expensive path runs and nobody finds out, which
    /// is exactly the bug the widget was added to prevent.
    #[default]
    Normal,
    /// Snapshot anyway and let the platform view fall out of the picture.
    /// Useful when the caller knows the view is behind something, or outside
    /// the part being animated.
    Forced,
}

impl SnapshotMode {
    /// What happens when a platform view is found in the subtree.
    pub fn on_platform_view(self) -> PlatformViewOutcome {
        match self {
            SnapshotMode::Permissive => PlatformViewOutcome::PaintChildLive,
            SnapshotMode::Normal => PlatformViewOutcome::Error,
            SnapshotMode::Forced => PlatformViewOutcome::SnapshotWithoutIt,
        }
    }
}

/// The three answers to "there is a platform view in here".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformViewOutcome {
    PaintChildLive,
    Error,
    SnapshotWithoutIt,
}

/// Upstream `SnapshotController`.
///
/// Two things, and the asymmetry between them is the point. `allowSnapshotting`
/// is a **value** that notifies on change; `clear()` is an **event** that
/// notifies unconditionally. Turning snapshotting on twice should do nothing;
/// asking for a fresh snapshot twice should produce two fresh snapshots,
/// because the child may have changed both times.
#[derive(Debug, Default)]
pub struct SnapshotController {
    allow_snapshotting: bool,
    notifications: usize,
}

impl SnapshotController {
    /// Upstream's default is **false**: a widget that snapshotted from the
    /// moment it was built would freeze its child for the whole of its life,
    /// and the caller wants it only for the length of an animation.
    pub fn new(allow_snapshotting: bool) -> SnapshotController {
        SnapshotController {
            allow_snapshotting,
            notifications: 0,
        }
    }

    pub fn allow_snapshotting(&self) -> bool {
        self.allow_snapshotting
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's setter, which returns early on the same value.
    pub fn set_allow_snapshotting(&mut self, value: bool) {
        if value == self.allow_snapshotting {
            return;
        }
        self.allow_snapshotting = value;
        self.notifications += 1;
    }

    /// Upstream's `clear`, which **notifies unconditionally** -- there is no
    /// value to compare, and the caller is saying the child changed.
    ///
    /// Its doc says it "has no effect if allowSnapshotting is false", which is
    /// true of the *outcome* rather than of the call: the notification still
    /// goes out, and a listener not snapshotting has nothing to discard.
    pub fn clear(&mut self) {
        self.notifications += 1;
    }
}

/// Upstream `SnapshotPainter`: what to do with the picture once there is one.
///
/// Two paint methods rather than one, because the widget has two states and
/// the effect has to be applied in both. `paint_snapshot` gets an image;
/// `paint` gets a callback that paints the live child. **A painter that
/// implements only the first would lose its effect entirely** whenever
/// snapshotting was off -- which includes every frame before the animation
/// starts.
pub trait SnapshotPainter {
    /// Upstream's `paintSnapshot`.
    ///
    /// It takes both a `size` and a `sourceSize`, and the difference is the
    /// trap upstream spends a paragraph on. The image is rasterised at
    /// **physical** pixels, so its width is the widget's width times the pixel
    /// ratio -- but `image.width` is that number **rounded to an integer**,
    /// and drawing from a source rect of that size samples slightly outside
    /// what was captured. `sourceSize` is the unrounded truth, and it is what
    /// the source rectangle must use.
    fn paint_snapshot(
        &self,
        size: (f32, f32),
        source_size: (f32, f32),
        pixel_ratio: f32,
    ) -> SnapshotDraw;

    /// Upstream's `paint`, for when snapshotting is off or a permissive mode
    /// met a platform view.
    fn paint(&self, size: (f32, f32)) -> LiveDraw;

    /// Upstream's `shouldRepaint`, and its doc is careful in both directions:
    /// `paint` **may be called anyway** when this returns false (an ancestor
    /// repainted), and may be called **without this being asked at all** (the
    /// box changed size). It is a permission to skip, not a promise.
    ///
    /// It also says the thing a caller most often gets wrong: **changing the
    /// painter does not refresh the snapshot.** The image is the controller's,
    /// not the painter's, and only `SnapshotController.clear` replaces it.
    fn should_repaint(&self, old: &Self) -> bool
    where
        Self: Sized;
}

/// What [`SnapshotPainter::paint_snapshot`] worked out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapshotDraw {
    /// The source rectangle, in the image's own pixels.
    pub src: (f32, f32, f32, f32),
    /// The destination rectangle, in logical pixels.
    pub dst: (f32, f32, f32, f32),
}

/// What [`SnapshotPainter::paint`] worked out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveDraw {
    pub size: (f32, f32),
}

/// Upstream's `_DefaultSnapshotPainter`: draws the image where the child was,
/// and paints the child unchanged when it cannot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultSnapshotPainter;

impl SnapshotPainter for DefaultSnapshotPainter {
    fn paint_snapshot(
        &self,
        size: (f32, f32),
        source_size: (f32, f32),
        _pixel_ratio: f32,
    ) -> SnapshotDraw {
        SnapshotDraw {
            // Upstream: `Rect.fromLTWH(0, 0, sourceSize.width,
            // sourceSize.height)` -- the *unrounded* captured size, not
            // `image.width`.
            src: (0.0, 0.0, source_size.0, source_size.1),
            dst: (0.0, 0.0, size.0, size.1),
        }
    }

    fn paint(&self, size: (f32, f32)) -> LiveDraw {
        LiveDraw { size }
    }

    /// The default painter has no state, so nothing about it can change.
    fn should_repaint(&self, _old: &DefaultSnapshotPainter) -> bool {
        false
    }
}

/// Upstream `SnapshotWidget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotWidget {
    pub mode: SnapshotMode,
    /// Upstream's `autoresize`, **false** by default.
    ///
    /// The default is the interesting choice: a resize while snapshotting
    /// would otherwise stretch the old picture rather than re-rasterise. That
    /// is usually right, because the common case is a scale animation, where
    /// stretching the picture **is the effect** -- re-rasterising every frame
    /// would give back exactly the cost the widget was there to avoid.
    pub autoresize: bool,
}

impl Default for SnapshotWidget {
    fn default() -> SnapshotWidget {
        SnapshotWidget::new()
    }
}

impl SnapshotWidget {
    pub fn new() -> SnapshotWidget {
        SnapshotWidget {
            mode: SnapshotMode::Normal,
            autoresize: false,
        }
    }

    pub fn with_mode(mut self, mode: SnapshotMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_autoresize(mut self, autoresize: bool) -> Self {
        self.autoresize = autoresize;
        self
    }

    /// Whether a size change discards the snapshot.
    pub fn resize_invalidates_snapshot(&self) -> bool {
        self.autoresize
    }

    /// Which of the painter's two methods a frame uses.
    pub fn paints_from_snapshot(
        &self,
        controller: &SnapshotController,
        has_platform_view: bool,
    ) -> bool {
        if !controller.allow_snapshotting() {
            return false;
        }
        if !has_platform_view {
            return true;
        }
        self.mode.on_platform_view() != PlatformViewOutcome::PaintChildLive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The three modes ---------------------------------------------------

    #[test]
    fn the_default_mode_throws_rather_than_silently_not_snapshotting() {
        // Silently skipping means the expensive path runs and nobody finds
        // out, which is exactly the bug the widget was added to prevent.
        assert_eq!(SnapshotMode::default(), SnapshotMode::Normal);
        assert_eq!(
            SnapshotMode::Normal.on_platform_view(),
            PlatformViewOutcome::Error
        );
    }

    #[test]
    fn permissive_falls_back_to_the_live_child_and_keeps_the_effect() {
        // The effect still runs, through paint rather than paintSnapshot, so
        // the reader sees a correct if more expensive frame.
        assert_eq!(
            SnapshotMode::Permissive.on_platform_view(),
            PlatformViewOutcome::PaintChildLive
        );
    }

    #[test]
    fn forced_snapshots_anyway_and_lets_the_platform_view_fall_out() {
        // Useful when the caller knows the view is behind something, or
        // outside the part being animated.
        assert_eq!(
            SnapshotMode::Forced.on_platform_view(),
            PlatformViewOutcome::SnapshotWithoutIt
        );
    }

    #[test]
    fn which_paint_method_a_frame_uses() {
        let widget = SnapshotWidget::new();
        let mut controller = SnapshotController::new(false);
        assert!(
            !widget.paints_from_snapshot(&controller, false),
            "snapshotting is off, so the live child"
        );

        controller.set_allow_snapshotting(true);
        assert!(widget.paints_from_snapshot(&controller, false));

        assert!(
            widget.paints_from_snapshot(&controller, true),
            "the normal mode would rather throw than paint live"
        );
        assert!(
            !SnapshotWidget::new()
                .with_mode(SnapshotMode::Permissive)
                .paints_from_snapshot(&controller, true),
            "where permissive steps back to the live child"
        );
        assert!(
            SnapshotWidget::new()
                .with_mode(SnapshotMode::Forced)
                .paints_from_snapshot(&controller, true)
        );
    }

    // -- The controller ----------------------------------------------------

    #[test]
    fn snapshotting_is_off_until_it_is_asked_for() {
        // A widget snapshotting from the moment it was built would freeze its
        // child for the whole of its life.
        assert!(!SnapshotController::new(false).allow_snapshotting());
    }

    #[test]
    fn turning_snapshotting_on_twice_says_nothing_the_second_time() {
        let mut controller = SnapshotController::new(false);
        controller.set_allow_snapshotting(true);
        assert_eq!(controller.notifications(), 1);

        controller.set_allow_snapshotting(true);
        assert_eq!(controller.notifications(), 1);

        controller.set_allow_snapshotting(false);
        assert_eq!(controller.notifications(), 2);
    }

    #[test]
    fn asking_for_a_fresh_snapshot_twice_produces_two_notifications() {
        // Unlike the flag, clear is an event: the child may have changed both
        // times, and there is no value to compare against.
        let mut controller = SnapshotController::new(true);
        controller.clear();
        controller.clear();
        assert_eq!(controller.notifications(), 2);
    }

    #[test]
    fn clear_still_notifies_while_snapshotting_is_off() {
        // Its doc's "has no effect" is about the outcome, not the call: a
        // listener that is not snapshotting has nothing to discard.
        let mut controller = SnapshotController::new(false);
        controller.clear();
        assert_eq!(controller.notifications(), 1);
    }

    // -- The painter -------------------------------------------------------

    #[test]
    fn the_source_rectangle_uses_the_unrounded_captured_size() {
        // image.width is the physical width rounded to an integer, and drawing
        // from a source rect of that size samples slightly outside what was
        // captured.
        let painter = DefaultSnapshotPainter;
        let draw = painter.paint_snapshot((100.0, 50.0), (275.5, 137.75), 2.755);
        assert_eq!(
            draw.src,
            (0.0, 0.0, 275.5, 137.75),
            "the truth, not the rounded 276"
        );
        assert_eq!(
            draw.dst,
            (0.0, 0.0, 100.0, 50.0),
            "and the destination is in logical pixels"
        );
    }

    #[test]
    fn the_live_path_paints_the_child_at_its_own_size() {
        let painter = DefaultSnapshotPainter;
        assert_eq!(
            painter.paint((100.0, 50.0)),
            LiveDraw {
                size: (100.0, 50.0)
            }
        );
    }

    #[test]
    fn a_painter_with_no_state_has_nothing_that_can_change() {
        assert!(!DefaultSnapshotPainter.should_repaint(&DefaultSnapshotPainter));
    }

    // -- The widget --------------------------------------------------------

    #[test]
    fn a_resize_stretches_the_old_picture_unless_autoresize_was_asked_for() {
        // Which is usually right: the common case is a scale animation, where
        // stretching the picture *is* the effect, and re-rasterising every
        // frame gives back exactly the cost the widget was avoiding.
        assert!(!SnapshotWidget::new().resize_invalidates_snapshot());
        assert!(
            SnapshotWidget::new()
                .with_autoresize(true)
                .resize_invalidates_snapshot()
        );
    }
}
