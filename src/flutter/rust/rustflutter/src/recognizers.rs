//! The last of upstream's gesture recognisers -- `gestures/eager.dart`,
//! `gestures/team.dart`, `gestures/force_press.dart`, and the two axis
//! subclasses of `gestures/monodrag.dart`.
//!
//! They have nothing in common except being small, which is why they are
//! together: each is one idea.
//!
//! * [`EagerGestureRecognizer`] wins every arena it enters, on purpose.
//! * [`GestureArenaTeam`] makes several recognisers share **one** seat, so
//!   they compete with the outside world rather than with each other.
//! * [`ForcePressGestureRecognizer`] watches how hard the screen is being
//!   pressed.
//! * [`VerticalDragGestureRecognizer`] and
//!   [`HorizontalDragGestureRecognizer`] are the axis policies of the drag
//!   recogniser: what counts as a fling, and how fast it is allowed to be.
//!
//! ## What is not here
//!
//! `FlutterErrorDetailsForPointerEventDispatcher` is the last class of the
//! gestures layer still missing, and it waits on `FlutterErrorDetails` -- the
//! diagnostics wave -- rather than on anything about gestures.

use crate::gestures::{
    Disposition, MAX_FLING_VELOCITY, MIN_FLING_VELOCITY, PointerEvent, PointerKind,
    VelocityEstimate, compute_hit_slop,
};
use crate::render::Offset;

// -- Winning on purpose -------------------------------------------------------

/// Upstream `EagerGestureRecognizer`: claims victory in every arena at once.
///
/// This looks like a recogniser that recognises nothing, and that is exactly
/// what it is for. Upstream passes it to an embedded platform view so that
/// every touch inside the view's bounds goes straight to the platform rather
/// than being deliberated over by Flutter -- the embedded view has its own
/// idea of what a touch means, and the arena's job here is to get out of the
/// way as fast as possible.
///
/// It stops tracking the pointer in the same breath as claiming it: having
/// won, there is nothing further it wants to know.
#[derive(Clone, Copy, Debug, Default)]
pub struct EagerGestureRecognizer;

impl EagerGestureRecognizer {
    pub fn new() -> EagerGestureRecognizer {
        EagerGestureRecognizer
    }

    /// Upstream's `addAllowedPointer`, whose entire body is the resolve and
    /// the stop.
    pub fn add_pointer(&self, _event: &PointerEvent) -> Disposition {
        Disposition::Accepted
    }

    /// Upstream's `debugDescription`.
    pub fn debug_description(&self) -> &'static str {
        "eager"
    }
}

// -- Sharing one seat ---------------------------------------------------------

/// A member of a [`GestureArenaTeam`], identified by whatever the caller uses
/// to name its recognisers.
pub type TeamMember = usize;

/// Upstream's `_CombiningGestureArenaMember`: the one seat a team occupies.
///
/// Everything interesting about a team is in here. The team joins the arena
/// **once**, and this stands in for all of its members; the members then
/// resolve against it rather than against the arena.
#[derive(Debug, Default)]
struct Combiner {
    members: Vec<TeamMember>,
    resolved: bool,
    winner: Option<TeamMember>,
    joined_arena: bool,
    /// What this combiner would tell the real arena, drained by the caller.
    verdict: Option<Disposition>,
    /// Which members have been told they lost, in order.
    rejected: Vec<TeamMember>,
    /// Which member was told it won.
    accepted: Option<TeamMember>,
}

/// Upstream `GestureArenaTeam`: several recognisers that compete with the rest
/// of the tree but not with one another.
///
/// The problem it solves: a scrollable's vertical drag and a slider's
/// horizontal drag inside it are both drags, and in a plain arena the first
/// one to claim victory takes the gesture from the other. A team gives them a
/// single seat, so the arena's question becomes "the team or something else",
/// and *which* member gets it is settled inside the team by the rules below.
///
/// * A member that accepts makes itself the winner, unless the team has a
///   [`captain`](Self::captain) -- in which case the captain wins whatever any
///   member says. That is what a captain is for: a recogniser that has the
///   final say on the team's behalf without having to be the one that noticed.
/// * A member that rejects merely leaves. Only when the **last** member has
///   left does the team give up its seat.
#[derive(Debug, Default)]
pub struct GestureArenaTeam {
    /// Upstream's `captain`. When set, it wins for the team no matter which
    /// member spoke first.
    pub captain: Option<TeamMember>,
    combiners: Vec<(i64, Combiner)>,
}

impl GestureArenaTeam {
    pub fn new() -> GestureArenaTeam {
        GestureArenaTeam {
            captain: None,
            combiners: Vec::new(),
        }
    }

    pub fn with_captain(mut self, captain: TeamMember) -> Self {
        self.captain = Some(captain);
        self
    }

    fn combiner(&mut self, pointer: i64) -> &mut Combiner {
        if let Some(at) = self.combiners.iter().position(|(id, _)| *id == pointer) {
            return &mut self.combiners[at].1;
        }
        self.combiners.push((pointer, Combiner::default()));
        let last = self.combiners.len() - 1;
        &mut self.combiners[last].1
    }

    /// Upstream's `add`.
    ///
    /// Returns whether this is the join that puts the team into the arena --
    /// upstream's `_entry ??= gestureArena.add(...)`, which happens on the
    /// first member only.
    pub fn add(&mut self, pointer: i64, member: TeamMember) -> bool {
        let combiner = self.combiner(pointer);
        combiner.members.push(member);
        let first = !combiner.joined_arena;
        combiner.joined_arena = true;
        first
    }

    /// Upstream's `_CombiningGestureArenaEntry.resolve`: one member's verdict,
    /// delivered to the team rather than to the arena.
    pub fn resolve(&mut self, pointer: i64, member: TeamMember, disposition: Disposition) {
        let captain = self.captain;
        let combiner = self.combiner(pointer);
        if combiner.resolved {
            return;
        }
        match disposition {
            Disposition::Accepted => {
                if combiner.winner.is_none() {
                    combiner.winner = Some(captain.unwrap_or(member));
                }
                combiner.verdict = Some(Disposition::Accepted);
            }
            Disposition::Rejected => {
                if let Some(at) = combiner.members.iter().position(|m| *m == member) {
                    combiner.members.remove(at);
                }
                combiner.rejected.push(member);
                if combiner.members.is_empty() {
                    combiner.verdict = Some(Disposition::Rejected);
                }
            }
        }
    }

    /// Takes what the team would tell the real arena about this pointer.
    pub fn take_verdict(&mut self, pointer: i64) -> Option<Disposition> {
        self.combiner(pointer).verdict.take()
    }

    /// Upstream's `acceptGesture` on the combiner: the arena gave the team the
    /// gesture, and the team hands it to one member and turns everyone else
    /// down.
    pub fn accept_gesture(&mut self, pointer: i64) {
        let captain = self.captain;
        let combiner = self.combiner(pointer);
        combiner.resolved = true;
        // Upstream's `_winner ??= _owner.captain ?? _members[0]`: with nobody
        // having spoken, the captain decides, and failing that the member that
        // joined first. **Join order is the tiebreak**, which is why a team
        // built in one order does not behave like the same team built in
        // another.
        if combiner.winner.is_none() {
            combiner.winner = captain.or_else(|| combiner.members.first().copied());
        }
        let winner = combiner.winner;
        let losers: Vec<TeamMember> = combiner
            .members
            .iter()
            .copied()
            .filter(|member| Some(*member) != winner)
            .collect();
        combiner.rejected.extend(losers);
        combiner.accepted = winner;
    }

    /// Upstream's `rejectGesture` on the combiner: everyone loses.
    pub fn reject_gesture(&mut self, pointer: i64) {
        let combiner = self.combiner(pointer);
        combiner.resolved = true;
        let losers = combiner.members.clone();
        combiner.rejected.extend(losers);
    }

    /// Which member the team gave the gesture to, if the arena has ruled.
    pub fn winner(&mut self, pointer: i64) -> Option<TeamMember> {
        self.combiner(pointer).accepted
    }

    /// Which members have been told they lost, in the order they were told.
    pub fn rejected(&mut self, pointer: i64) -> Vec<TeamMember> {
        self.combiner(pointer).rejected.clone()
    }

    /// Whether this pointer's combiner has closed.
    pub fn is_resolved(&mut self, pointer: i64) -> bool {
        self.combiner(pointer).resolved
    }
}

// -- How hard the screen is being pressed -------------------------------------

/// Upstream's `_ForceState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceState {
    Ready,
    /// A finger is down but no force press has been detected.
    Possible,
    /// The arena has been won, but the pressure has not yet crossed
    /// `start_pressure` -- upstream's comment is the reason this state exists
    /// separately from `Started`: being the only recogniser in the arena wins
    /// the gesture immediately, and the press must still not *start* until the
    /// finger actually presses.
    Accepted,
    Started,
    Peaked,
}

/// Upstream's `_inverseLerp`, the default interpolation.
///
/// The clamp is skipped for a NaN, and upstream says why: a device that
/// misreports a pressure outside its own declared range should not stop the
/// recogniser working.
pub fn inverse_lerp(min: f32, max: f32, t: f32) -> f32 {
    debug_assert!(min <= max);
    let value = (t - min) / (max - min);
    if value.is_nan() {
        value
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// Upstream `ForcePressGestureRecognizer`: a press that is about *force*.
///
/// Upstream reads each event's `pressureMin` and `pressureMax`; this crate's
/// [`PointerEvent`] carries only `pressure`, so the range is configured on the
/// recogniser instead of arriving with every event. The rest is upstream's.
pub struct ForcePressGestureRecognizer {
    /// Upstream's `startPressure`, normalised: 0.4.
    pub start_pressure: f32,
    /// Upstream's `peakPressure`: 0.85. Upstream asserts it is above the
    /// start, which is the only relationship between them that makes sense.
    pub peak_pressure: f32,
    /// The device's range, which upstream reads off each event.
    pub pressure_min: f32,
    pub pressure_max: f32,
    state: ForceState,
    last_position: Offset,
    last_pressure: f32,
}

impl Default for ForcePressGestureRecognizer {
    fn default() -> ForcePressGestureRecognizer {
        ForcePressGestureRecognizer::new()
    }
}

impl ForcePressGestureRecognizer {
    pub fn new() -> ForcePressGestureRecognizer {
        ForcePressGestureRecognizer {
            start_pressure: 0.4,
            peak_pressure: 0.85,
            pressure_min: 0.0,
            pressure_max: 1.0,
            state: ForceState::Ready,
            last_position: Offset::ZERO,
            last_pressure: 0.0,
        }
    }

    pub fn with_thresholds(mut self, start_pressure: f32, peak_pressure: f32) -> Self {
        debug_assert!(peak_pressure > start_pressure);
        self.start_pressure = start_pressure;
        self.peak_pressure = peak_pressure;
        self
    }

    /// The device's pressure range, upstream's `pressureMin`/`pressureMax`.
    pub fn with_pressure_range(mut self, min: f32, max: f32) -> Self {
        self.pressure_min = min;
        self.pressure_max = max;
        self
    }

    pub fn state(&self) -> ForceState {
        self.state
    }

    pub fn last_pressure(&self) -> f32 {
        self.last_pressure
    }

    pub fn last_position(&self) -> Offset {
        self.last_position
    }

    /// Upstream's `debugDescription`.
    pub fn debug_description(&self) -> &'static str {
        "force press"
    }

    /// Upstream's `addAllowedPointer`.
    ///
    /// **A screen that cannot measure force does not get to play.** Upstream
    /// tests `pressureMax <= 1.0`, which is what a device without pressure
    /// sensing reports, and rejects the pointer outright rather than
    /// competing for a gesture it could never detect.
    pub fn add_pointer(&mut self, event: &PointerEvent) -> Option<Disposition> {
        if self.pressure_max <= 1.0 {
            return Some(Disposition::Rejected);
        }
        if self.state == ForceState::Ready {
            self.state = ForceState::Possible;
            self.last_position = event.position;
        }
        None
    }

    /// Upstream's `handleEvent`, for a move or a down.
    ///
    /// Returns what to tell the arena, and the events to report are read back
    /// from the state transition -- see [`Self::state`].
    ///
    /// Upstream's note is worth keeping: a finger that does not move but
    /// presses harder still produces move events, which is the only reason a
    /// force press can be noticed at all.
    pub fn handle_pressure(&mut self, event: &PointerEvent) -> ForcePressStep {
        let pressure = inverse_lerp(self.pressure_min, self.pressure_max, event.pressure as f32);
        self.last_position = event.position;
        self.last_pressure = pressure;
        let mut step = ForcePressStep::default();

        if self.state == ForceState::Possible {
            if pressure > self.start_pressure {
                self.state = ForceState::Started;
                step.resolution = Some(Disposition::Accepted);
            } else if event.delta.distance_squared() > compute_hit_slop(event.kind) {
                // Upstream compares a squared distance against an unsquared
                // slop. Ported as written: it makes the recogniser give up
                // sooner than the name suggests, and every caller has been
                // running against that.
                step.resolution = Some(Disposition::Rejected);
            }
        }

        if pressure > self.start_pressure && self.state == ForceState::Accepted {
            self.state = ForceState::Started;
            step.start = true;
        }
        if pressure > self.peak_pressure && self.state == ForceState::Started {
            self.state = ForceState::Peaked;
            step.peak = true;
        }
        if !pressure.is_nan()
            && (self.state == ForceState::Started || self.state == ForceState::Peaked)
        {
            step.update = true;
        }
        step
    }

    /// Upstream's `acceptGesture`.
    ///
    /// Returns whether the start should be reported now -- which it is only if
    /// the pressure had *already* crossed the threshold, because a gesture won
    /// by default is not yet a press.
    pub fn accept_gesture(&mut self) -> bool {
        if self.state == ForceState::Possible {
            self.state = ForceState::Accepted;
        }
        self.state == ForceState::Started
    }

    /// Upstream's `didStopTrackingLastPointer`.
    ///
    /// Returns whether the end should be reported, and what to tell the arena.
    pub fn stop_tracking(&mut self) -> (bool, Option<Disposition>) {
        let was_accepted = self.state == ForceState::Started || self.state == ForceState::Peaked;
        if self.state == ForceState::Possible {
            self.state = ForceState::Ready;
            return (false, Some(Disposition::Rejected));
        }
        self.state = ForceState::Ready;
        (was_accepted, None)
    }
}

/// What one pressure event asks the caller to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForcePressStep {
    pub resolution: Option<Disposition>,
    pub start: bool,
    pub peak: bool,
    pub update: bool,
}

// -- Which way a drag has to go -----------------------------------------------

/// The axis policy of upstream's `DragGestureRecognizer` subclasses.
///
/// The two named types below are what upstream calls them; this enum is the
/// shared body, because the subclasses are three method overrides each and the
/// overrides only ever read one component or the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragAxis {
    Vertical,
    Horizontal,
}

impl DragAxis {
    /// The component this axis reads out of an offset.
    pub fn component(self, offset: Offset) -> f32 {
        match self {
            DragAxis::Vertical => offset.dy,
            DragAxis::Horizontal => offset.dx,
        }
    }

    /// Upstream's `_getDeltaForDetails`: the movement flattened onto the axis.
    pub fn delta_for_details(self, delta: Offset) -> Offset {
        match self {
            DragAxis::Vertical => Offset::new(0.0, delta.dy),
            DragAxis::Horizontal => Offset::new(delta.dx, 0.0),
        }
    }
}

/// The shared body of upstream's `VerticalDragGestureRecognizer` and
/// `HorizontalDragGestureRecognizer`.
#[derive(Clone, Copy, Debug)]
pub struct AxisDragPolicy {
    pub axis: DragAxis,
    /// Upstream's `minFlingVelocity`, `None` for `kMinFlingVelocity`.
    pub min_fling_velocity: Option<f32>,
    /// Upstream's `minFlingDistance`, `None` for the hit slop.
    pub min_fling_distance: Option<f32>,
    /// Upstream's `maxFlingVelocity`, `None` for `kMaxFlingVelocity`.
    pub max_fling_velocity: Option<f32>,
}

impl AxisDragPolicy {
    pub fn new(axis: DragAxis) -> AxisDragPolicy {
        AxisDragPolicy {
            axis,
            min_fling_velocity: None,
            min_fling_distance: None,
            max_fling_velocity: None,
        }
    }

    /// Upstream's `isFlingGesture`: **fast enough and far enough, both.**
    ///
    /// The distance test is the one that is easy to leave out and expensive to
    /// leave out. A finger that jitters in place at the moment of release can
    /// register a high instantaneous velocity while having gone nowhere, and
    /// without the distance test that jitter flings the list.
    pub fn is_fling(&self, estimate: &VelocityEstimate, kind: PointerKind) -> bool {
        let min_velocity = self.min_fling_velocity.unwrap_or(MIN_FLING_VELOCITY);
        let min_distance = self
            .min_fling_distance
            .unwrap_or_else(|| compute_hit_slop(kind));
        self.axis.component(estimate.pixels_per_second).abs() > min_velocity
            && self.axis.component(estimate.offset).abs() > min_distance
    }

    /// Upstream's `considerFling`: the fling's velocity, along the axis only
    /// and clamped, or `None` if this was not a fling.
    ///
    /// The clamp is what keeps a fling that the estimator got over-excited
    /// about from throwing a list a thousand screens.
    pub fn consider_fling(
        &self,
        estimate: &VelocityEstimate,
        kind: PointerKind,
    ) -> Option<(Offset, f32)> {
        if !self.is_fling(estimate, kind) {
            return None;
        }
        let max_velocity = self.max_fling_velocity.unwrap_or(MAX_FLING_VELOCITY);
        let along = self
            .axis
            .component(estimate.pixels_per_second)
            .clamp(-max_velocity, max_velocity);
        let velocity = match self.axis {
            DragAxis::Vertical => Offset::new(0.0, along),
            DragAxis::Horizontal => Offset::new(along, 0.0),
        };
        Some((velocity, along))
    }

    /// Upstream's `hasSufficientGlobalDistanceToAccept`, which for both axis
    /// recognisers is the **hit** slop rather than the pan slop -- a drag
    /// already committed to one direction does not need to be as deliberate as
    /// a free one.
    pub fn has_sufficient_global_distance_to_accept(
        &self,
        global_distance_moved: f32,
        kind: PointerKind,
    ) -> bool {
        global_distance_moved.abs() > compute_hit_slop(kind)
    }
}

/// Upstream `VerticalDragGestureRecognizer`: drags up and down.
#[derive(Clone, Copy, Debug)]
pub struct VerticalDragGestureRecognizer {
    pub policy: AxisDragPolicy,
}

/// Upstream `HorizontalDragGestureRecognizer`: drags left and right.
#[derive(Clone, Copy, Debug)]
pub struct HorizontalDragGestureRecognizer {
    pub policy: AxisDragPolicy,
}

macro_rules! axis_drag_recognizer {
    ($name:ident, $axis:expr, $description:literal) => {
        impl $name {
            pub fn new() -> $name {
                $name {
                    policy: AxisDragPolicy::new($axis),
                }
            }

            /// Upstream's `debugDescription`.
            pub fn debug_description(&self) -> &'static str {
                $description
            }
        }

        impl Default for $name {
            fn default() -> $name {
                $name::new()
            }
        }

        impl std::ops::Deref for $name {
            type Target = AxisDragPolicy;

            fn deref(&self) -> &AxisDragPolicy {
                &self.policy
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut AxisDragPolicy {
                &mut self.policy
            }
        }
    };
}

axis_drag_recognizer!(
    VerticalDragGestureRecognizer,
    DragAxis::Vertical,
    "vertical drag"
);
axis_drag_recognizer!(
    HorizontalDragGestureRecognizer,
    DragAxis::Horizontal,
    "horizontal drag"
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::{PRIMARY_BUTTON, PointerChange, SignalKind, TOUCH_SLOP};

    fn event(position: Offset, delta: Offset, pressure: f64) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change: PointerChange::Move,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: PRIMARY_BUTTON,
            time_stamp_micros: 0,
            position,
            delta,
            scroll_delta: Offset::ZERO,
            pressure,
            local_position: position,
        }
    }

    #[test]
    fn the_eager_recognizer_wins_without_looking_at_anything() {
        // Which is the whole of it. An embedded platform view has its own idea
        // of what a touch means, and the arena's job is to get out of the way
        // as fast as it can.
        let eager = EagerGestureRecognizer::new();
        assert_eq!(
            eager.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0)),
            Disposition::Accepted
        );
        assert_eq!(eager.debug_description(), "eager");
    }

    // -- The team --------------------------------------------------------

    #[test]
    fn a_team_takes_one_seat_no_matter_how_many_members_join() {
        // The point of a team: the arena's question becomes "the team or
        // something else", and which member gets it is settled inside.
        let mut team = GestureArenaTeam::new();
        assert!(
            team.add(1, 0),
            "the first member puts the team in the arena"
        );
        assert!(!team.add(1, 1), "the second does not join again");
        assert!(!team.add(1, 2));
        assert!(team.add(2, 0), "a different pointer is a different seat");
    }

    #[test]
    fn the_first_member_to_accept_wins_and_the_rest_are_told_they_lost() {
        let mut team = GestureArenaTeam::new();
        team.add(1, 10);
        team.add(1, 20);
        team.add(1, 30);
        team.resolve(1, 20, Disposition::Accepted);
        assert_eq!(
            team.take_verdict(1),
            Some(Disposition::Accepted),
            "and the team tells the real arena"
        );
        team.accept_gesture(1);
        assert_eq!(team.winner(1), Some(20));
        assert_eq!(team.rejected(1), vec![10, 30]);
    }

    #[test]
    fn a_captain_wins_for_the_team_whoever_actually_noticed() {
        // What a captain is for: a recogniser with the final say on the team's
        // behalf, without having to be the one that saw the gesture first.
        let mut team = GestureArenaTeam::new().with_captain(99);
        team.add(1, 10);
        team.add(1, 20);
        team.resolve(1, 20, Disposition::Accepted);
        team.accept_gesture(1);
        assert_eq!(team.winner(1), Some(99));
        assert_eq!(
            team.rejected(1),
            vec![10, 20],
            "including the one that spoke"
        );
    }

    #[test]
    fn a_member_leaving_does_not_give_up_the_seat_until_it_is_the_last() {
        // The team only stops competing when there is nobody left to compete
        // for -- otherwise one member changing its mind would hand the gesture
        // to something outside the team.
        let mut team = GestureArenaTeam::new();
        team.add(1, 10);
        team.add(1, 20);
        team.resolve(1, 10, Disposition::Rejected);
        assert_eq!(team.take_verdict(1), None, "one down, one to go");
        assert_eq!(team.rejected(1), vec![10]);
        team.resolve(1, 20, Disposition::Rejected);
        assert_eq!(team.take_verdict(1), Some(Disposition::Rejected));
    }

    #[test]
    fn with_nobody_having_spoken_join_order_is_the_tiebreak() {
        // Upstream's `_winner ??= captain ?? _members[0]`. Which means a team
        // assembled in one order does not behave like the same team assembled
        // in another, and that is worth knowing rather than discovering.
        let mut team = GestureArenaTeam::new();
        team.add(1, 10);
        team.add(1, 20);
        team.accept_gesture(1);
        assert_eq!(team.winner(1), Some(10));

        let mut other = GestureArenaTeam::new();
        other.add(1, 20);
        other.add(1, 10);
        other.accept_gesture(1);
        assert_eq!(other.winner(1), Some(20));
    }

    #[test]
    fn a_team_that_loses_the_arena_tells_everyone() {
        let mut team = GestureArenaTeam::new();
        team.add(1, 10);
        team.add(1, 20);
        team.reject_gesture(1);
        assert_eq!(team.rejected(1), vec![10, 20]);
        assert!(team.is_resolved(1));
    }

    #[test]
    fn a_resolved_team_stops_listening() {
        let mut team = GestureArenaTeam::new();
        team.add(1, 10);
        team.accept_gesture(1);
        assert_eq!(team.rejected(1), Vec::<TeamMember>::new());
        team.resolve(1, 10, Disposition::Rejected);
        assert_eq!(team.winner(1), Some(10), "the verdict already went out");
        assert_eq!(
            team.rejected(1),
            Vec::<TeamMember>::new(),
            "and the late change of mind changed nothing"
        );
    }

    // -- Force press -----------------------------------------------------

    #[test]
    fn a_screen_that_cannot_measure_force_does_not_get_to_play() {
        // Upstream tests pressureMax <= 1.0, which is what a device without
        // pressure sensing reports, and rejects rather than competing for a
        // gesture it could never detect.
        let mut plain = ForcePressGestureRecognizer::new();
        assert_eq!(
            plain.add_pointer(&event(Offset::ZERO, Offset::ZERO, 1.0)),
            Some(Disposition::Rejected)
        );

        let mut sensing = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 6.0);
        assert_eq!(
            sensing.add_pointer(&event(Offset::ZERO, Offset::ZERO, 1.0)),
            None
        );
        assert_eq!(sensing.state(), ForceState::Possible);
    }

    #[test]
    fn pressing_hard_enough_claims_the_gesture() {
        let mut recognizer = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 6.0);
        recognizer.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0));
        // 0.4 of the way up the range is exactly the threshold, so just past.
        let step = recognizer.handle_pressure(&event(Offset::ZERO, Offset::ZERO, 3.0));
        assert_eq!(step.resolution, Some(Disposition::Accepted));
        assert_eq!(recognizer.state(), ForceState::Started);
        assert!(step.update);
        assert!(!step.peak);
    }

    #[test]
    fn winning_the_arena_alone_is_not_yet_a_press() {
        // Upstream's reason for having an Accepted state at all: being the only
        // recogniser in the arena wins the gesture immediately, and the press
        // must still not start until the finger actually presses.
        let mut recognizer = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 6.0);
        recognizer.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0));
        assert!(!recognizer.accept_gesture(), "nothing to report yet");
        assert_eq!(recognizer.state(), ForceState::Accepted);

        let step = recognizer.handle_pressure(&event(Offset::ZERO, Offset::ZERO, 3.0));
        assert!(step.start, "now it starts");
        assert_eq!(recognizer.state(), ForceState::Started);
    }

    #[test]
    fn the_peak_is_reported_once_and_updates_carry_on_past_it() {
        // Upstream is explicit that crossing the peak does not end anything.
        let mut recognizer = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 2.0);
        recognizer.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0));
        recognizer.handle_pressure(&event(Offset::ZERO, Offset::ZERO, 1.0));
        assert_eq!(recognizer.state(), ForceState::Started);

        let peaked = recognizer.handle_pressure(&event(Offset::ZERO, Offset::ZERO, 1.9));
        assert!(peaked.peak);
        assert_eq!(recognizer.state(), ForceState::Peaked);

        let after = recognizer.handle_pressure(&event(Offset::ZERO, Offset::ZERO, 1.95));
        assert!(!after.peak, "said once");
        assert!(after.update, "and it keeps reporting");
    }

    #[test]
    fn a_finger_that_slides_gives_up_on_the_press() {
        let mut recognizer = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 6.0);
        recognizer.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0));
        let step =
            recognizer.handle_pressure(&event(Offset::new(30.0, 0.0), Offset::new(30.0, 0.0), 0.5));
        assert_eq!(step.resolution, Some(Disposition::Rejected));
    }

    #[test]
    fn the_interpolation_clamps_a_misreported_pressure_but_passes_a_nan_through() {
        // Upstream's reason for the asymmetry: a device that misreports a
        // pressure outside its own declared range should not stop the
        // recogniser working, but an interpolation that declines to answer
        // must be able to say so.
        assert_eq!(inverse_lerp(0.0, 2.0, 1.0), 0.5);
        assert_eq!(inverse_lerp(0.0, 2.0, 4.0), 1.0, "clamped");
        assert_eq!(inverse_lerp(0.0, 2.0, -1.0), 0.0, "clamped");
        assert!(inverse_lerp(1.0, 1.0, 1.0).is_nan(), "a range of nothing");
    }

    #[test]
    fn letting_go_before_pressing_is_not_a_press() {
        let mut recognizer = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 6.0);
        recognizer.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0));
        assert_eq!(
            recognizer.stop_tracking(),
            (false, Some(Disposition::Rejected))
        );

        // Whereas letting go mid-press ends it.
        let mut recognizer = ForcePressGestureRecognizer::new().with_pressure_range(0.0, 6.0);
        recognizer.add_pointer(&event(Offset::ZERO, Offset::ZERO, 0.0));
        recognizer.handle_pressure(&event(Offset::ZERO, Offset::ZERO, 3.0));
        assert_eq!(recognizer.stop_tracking(), (true, None));
    }

    // -- The two axis drags ----------------------------------------------

    fn estimate(velocity: Offset, offset: Offset) -> VelocityEstimate {
        VelocityEstimate {
            pixels_per_second: velocity,
            offset,
            duration_micros: 16_000,
        }
    }

    #[test]
    fn a_fling_has_to_be_fast_enough_and_to_have_gone_somewhere() {
        // The distance half is the one that is easy to leave out and expensive
        // to leave out: a finger that jitters in place at the moment of release
        // registers a high instantaneous velocity while having gone nowhere,
        // and without it that jitter flings the list.
        let vertical = VerticalDragGestureRecognizer::new();
        let far = TOUCH_SLOP + 5.0;
        assert!(vertical.is_fling(
            &estimate(Offset::new(0.0, 400.0), Offset::new(0.0, far)),
            PointerKind::Touch
        ));
        assert!(
            !vertical.is_fling(
                &estimate(Offset::new(0.0, 4000.0), Offset::new(0.0, 1.0)),
                PointerKind::Touch
            ),
            "fast but stationary is a twitch"
        );
        assert!(
            !vertical.is_fling(
                &estimate(Offset::new(0.0, 10.0), Offset::new(0.0, far)),
                PointerKind::Touch
            ),
            "far but slow is a drag that stopped"
        );
    }

    #[test]
    fn each_axis_reads_only_its_own_component() {
        let far = TOUCH_SLOP + 5.0;
        let sideways = estimate(Offset::new(400.0, 0.0), Offset::new(far, 0.0));
        assert!(HorizontalDragGestureRecognizer::new().is_fling(&sideways, PointerKind::Touch));
        assert!(!VerticalDragGestureRecognizer::new().is_fling(&sideways, PointerKind::Touch));
    }

    #[test]
    fn a_fling_is_reported_along_its_axis_only_and_clamped() {
        // The clamp keeps a fling the estimator got over-excited about from
        // throwing a list a thousand screens.
        let vertical = VerticalDragGestureRecognizer::new();
        let far = TOUCH_SLOP + 5.0;
        let (velocity, primary) = vertical
            .consider_fling(
                &estimate(Offset::new(9999.0, 50_000.0), Offset::new(far, far)),
                PointerKind::Touch,
            )
            .expect("that is a fling");
        assert_eq!(velocity.dx, 0.0, "the other axis is dropped entirely");
        assert_eq!(velocity.dy, MAX_FLING_VELOCITY);
        assert_eq!(primary, MAX_FLING_VELOCITY);

        assert!(
            vertical
                .consider_fling(&estimate(Offset::ZERO, Offset::ZERO), PointerKind::Touch)
                .is_none()
        );
    }

    #[test]
    fn a_caller_may_move_the_thresholds() {
        let mut picky = VerticalDragGestureRecognizer::new();
        picky.min_fling_velocity = Some(2000.0);
        let far = TOUCH_SLOP + 5.0;
        assert!(!picky.is_fling(
            &estimate(Offset::new(0.0, 400.0), Offset::new(0.0, far)),
            PointerKind::Touch
        ));

        let mut lenient = VerticalDragGestureRecognizer::new();
        lenient.min_fling_distance = Some(0.5);
        assert!(lenient.is_fling(
            &estimate(Offset::new(0.0, 400.0), Offset::new(0.0, 1.0)),
            PointerKind::Touch
        ));
    }

    #[test]
    fn both_axis_recognizers_accept_at_the_hit_slop_not_the_pan_slop() {
        // A drag already committed to one direction does not have to be as
        // deliberate as a free one.
        let just_past = TOUCH_SLOP + 1.0;
        assert!(
            VerticalDragGestureRecognizer::new()
                .has_sufficient_global_distance_to_accept(just_past, PointerKind::Touch)
        );
        assert!(
            HorizontalDragGestureRecognizer::new()
                .has_sufficient_global_distance_to_accept(-just_past, PointerKind::Touch)
        );
        assert!(
            !VerticalDragGestureRecognizer::new()
                .has_sufficient_global_distance_to_accept(TOUCH_SLOP - 1.0, PointerKind::Touch)
        );
    }

    #[test]
    fn the_axis_flattens_a_delta_onto_itself() {
        let both = Offset::new(3.0, 5.0);
        assert_eq!(
            DragAxis::Vertical.delta_for_details(both),
            Offset::new(0.0, 5.0)
        );
        assert_eq!(
            DragAxis::Horizontal.delta_for_details(both),
            Offset::new(3.0, 0.0)
        );
    }

    #[test]
    fn the_recognizers_carry_upstreams_descriptions() {
        assert_eq!(
            VerticalDragGestureRecognizer::new().debug_description(),
            "vertical drag"
        );
        assert_eq!(
            HorizontalDragGestureRecognizer::new().debug_description(),
            "horizontal drag"
        );
        assert_eq!(
            ForcePressGestureRecognizer::new().debug_description(),
            "force press"
        );
    }
}

/// Upstream `GestureRecognizerState`: where a primary-pointer recogniser is in
/// its attempt at a gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GestureRecognizerState {
    /// Not attempting anything. **The only state that will take a new primary
    /// pointer** -- see [`PrimaryPointerTracking::add_allowed_pointer`].
    #[default]
    Ready,
    /// Watching a pointer, with the arena undecided.
    Possible,
    /// This attempt is over, and **the recogniser is not.** Upstream returns
    /// to `Ready` from here, so `Defunct` means "the gesture I was watching
    /// for did not happen", not "this object is finished".
    Defunct,
}

/// Upstream `PrimaryPointerGestureRecognizer`'s state, less the timers and the
/// arena: the three transitions and the two guards on them.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrimaryPointerTracking {
    state: GestureRecognizerState,
    primary: Option<i64>,
}

impl PrimaryPointerTracking {
    pub fn new() -> PrimaryPointerTracking {
        PrimaryPointerTracking::default()
    }

    pub fn state(&self) -> GestureRecognizerState {
        self.state
    }

    pub fn primary_pointer(&self) -> Option<i64> {
        self.primary
    }

    /// Upstream's `addAllowedPointer`, whose whole body is inside
    /// `if (state == GestureRecognizerState.ready)`.
    ///
    /// **A second finger arriving while the first is being watched does not
    /// become primary**, and does not restart the attempt. That guard is what
    /// makes "primary" mean anything: without it the recogniser would follow
    /// whichever pointer touched down most recently, and a two-finger touch
    /// would silently retarget the gesture.
    ///
    /// Returns whether this pointer was taken as the primary one.
    pub fn add_allowed_pointer(&mut self, pointer: i64) -> bool {
        if self.state != GestureRecognizerState::Ready {
            return false;
        }
        self.state = GestureRecognizerState::Possible;
        self.primary = Some(pointer);
        true
    }

    /// Upstream's `rejectGesture`, guarded on **both** the pointer and the
    /// state: `if (pointer == primaryPointer && state == possible)`.
    ///
    /// Another pointer losing the arena says nothing about this gesture, and a
    /// recogniser already `Defunct` has nothing left to give up.
    pub fn reject(&mut self, pointer: i64) -> bool {
        if self.primary != Some(pointer) || self.state != GestureRecognizerState::Possible {
            return false;
        }
        self.state = GestureRecognizerState::Defunct;
        true
    }

    /// Upstream's `didStopTrackingLastPointer`, which opens with
    /// `assert(state != GestureRecognizerState.ready)`.
    ///
    /// You cannot stop tracking what you never started, so this reports the
    /// assert rather than quietly doing nothing -- and it returns to `Ready`
    /// from `Defunct` as well as from `Possible`, which is what keeps a
    /// rejected recogniser usable for the next touch.
    pub fn stop_tracking_last_pointer(&mut self) -> bool {
        if self.state == GestureRecognizerState::Ready {
            return false;
        }
        self.state = GestureRecognizerState::Ready;
        self.primary = None;
        true
    }
}

#[cfg(test)]
mod primary_pointer_tests {
    use super::{GestureRecognizerState, PrimaryPointerTracking};

    #[test]
    fn a_second_finger_does_not_become_the_primary_one() {
        // Upstream's whole addAllowedPointer body sits inside `if (state ==
        // ready)`. Without that guard the recogniser would follow whichever
        // pointer touched down last, and a two-finger touch would retarget the
        // gesture without saying so.
        let mut tracking = PrimaryPointerTracking::new();
        assert!(tracking.add_allowed_pointer(1));
        assert_eq!(tracking.primary_pointer(), Some(1));
        assert_eq!(tracking.state(), GestureRecognizerState::Possible);

        assert!(!tracking.add_allowed_pointer(2), "the second is not taken");
        assert_eq!(tracking.primary_pointer(), Some(1), "and the first is kept");
    }

    #[test]
    fn only_the_primary_pointer_can_make_it_defunct() {
        // Another pointer losing the arena says nothing about this gesture.
        let mut tracking = PrimaryPointerTracking::new();
        tracking.add_allowed_pointer(1);
        assert!(!tracking.reject(2), "a stranger's rejection");
        assert_eq!(tracking.state(), GestureRecognizerState::Possible);
        assert!(tracking.reject(1));
        assert_eq!(tracking.state(), GestureRecognizerState::Defunct);
    }

    #[test]
    fn and_rejecting_twice_changes_nothing() {
        // A recogniser already defunct has nothing left to give up.
        let mut tracking = PrimaryPointerTracking::new();
        tracking.add_allowed_pointer(1);
        assert!(tracking.reject(1));
        assert!(!tracking.reject(1));
        assert_eq!(tracking.state(), GestureRecognizerState::Defunct);
    }

    #[test]
    fn defunct_is_the_end_of_the_attempt_and_not_of_the_recogniser() {
        // didStopTrackingLastPointer returns to ready from defunct as well as
        // from possible, which is what keeps a rejected recogniser usable for
        // the next touch.
        let mut tracking = PrimaryPointerTracking::new();
        tracking.add_allowed_pointer(1);
        tracking.reject(1);
        assert!(tracking.stop_tracking_last_pointer());
        assert_eq!(tracking.state(), GestureRecognizerState::Ready);
        assert_eq!(tracking.primary_pointer(), None);

        // And it really is usable: a new pointer is taken as primary again.
        assert!(tracking.add_allowed_pointer(7));
        assert_eq!(tracking.primary_pointer(), Some(7));
    }

    #[test]
    fn and_it_comes_back_from_possible_too() {
        let mut tracking = PrimaryPointerTracking::new();
        tracking.add_allowed_pointer(1);
        assert_eq!(tracking.state(), GestureRecognizerState::Possible);
        assert!(tracking.stop_tracking_last_pointer());
        assert_eq!(tracking.state(), GestureRecognizerState::Ready);
    }

    #[test]
    fn you_cannot_stop_tracking_what_you_never_started() {
        // Upstream asserts `state != ready` here. Reported rather than
        // silently ignored, so a caller that got its order wrong finds out.
        let mut tracking = PrimaryPointerTracking::new();
        assert_eq!(tracking.state(), GestureRecognizerState::Ready);
        assert!(!tracking.stop_tracking_last_pointer());
    }

    #[test]
    fn a_fresh_recogniser_is_watching_nothing() {
        let tracking = PrimaryPointerTracking::new();
        assert_eq!(tracking.state(), GestureRecognizerState::Ready);
        assert_eq!(tracking.primary_pointer(), None);
        assert_eq!(
            GestureRecognizerState::default(),
            GestureRecognizerState::Ready
        );
    }
}
