//! Ports of `material/progress_indicator.dart` and
//! `material/refresh_indicator.dart`.
//!
//! Telling the reader that something is happening. Two kinds: one that knows
//! how far along it is and one that does not, and a third that is really a
//! gesture with an indicator attached.

/// Upstream `ProgressIndicator`, the base of the two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressIndicator {
    /// `None` is **indeterminate**: something is happening and nobody knows how
    /// much of it is left. A number between zero and one is determinate.
    pub value: Option<f32>,
    /// Whether an animation controller was supplied to drive the indeterminate
    /// animation.
    pub has_controller: bool,
}

impl ProgressIndicator {
    /// Upstream's linear indeterminate period, and the comment above it is a
    /// citation: it is extracted from Android's own
    /// `progress_indeterminate_material.xml`, with the source URL given.
    pub const INDETERMINATE_LINEAR_MS: u32 = 1800;

    /// The circular one, and it is written as a **product** rather than a
    /// number: `1333 * 2222`, which is about forty-nine minutes.
    ///
    /// That is not a duration anybody intends to watch. It is a common
    /// multiple, chosen so the rotation and the stroke sweep -- which have
    /// different periods -- only come back into phase after a very long time.
    /// **The animation never visibly repeats.** Also extracted from Android,
    /// with its own URL.
    pub const INDETERMINATE_CIRCULAR_MS: u32 = 1333 * 2222;

    /// Below this progress the track gap is scaled down proportionally.
    ///
    /// At zero there is no bar, so a gap between the bar and the track would be
    /// a notch floating in nothing. Ramping it away is cheaper than special
    /// casing the empty state.
    pub const TRACK_GAP_RAMP_DOWN_THRESHOLD: f32 = 0.01;

    pub fn indeterminate() -> ProgressIndicator {
        ProgressIndicator {
            value: None,
            has_controller: false,
        }
    }

    pub fn determinate(value: f32) -> ProgressIndicator {
        ProgressIndicator {
            value: Some(value),
            has_controller: false,
        }
    }

    pub fn is_determinate(&self) -> bool {
        self.value.is_some()
    }

    /// Upstream's assertion, whose message says where the contradiction is:
    /// *"The 'value' property is for a determinate indicator with a specific
    /// progress, while the 'controller' is for controlling the animation of an
    /// indeterminate indicator."*
    ///
    /// Having both is asking for both kinds of indicator at once.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.value.is_some() && self.has_controller {
            return Err("A progress indicator cannot have both a value and a controller.");
        }
        Ok(())
    }

    /// How much of the track gap is drawn. Scaled proportionally below the
    /// threshold, full above it.
    pub fn track_gap_scale(&self) -> f32 {
        match self.value {
            None => 1.0,
            Some(value) if value >= ProgressIndicator::TRACK_GAP_RAMP_DOWN_THRESHOLD => 1.0,
            Some(value) => {
                (value / ProgressIndicator::TRACK_GAP_RAMP_DOWN_THRESHOLD).clamp(0.0, 1.0)
            }
        }
    }
}

/// Upstream `LinearProgressIndicator`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearProgressIndicator {
    pub base: ProgressIndicator,
    pub min_height: f32,
}

impl LinearProgressIndicator {
    pub const DEFAULT_MIN_HEIGHT: f32 = 4.0;

    pub fn new(base: ProgressIndicator) -> LinearProgressIndicator {
        LinearProgressIndicator {
            base,
            min_height: LinearProgressIndicator::DEFAULT_MIN_HEIGHT,
        }
    }

    pub fn period_ms(&self) -> u32 {
        ProgressIndicator::INDETERMINATE_LINEAR_MS
    }
}

/// Upstream `CircularProgressIndicator`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircularProgressIndicator {
    pub base: ProgressIndicator,
    pub stroke_width: f32,
}

impl CircularProgressIndicator {
    pub const DEFAULT_STROKE_WIDTH: f32 = 4.0;

    pub fn new(base: ProgressIndicator) -> CircularProgressIndicator {
        CircularProgressIndicator {
            base,
            stroke_width: CircularProgressIndicator::DEFAULT_STROKE_WIDTH,
        }
    }

    pub fn period_ms(&self) -> u32 {
        ProgressIndicator::INDETERMINATE_CIRCULAR_MS
    }

    /// Roughly how long before the indeterminate animation repeats itself.
    pub fn period_minutes(&self) -> f32 {
        self.period_ms() as f32 / 60_000.0
    }
}

// -- Drawing them ---------------------------------------------------------------

/// Upstream `LinearProgressIndicator`, as a widget.
///
/// # The track is a colour of its own and not a dimmed fill
///
/// A track dimmed from the fill reads as "this part is done less". A track in
/// its own colour reads as the space the fill is moving through, which is what
/// it is. Upstream's default is `secondaryContainer`, and that is why.
///
/// # An indeterminate bar has no width to draw
///
/// `value` of `None` means nobody knows how far along it is, and upstream
/// animates a sliver back and forth. This port draws the full track and no
/// fill, and says so: an indeterminate bar drawn as a *full* one would be a
/// lie, and drawn as an empty one is at least true.
impl crate::framework::Component for LinearProgressIndicator {
    fn build(&self, context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        debug_assert!(
            self.base.validate().is_ok(),
            "{}",
            self.base.validate().unwrap_err()
        );
        let resolved = crate::component_themes::ResolvedProgressIndicator::of(context);
        // Upstream's `minHeight ?? theme.linearMinHeight ?? 4`.
        let height = if self.min_height == LinearProgressIndicator::DEFAULT_MIN_HEIGHT {
            resolved.linear_min_height
        } else {
            self.min_height
        };
        let value = self.base.value;
        let fill = resolved.color;
        let track = resolved.linear_track_color;

        crate::framework::leaf(move || {
            let mut bar = crate::widgets::Container::new()
                .with_height(height)
                .with_color(track)
                .with_corner_radius(height / 2.0);
            if let Some(value) = value {
                // A flex weight rather than a width: the bar's own width is
                // whatever it is given, and a fraction of it is not known until
                // then. Writing it as a width would mean guessing.
                let value = value.clamp(0.0, 1.0);
                let filled = (value * 1000.0).round() as u32;
                let rest = 1000 - filled.min(1000);
                let mut row = crate::render::RenderFlex::row()
                    .with_main_axis_size(crate::render::MainAxisSize::Max)
                    .with_main_axis_alignment(crate::render::MainAxisAlignment::Start)
                    .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch);
                // Zero-weight children are not pushed at all: a flex factor of
                // nothing is not a child that takes no room, it is a
                // contradiction, and the flex says so.
                if filled > 0 {
                    row = row.push_flex(crate::render::FlexChild::expanded(
                        crate::render::RenderRef::new(
                            crate::widgets::Container::new()
                                .with_color(fill)
                                .with_corner_radius(height / 2.0),
                        ),
                        filled,
                    ));
                }
                if rest > 0 {
                    row = row.push_flex(crate::render::FlexChild::expanded(
                        crate::render::RenderRef::new(crate::widgets::Empty),
                        rest,
                    ));
                }
                bar = bar.with_child(row);
            }
            bar
        })
    }
}

/// Upstream `CircularProgressIndicator`, as a widget.
///
/// The track is transparent by default -- upstream's `circularTrackColor` is
/// null -- because a spinner with a ring behind it reads as a control that
/// could be dragged rather than as something happening.
impl crate::framework::Component for CircularProgressIndicator {
    fn build(&self, context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        debug_assert!(
            self.base.validate().is_ok(),
            "{}",
            self.base.validate().unwrap_err()
        );
        let resolved = crate::component_themes::ResolvedProgressIndicator::of(context);
        let stroke = self.stroke_width;
        let fill = resolved.color;
        let track = resolved.circular_track_color;
        // Upstream's `_kMinCircularProgressIndicatorSize`.
        let size = 36.0;

        crate::framework::leaf(move || {
            let mut ring = crate::widgets::Container::new()
                .with_size(size, size)
                .with_corner_radius(size / 2.0)
                .with_border(stroke, fill);
            if let Some(track) = track {
                ring = ring.with_color(track);
            }
            ring
        })
    }
}

/// Upstream `RefreshProgressIndicator`: the circular one that
/// [`RefreshIndicator`] shows.
///
/// It is a separate class rather than a configuration because it grows and
/// shrinks with the drag, and a spinner that is being dragged is a different
/// animation from one that is merely spinning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefreshProgressIndicator {
    pub base: ProgressIndicator,
    pub stroke_width: f32,
}

impl RefreshProgressIndicator {
    /// Upstream's default is thicker than the ordinary circular one: it sits on
    /// a card over the content and has to read against whatever is behind it.
    pub const DEFAULT_STROKE_WIDTH: f32 = 3.0;

    pub fn new() -> RefreshProgressIndicator {
        RefreshProgressIndicator {
            base: ProgressIndicator::indeterminate(),
            stroke_width: RefreshProgressIndicator::DEFAULT_STROKE_WIDTH,
        }
    }
}

impl Default for RefreshProgressIndicator {
    fn default() -> Self {
        RefreshProgressIndicator::new()
    }
}

/// Upstream `RefreshIndicatorStatus`.
///
/// Six of them, and the last two are worth noticing: `done` and `canceled` are
/// both fade-outs and differ only in **why**. They are separate because what
/// they look like differs -- a finished refresh scales away, an abandoned drag
/// simply retracts -- and because "it worked" and "you changed your mind" are
/// not the same thing to say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RefreshIndicatorStatus {
    #[default]
    Idle,
    /// A pointer is down.
    Drag,
    /// Dragged far enough that letting go will refresh.
    Armed,
    /// Animating to the indicator's resting displacement.
    Snap,
    /// Running the callback.
    Refresh,
    /// Fading out after refreshing.
    Done,
    /// Fading out after not arming.
    Canceled,
}

/// Upstream `RefreshIndicatorTriggerMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RefreshIndicatorTriggerMode {
    /// Only when the list was already at the top when the drag began. The
    /// default, and the conservative one: a drag that started halfway down is a
    /// scroll, not a pull.
    #[default]
    OnEdge,
    /// Whatever the scroll position was.
    Anywhere,
}

/// Upstream `RefreshIndicator`.
#[derive(Clone, Debug, PartialEq)]
pub struct RefreshIndicator {
    /// Upstream's `semanticsLabel`. `None` is not "unnamed" -- see
    /// [`RefreshIndicator::semantics_label`].
    pub semantics_label: Option<String>,
    pub trigger_mode: RefreshIndicatorTriggerMode,
    /// Where the spinner comes to rest, in pixels from the edge.
    pub displacement: f32,
    status: RefreshIndicatorStatus,
    drag_offset: f32,
    position: f32,
}

impl RefreshIndicator {
    /// The drag needed is **a quarter of the container**, not a fixed number of
    /// pixels -- so a tall list and a short one both ask for "a quarter of what
    /// you can see" rather than the tall one feeling stiff.
    pub const DRAG_CONTAINER_EXTENT_PERCENTAGE: f32 = 0.25;

    /// Upstream's `RefreshIndicator.semanticsLabel`.
    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// What the spinner is announced as, from upstream's
    /// `widget.semanticsLabel ?? MaterialLocalizations.of(context)
    /// .refreshIndicatorSemanticLabel`.
    ///
    /// The default is the verb and not the noun -- "Refresh", not "Loading" --
    /// because a reader meets this while the gesture is still theirs to
    /// finish or abandon, and what matters is what letting go will do.
    pub fn semantics_label(&self) -> String {
        self.semantics_label.clone().unwrap_or_else(|| {
            crate::material_app::DefaultMaterialLocalizations::REFRESH_INDICATOR_SEMANTIC_LABEL
                .to_string()
        })
    }
    /// How far past the resting displacement the drag may push it.
    pub const DRAG_SIZE_FACTOR_LIMIT: f32 = 1.5;
    pub const SNAP_DURATION_MS: f32 = 150.0;
    pub const SCALE_DURATION_MS: f32 = 200.0;
    pub const DEFAULT_DISPLACEMENT: f32 = 40.0;

    pub fn new() -> RefreshIndicator {
        RefreshIndicator {
            semantics_label: None,
            trigger_mode: RefreshIndicatorTriggerMode::OnEdge,
            displacement: RefreshIndicator::DEFAULT_DISPLACEMENT,
            status: RefreshIndicatorStatus::Idle,
            drag_offset: 0.0,
            position: 0.0,
        }
    }

    pub fn status(&self) -> RefreshIndicatorStatus {
        self.status
    }

    pub fn position(&self) -> f32 {
        self.position
    }

    /// Upstream's guard on the notification: only the **outermost** scrollable
    /// (`depth == 0`) and only its **leading** edge. A nested list overscrolling
    /// inside this one is not a pull to refresh, and neither is reaching the
    /// bottom.
    pub fn accepts_notification(depth: usize, leading: bool) -> bool {
        depth == 0 && leading
    }

    pub fn begin_drag(&mut self, at_edge: bool) -> bool {
        if self.trigger_mode == RefreshIndicatorTriggerMode::OnEdge && !at_edge {
            return false;
        }
        self.status = RefreshIndicatorStatus::Drag;
        self.drag_offset = 0.0;
        self.position = 0.0;
        true
    }

    /// Upstream `_checkDragOffset`.
    ///
    /// The line worth keeping is the one that runs only while armed:
    /// `newValue = max(newValue, 1 / _kDragSizeFactorLimit)`. **Once armed, the
    /// indicator will not retreat below its resting size** however far back up
    /// the reader drags. Un-arming by dragging back would make the control
    /// twitchy at exactly the moment the reader is deciding.
    pub fn drag_by(&mut self, overscroll: f32, container_extent: f32) {
        if !matches!(
            self.status,
            RefreshIndicatorStatus::Drag | RefreshIndicatorStatus::Armed
        ) {
            return;
        }
        self.drag_offset += overscroll;
        let mut new_value = self.drag_offset
            / (container_extent * RefreshIndicator::DRAG_CONTAINER_EXTENT_PERCENTAGE);
        if self.status == RefreshIndicatorStatus::Armed {
            new_value = new_value.max(1.0 / RefreshIndicator::DRAG_SIZE_FACTOR_LIMIT);
        }
        self.position = new_value.clamp(0.0, 1.0);
        if self.status == RefreshIndicatorStatus::Drag && self.position >= 1.0 {
            self.status = RefreshIndicatorStatus::Armed;
        }
    }

    /// Letting go.
    pub fn release(&mut self) -> RefreshIndicatorStatus {
        self.status = match self.status {
            RefreshIndicatorStatus::Armed => RefreshIndicatorStatus::Snap,
            RefreshIndicatorStatus::Drag => RefreshIndicatorStatus::Canceled,
            other => other,
        };
        self.status
    }

    /// The snap animation finishing hands over to the callback.
    pub fn snap_complete(&mut self) {
        if self.status == RefreshIndicatorStatus::Snap {
            self.status = RefreshIndicatorStatus::Refresh;
        }
    }

    /// The callback finishing.
    pub fn refresh_complete(&mut self) {
        if self.status == RefreshIndicatorStatus::Refresh {
            self.status = RefreshIndicatorStatus::Done;
        }
    }

    /// Whichever fade-out is running, finishing it returns to idle.
    pub fn fade_complete(&mut self) {
        if matches!(
            self.status,
            RefreshIndicatorStatus::Done | RefreshIndicatorStatus::Canceled
        ) {
            self.status = RefreshIndicatorStatus::Idle;
            self.position = 0.0;
            self.drag_offset = 0.0;
        }
    }
}

impl Default for RefreshIndicator {
    fn default() -> Self {
        RefreshIndicator::new()
    }
}

/// Upstream `RefreshIndicatorState`, which is the machine above.
pub type RefreshIndicatorState = RefreshIndicator;

#[cfg(test)]
mod tests {
    use super::*;

    // -- The two durations are quotations ---------------------------------------

    #[test]
    fn the_circular_period_is_a_product_so_the_animation_never_visibly_repeats() {
        // 1333 * 2222 is about forty-nine minutes -- not a duration anybody
        // intends to watch, but a common multiple, so the rotation and the
        // stroke sweep only come back into phase after a very long time.
        assert_eq!(ProgressIndicator::INDETERMINATE_CIRCULAR_MS, 1333 * 2222);
        let indicator = CircularProgressIndicator::new(ProgressIndicator::indeterminate());
        assert!(
            indicator.period_minutes() > 45.0,
            "{} minutes",
            indicator.period_minutes()
        );
    }

    #[test]
    fn the_linear_period_is_androids_own_number() {
        // Extracted from progress_indeterminate_material.xml, with the source
        // URL in the comment above it.
        assert_eq!(ProgressIndicator::INDETERMINATE_LINEAR_MS, 1800);
        assert_eq!(
            LinearProgressIndicator::new(ProgressIndicator::indeterminate()).period_ms(),
            1800
        );
    }

    // -- Determinate and not ------------------------------------------------------

    #[test]
    fn a_value_and_a_controller_are_two_different_indicators_asked_for_at_once() {
        let plain = ProgressIndicator::determinate(0.5);
        assert_eq!(plain.validate(), Ok(()));
        assert!(plain.is_determinate());

        let spinning = ProgressIndicator {
            has_controller: true,
            ..ProgressIndicator::indeterminate()
        };
        assert_eq!(spinning.validate(), Ok(()));
        assert!(!spinning.is_determinate());

        let both = ProgressIndicator {
            value: Some(0.5),
            has_controller: true,
        };
        assert!(both.validate().is_err());
    }

    #[test]
    fn the_track_gap_is_ramped_away_at_the_empty_end() {
        // At zero there is no bar, so a gap between the bar and the track would
        // be a notch floating in nothing.
        let empty = ProgressIndicator::determinate(0.0);
        assert_eq!(empty.track_gap_scale(), 0.0);

        let almost_empty = ProgressIndicator::determinate(0.005);
        assert!(almost_empty.track_gap_scale() > 0.0);
        assert!(almost_empty.track_gap_scale() < 1.0);

        let ordinary = ProgressIndicator::determinate(0.5);
        assert_eq!(ordinary.track_gap_scale(), 1.0);
    }

    #[test]
    fn an_indeterminate_indicator_has_a_full_gap_because_it_has_no_empty_end() {
        assert_eq!(ProgressIndicator::indeterminate().track_gap_scale(), 1.0);
    }

    #[test]
    fn the_refresh_spinner_is_drawn_thicker_than_the_ordinary_one() {
        // It sits on a card over the content and has to read against whatever
        // is behind it.
        assert_ne!(
            RefreshProgressIndicator::DEFAULT_STROKE_WIDTH,
            CircularProgressIndicator::DEFAULT_STROKE_WIDTH
        );
        assert!(!RefreshProgressIndicator::new().base.is_determinate());
    }

    // -- Pull to refresh --------------------------------------------------------------

    fn indicator() -> RefreshIndicator {
        let mut indicator = RefreshIndicator::new();
        indicator.begin_drag(true);
        indicator
    }

    #[test]
    fn a_drag_that_started_halfway_down_is_a_scroll_and_not_a_pull() {
        let mut on_edge = RefreshIndicator::new();
        assert!(!on_edge.begin_drag(false));
        assert_eq!(on_edge.status(), RefreshIndicatorStatus::Idle);
        assert!(on_edge.begin_drag(true));

        let mut anywhere = RefreshIndicator::new();
        anywhere.trigger_mode = RefreshIndicatorTriggerMode::Anywhere;
        assert!(anywhere.begin_drag(false));
    }

    #[test]
    fn the_pull_asks_for_a_quarter_of_what_you_can_see() {
        // Not a fixed number of pixels, so a tall list does not feel stiff.
        let mut short = indicator();
        short.drag_by(100.0, 400.0);
        assert_eq!(short.position(), 1.0, "a quarter of 400 is 100");

        let mut tall = indicator();
        tall.drag_by(100.0, 800.0);
        assert_eq!(tall.position(), 0.5, "and only half as far up an 800 list");
    }

    #[test]
    fn once_armed_the_indicator_does_not_retreat_below_its_resting_size() {
        // Un-arming by dragging back would make the control twitchy at exactly
        // the moment the reader is deciding.
        let mut indicator = indicator();
        indicator.drag_by(100.0, 400.0);
        assert_eq!(indicator.status(), RefreshIndicatorStatus::Armed);

        indicator.drag_by(-95.0, 400.0);
        assert_eq!(
            indicator.status(),
            RefreshIndicatorStatus::Armed,
            "still armed"
        );
        assert!(
            (indicator.position() - 1.0 / RefreshIndicator::DRAG_SIZE_FACTOR_LIMIT).abs() < 1e-6,
            "and held at the floor: {}",
            indicator.position()
        );
    }

    #[test]
    fn letting_go_armed_snaps_and_letting_go_short_cancels() {
        let mut armed = indicator();
        armed.drag_by(100.0, 400.0);
        assert_eq!(armed.release(), RefreshIndicatorStatus::Snap);

        let mut short = indicator();
        short.drag_by(20.0, 400.0);
        assert_eq!(short.release(), RefreshIndicatorStatus::Canceled);
    }

    #[test]
    fn the_whole_cycle_ends_back_at_idle() {
        let mut indicator = indicator();
        indicator.drag_by(100.0, 400.0);
        indicator.release();
        indicator.snap_complete();
        assert_eq!(indicator.status(), RefreshIndicatorStatus::Refresh);

        indicator.refresh_complete();
        assert_eq!(indicator.status(), RefreshIndicatorStatus::Done);

        indicator.fade_complete();
        assert_eq!(indicator.status(), RefreshIndicatorStatus::Idle);
        assert_eq!(indicator.position(), 0.0);
    }

    #[test]
    fn done_and_canceled_are_both_fade_outs_and_differ_only_in_why() {
        // Which is why they are separate: "it worked" and "you changed your
        // mind" are not the same thing to say.
        let mut cancelled = indicator();
        cancelled.drag_by(20.0, 400.0);
        cancelled.release();
        assert_eq!(cancelled.status(), RefreshIndicatorStatus::Canceled);
        cancelled.fade_complete();
        assert_eq!(cancelled.status(), RefreshIndicatorStatus::Idle);
    }

    #[test]
    fn a_nested_list_overscrolling_inside_this_one_is_not_a_pull_to_refresh() {
        assert!(RefreshIndicator::accepts_notification(0, true));
        assert!(!RefreshIndicator::accepts_notification(1, true));
        assert!(
            !RefreshIndicator::accepts_notification(0, false),
            "and neither is reaching the bottom"
        );
    }

    #[test]
    fn dragging_while_idle_does_nothing() {
        let mut idle = RefreshIndicator::new();
        idle.drag_by(100.0, 400.0);
        assert_eq!(idle.position(), 0.0);
        assert_eq!(idle.status(), RefreshIndicatorStatus::Idle);
    }
}

#[cfg(test)]
mod progress_widget_tests {
    use super::*;
    use crate::component_themes::{
        ProgressIndicatorTheme, ProgressIndicatorThemeData, ResolvedProgressIndicator,
    };
    use crate::engine::Color;
    use crate::framework::{BuildContext, Component, ElementTree, component, provide};
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};

    struct Reader(std::rc::Rc<std::cell::RefCell<Option<ResolvedProgressIndicator>>>);

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
            *self.0.borrow_mut() = Some(ResolvedProgressIndicator::of(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(data: ProgressIndicatorThemeData) -> ResolvedProgressIndicator {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            crate::components::Theme::dark(),
            ProgressIndicatorTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// Lays a widget out at 200 by 40 and reports every child rectangle in it.
    fn boxes(widget: crate::framework::AnyWidget) -> Vec<(Offset, Size)> {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(crate::components::Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(200.0, 40.0));

        fn walk(node: &dyn RenderBox, at: Offset, found: &mut Vec<(Offset, Size)>) {
            node.visit_children(&mut |child, offset| {
                let here = Offset::new(at.dx + offset.dx, at.dy + offset.dy);
                found.push((here, child.size()));
                walk(child, here, found);
            });
        }
        let mut found = Vec::new();
        walk(&root, Offset::ZERO, &mut found);
        found
    }

    fn bar(value: Option<f32>) -> LinearProgressIndicator {
        LinearProgressIndicator::new(ProgressIndicator {
            value,
            has_controller: false,
        })
    }

    #[test]
    fn the_track_is_a_colour_of_its_own_and_not_a_dimmed_fill() {
        // A track dimmed from the fill reads as "this part is done less". Its
        // own colour reads as the space the fill is moving through.
        let resolved = resolve(ProgressIndicatorThemeData::new());
        let scheme = crate::theme::ThemeData::fallback().color_scheme;
        assert_eq!(resolved.color, scheme.primary);
        assert_eq!(resolved.linear_track_color, scheme.secondary_container());
        assert_ne!(resolved.color, resolved.linear_track_color);
    }

    #[test]
    fn a_spinner_has_no_ring_behind_it_unless_one_is_asked_for() {
        // A ring makes it read as a control that could be dragged rather than
        // as something happening. The linear one has a track by default and the
        // circular one does not, and the difference is upstream's.
        let resolved = resolve(ProgressIndicatorThemeData::new());
        assert_eq!(resolved.circular_track_color, None);

        let mut data = ProgressIndicatorThemeData::new();
        data.circular_track_color = Some(Color::argb(0xFF, 1, 2, 3));
        assert_eq!(
            resolve(data).circular_track_color,
            Some(Color::argb(0xFF, 1, 2, 3))
        );
    }

    #[test]
    fn the_theme_beats_the_defaults() {
        let mut data = ProgressIndicatorThemeData::new();
        data.color = Some(Color::argb(0xFF, 9, 9, 9));
        data.linear_min_height = Some(11.0);
        let resolved = resolve(data);
        assert_eq!(resolved.color, Color::argb(0xFF, 9, 9, 9));
        assert_eq!(resolved.linear_min_height, 11.0);
    }

    #[test]
    fn a_bar_is_the_themes_height_and_a_widget_that_asked_gets_its_own() {
        let mut data = ProgressIndicatorThemeData::new();
        data.linear_min_height = Some(10.0);
        let themed = {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                ProgressIndicatorTheme::new(data.clone(), component(bar(Some(0.5)))),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints {
                min_width: 0.0,
                max_width: 200.0,
                min_height: 0.0,
                max_height: 100.0,
            })
        };
        assert_eq!(themed.height, 10.0);

        let mut asked = bar(Some(0.5));
        asked.min_height = 20.0;
        let own = {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                ProgressIndicatorTheme::new(data, component(asked)),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints {
                min_width: 0.0,
                max_width: 200.0,
                min_height: 0.0,
                max_height: 100.0,
            })
        };
        assert_eq!(own.height, 20.0);
    }

    #[test]
    fn the_fill_is_as_wide_a_fraction_of_the_bar_as_the_value_says() {
        // Read off the laid-out tree rather than from the code: a fraction of a
        // width nobody knows until layout is exactly the thing that can be got
        // wrong by writing it as a width.
        let half = boxes(component(bar(Some(0.5))));
        let widest = half
            .iter()
            .filter(|(_, size)| size.width > 0.0 && size.width < 200.0)
            .map(|(_, size)| size.width)
            .fold(0.0f32, f32::max);
        assert!(
            (widest - 100.0).abs() < 1.0,
            "half of two hundred, got {widest}"
        );

        let quarter = boxes(component(bar(Some(0.25))));
        let quarter_width = quarter
            .iter()
            .filter(|(_, size)| size.width > 0.0 && size.width < 200.0)
            .map(|(_, size)| size.width)
            .fold(0.0f32, f32::max);
        assert!(
            (quarter_width - 50.0).abs() < 1.0,
            "a quarter, got {quarter_width}"
        );
    }

    #[test]
    fn a_full_bar_leaves_nothing_and_an_empty_one_fills_nothing() {
        let full = boxes(component(bar(Some(1.0))));
        assert!(
            full.iter()
                .any(|(_, size)| (size.width - 200.0).abs() < 1.0),
            "the fill reaches the end"
        );

        let empty = boxes(component(bar(Some(0.0))));
        assert!(
            !empty
                .iter()
                .any(|(_, size)| size.width > 0.0 && size.width < 200.0),
            "and nothing is filled at zero: {empty:?}"
        );
    }

    #[test]
    fn an_indeterminate_bar_draws_the_track_and_no_fill() {
        // Drawing it full would be a lie about how far along it is; drawing it
        // empty is at least true.
        // Counted in rectangles drawn, not in box widths: a *full* fill is
        // exactly as wide as the track, so "narrower than the bar" -- which is
        // how the other tests here find a fill -- cannot see one. Counting
        // catches both the empty case and the full one.
        fn rects(value: Option<f32>) -> u32 {
            crate::engine_test_stubs::reset_layer_calls();
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                crate::components::Theme::dark(),
                component(bar(value)),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::tight(200.0, 40.0));
            let mut layers = crate::engine::LayerTree::new(200, 40);
            {
                let mut context =
                    crate::render::PaintContext::new(&mut layers, Size::new(200.0, 40.0));
                root.paint(&mut context, Offset::ZERO);
            }
            crate::engine_test_stubs::layer_calls().rects
        }

        let track_only = rects(None);
        assert_eq!(
            rects(Some(0.5)),
            track_only + 1,
            "a value adds exactly one rectangle: the fill"
        );
        assert_eq!(
            rects(Some(1.0)),
            track_only + 1,
            "and a full bar is still one fill, not none"
        );
    }

    #[test]
    fn a_value_and_a_controller_together_are_refused() {
        // Upstream's assert: having both is asking for both kinds of indicator
        // at once.
        let base = ProgressIndicator {
            value: Some(0.5),
            has_controller: true,
        };
        assert!(base.validate().is_err());
    }
}

#[cfg(test)]
mod refresh_semantics_tests {
    use super::RefreshIndicator;

    #[test]
    fn the_spinner_is_announced_as_what_letting_go_will_do() {
        // The verb, not the noun. A reader meets this mid-gesture, with the
        // choice still theirs.
        assert_eq!(RefreshIndicator::new().semantics_label(), "Refresh");
    }

    #[test]
    fn and_a_list_that_said_what_it_refreshes_says_that() {
        assert_eq!(
            RefreshIndicator::new()
                .with_semantics_label("Refresh inbox")
                .semantics_label(),
            "Refresh inbox"
        );
    }

    #[test]
    fn the_label_is_not_the_trigger_mode_in_disguise() {
        // Both are configuration on the same widget and neither touches the
        // other: a list that only refreshes from the edge still says the same
        // thing when it does.
        let mut indicator = RefreshIndicator::new();
        indicator.trigger_mode = super::RefreshIndicatorTriggerMode::Anywhere;
        assert_eq!(indicator.semantics_label(), "Refresh");
    }
}
