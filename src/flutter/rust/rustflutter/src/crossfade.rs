//! Swapping one child for another over time -- ports of upstream's
//! `widgets/animated_cross_fade.dart`, `widgets/animated_switcher.dart` and
//! `widgets/fade_in_image.dart`.
//!
//! All three do the same thing and differ in what they know. A cross-fade is
//! told **both** children and which to show; a switcher is told **one** child
//! and works out for itself whether it is a new one; a fade-in image is a
//! switcher whose two children are a placeholder and the real thing, and which
//! knows when the second has arrived.
//!
//! The recurring decision is what happens to the child on its way **out**, and
//! the three do **not** answer it the same way, which is worth knowing before
//! reaching for one of them.
//!
//! `AnimatedCrossFade` takes the outgoing child out of everything except the
//! paint: no taps, no semantics, tickers off. A reader watching that fade
//! cannot tap a button that is halfway gone and never hears it read out
//! alongside the one replacing it -- see
//! [`AnimatedCrossFade::bottom_treatment`].
//!
//! `AnimatedSwitcher` does **none** of that. Its outgoing children stay in the
//! stack as ordinary widgets, still hit-testable and still announced, and the
//! class's own documentation does not mention it. That is not an oversight to
//! be tidied up here: a switcher is given one child at a time and does not know
//! that the thing leaving and the thing arriving are two versions of the same
//! thing, so it has no grounds for silencing either. A caller who wants the
//! cross-fade's behaviour has to say so.
//!
//! `FadeInImage` sidesteps the question. Both of its images are built with
//! `excludeFromSemantics: true` and one `Semantics` node is put around the
//! pair, so there is never a second announcement to suppress.

use crate::framework::{AnyWidget, StatefulComponent};

/// Upstream `CrossFadeState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossFadeState {
    #[default]
    ShowFirst,
    ShowSecond,
}

/// Which of the two children is on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossFadeLayers {
    /// The child fading **in**, drawn on top.
    pub top: CrossFadeState,
    /// The child fading **out**, drawn underneath.
    pub bottom: CrossFadeState,
}

/// What a layer gets while the fade runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerTreatment {
    pub tickers_enabled: bool,
    pub ignores_pointer: bool,
    pub excludes_semantics: bool,
    pub excludes_focus: bool,
}

/// Upstream `AnimatedCrossFade`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatedCrossFade {
    pub state: CrossFadeState,
    pub duration_micros: i64,
    pub reverse_duration_micros: Option<i64>,
    /// Upstream's `excludeBottomFocus`, **true** by default.
    pub exclude_bottom_focus: bool,
}

impl Default for AnimatedCrossFade {
    fn default() -> AnimatedCrossFade {
        AnimatedCrossFade::new(CrossFadeState::ShowFirst, 200_000)
    }
}

impl AnimatedCrossFade {
    pub fn new(state: CrossFadeState, duration_micros: i64) -> AnimatedCrossFade {
        AnimatedCrossFade {
            state,
            duration_micros,
            reverse_duration_micros: None,
            exclude_bottom_focus: true,
        }
    }

    /// Which child is on top, from the controller's direction.
    ///
    /// The controller runs **forward towards the second child**, so
    /// `showFirst` reverses it. That is why the two children are not
    /// symmetrical in the code even though they are in the API: one of them is
    /// where zero is.
    pub fn layers(&self, forward_or_completed: bool) -> CrossFadeLayers {
        if forward_or_completed {
            CrossFadeLayers {
                top: CrossFadeState::ShowSecond,
                bottom: CrossFadeState::ShowFirst,
            }
        } else {
            CrossFadeLayers {
                top: CrossFadeState::ShowFirst,
                bottom: CrossFadeState::ShowSecond,
            }
        }
    }

    /// Upstream's treatment of the **top** child: everything on.
    ///
    /// The comment on its semantics is explicit -- "always publish semantics
    /// for the widget that's fading in" -- and its tickers stay enabled
    /// unconditionally, because the thing arriving is the thing the reader is
    /// about to interact with.
    pub fn top_treatment(&self) -> LayerTreatment {
        LayerTreatment {
            tickers_enabled: true,
            ignores_pointer: false,
            excludes_semantics: false,
            excludes_focus: false,
        }
    }

    /// Upstream's treatment of the **bottom** child, and the asymmetry is the
    /// design.
    ///
    /// It **always ignores pointers** and **always excludes semantics** --
    /// upstream's comment is "always exclude the semantics of the widget
    /// that's fading out", so a screen reader never reads two versions of the
    /// same thing at once. Its tickers run **only while the fade is running**,
    /// which stops a settled cross-fade paying for an invisible subtree's
    /// animations for as long as it exists.
    ///
    /// Focus is the one part the caller controls, and it defaults to excluded.
    pub fn bottom_treatment(&self, animating: bool) -> LayerTreatment {
        LayerTreatment {
            tickers_enabled: animating,
            ignores_pointer: true,
            excludes_semantics: true,
            excludes_focus: self.exclude_bottom_focus,
        }
    }

    /// Upstream's `defaultLayoutBuilder`, which positions the **bottom** child
    /// with `left/top/right` set and the top child with none of them.
    ///
    /// So the outgoing child is stretched to the incoming one's width while
    /// the incoming one sizes itself. The stack takes its size from the top
    /// child, which is what makes the whole thing grow towards the new
    /// content rather than jumping to it.
    pub fn bottom_is_stretched_horizontally() -> bool {
        true
    }

    /// Whether the widget wraps its result in an `AnimatedSize`.
    ///
    /// It always does, and that is most of why the class exists: fading two
    /// differently-sized children into each other without animating the size
    /// makes the surrounding layout jump on the first frame.
    pub fn animates_size(&self) -> bool {
        true
    }

    /// The widget: the two children stacked, one fading into the other.
    ///
    /// Everything above this had been ported and had **no consumer** --
    /// `layers`, the two treatments, the stretch rule -- so the policy was
    /// complete and nothing in the crate could actually cross-fade anything.
    ///
    /// `progress` runs 0 to 1 in the direction the state names, which is what
    /// upstream takes from its `AnimationController`. There is no controller
    /// here; the caller drives it, the way [`crate::controls::Spinner`] and
    /// [`crate::tabs::TabBarView`] are driven.
    ///
    /// The asymmetry is all from [`AnimatedCrossFade::bottom_treatment`]: the
    /// outgoing child ignores pointers and leaves the semantics walk, so a
    /// reader never meets two versions of the same thing and a finger never
    /// lands on the one that is leaving.
    ///
    /// **Not** wrapped in an animated size, which
    /// [`AnimatedCrossFade::animates_size`] says upstream always does: this
    /// crate has no `AnimatedSize`, so the stack takes the top child's size
    /// and the surrounding layout moves in one step rather than easing. The
    /// rule is left saying `true` because it is upstream's, and this is the
    /// piece that is missing rather than a decision taken here.
    pub fn widget(
        &self,
        progress: f32,
        first: crate::framework::AnyWidget,
        second: crate::framework::AnyWidget,
    ) -> crate::framework::AnyWidget {
        let cross = *self;
        let progress = progress.clamp(0.0, 1.0);
        // Upstream's `_controller.status.isForwardOrCompleted`, which for a
        // caller-driven progress is "on the way to, or arrived at, the second".
        let forward = matches!(self.state, CrossFadeState::ShowSecond);
        let animating = progress > 0.0 && progress < 1.0;
        let layers = self.layers(forward);
        crate::framework::many(vec![first, second], move |mut rendered| {
            let second = rendered.pop().expect("two children");
            let first = rendered.pop().expect("two children");
            let pick = |which: CrossFadeState| match which {
                CrossFadeState::ShowFirst => first.clone(),
                CrossFadeState::ShowSecond => second.clone(),
            };
            // The top child is the one arriving, so it takes the progress; the
            // bottom is the one leaving, and takes what is left.
            let (top_opacity, bottom_opacity) = if forward {
                (progress, 1.0 - progress)
            } else {
                (1.0 - progress, progress)
            };
            let bottom = cross.bottom_treatment(animating);
            let mut leaving: crate::render::BoxedRender = crate::render::RenderRef::new(
                crate::render::RenderOpacity::new(bottom_opacity, pick(layers.bottom)),
            );
            if bottom.ignores_pointer {
                leaving =
                    crate::render::RenderRef::new(crate::render::RenderIgnorePointer::new(leaving));
            }
            if bottom.excludes_semantics {
                leaving = crate::render::RenderRef::new(
                    crate::semantics_markers::ExcludeSemantics::new().wrapping(leaving),
                );
            }
            crate::render::RenderRef::new(
                crate::render::RenderStack::new()
                    // The outgoing child is stretched to the incoming one's
                    // width while the incoming one sizes itself -- upstream's
                    // `defaultLayoutBuilder`, and what makes the box grow
                    // towards the new content instead of jumping to it.
                    .push_positioned(
                        leaving,
                        crate::render::StackPosition {
                            left: Some(0.0),
                            top: Some(0.0),
                            right: Some(0.0),
                            ..Default::default()
                        },
                    )
                    .push(crate::render::RenderOpacity::new(
                        top_opacity,
                        pick(layers.top),
                    )),
            )
        })
    }
}

/// Upstream `AnimatedSwitcher`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatedSwitcher {
    pub duration_micros: i64,
    pub reverse_duration_micros: Option<i64>,
    /// Upstream's `switchInCurve`, `Curves.linear` by default.
    pub switch_in_curve: crate::animation::Curve,
    /// Upstream's `switchOutCurve`, also `Curves.linear` by default.
    ///
    /// Reached less often than it looks: see
    /// [`AnimatedSwitcher::curve_a_child_leaves_on`].
    pub switch_out_curve: crate::animation::Curve,
}

/// A child, reduced to what `Widget.canUpdate` compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitcherChild {
    /// Upstream compares `runtimeType`.
    pub widget_type: &'static str,
    pub key: Option<u64>,
}

impl SwitcherChild {
    pub fn new(widget_type: &'static str) -> SwitcherChild {
        SwitcherChild {
            widget_type,
            key: None,
        }
    }

    pub fn keyed(widget_type: &'static str, key: u64) -> SwitcherChild {
        SwitcherChild {
            widget_type,
            key: Some(key),
        }
    }

    /// Upstream's `Widget.canUpdate`: same runtime type **and** same key.
    pub fn can_update(&self, other: &SwitcherChild) -> bool {
        self.widget_type == other.widget_type && self.key == other.key
    }
}

/// What a rebuild did to the switcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchOutcome {
    /// A new entry was added and the old one started fading out.
    Switched,
    /// The existing entry was updated in place -- no animation.
    UpdatedInPlace,
    /// There was nothing before and nothing now.
    Nothing,
}

impl Default for AnimatedSwitcher {
    fn default() -> AnimatedSwitcher {
        AnimatedSwitcher::new(200_000)
    }
}

impl AnimatedSwitcher {
    pub fn new(duration_micros: i64) -> AnimatedSwitcher {
        AnimatedSwitcher {
            duration_micros,
            reverse_duration_micros: None,
            switch_in_curve: crate::animation::Curve::Linear,
            switch_out_curve: crate::animation::Curve::Linear,
        }
    }

    /// Upstream's `didUpdateWidget` decision, and the trap it is famous for.
    ///
    /// A switch happens only when the new child **cannot update** the old one
    /// -- a different runtime type or a different key. Two `Text` widgets with
    /// different strings and no keys can update each other, so **nothing
    /// animates**: the text simply changes.
    ///
    /// That surprises everyone once, and upstream's documentation says so at
    /// length. The fix is a key, which is why nearly every example in the
    /// wild has one.
    pub fn decide(old: Option<SwitcherChild>, new: Option<SwitcherChild>) -> SwitchOutcome {
        AnimatedSwitcher::outcome(old.is_some(), new.is_some(), || {
            new.expect("checked").can_update(&old.expect("checked"))
        })
    }

    /// The same decision over real widgets, which is the one the widget below
    /// actually makes.
    ///
    /// [`AnimatedSwitcher::decide`] models a child as a type name and a key
    /// because that is what upstream's `Widget.canUpdate` compares; an
    /// [`AnyWidget`] answers the question about itself, so this asks it
    /// directly rather than describing it a second time.
    pub fn decide_widgets(old: Option<&AnyWidget>, new: Option<&AnyWidget>) -> SwitchOutcome {
        AnimatedSwitcher::outcome(old.is_some(), new.is_some(), || {
            new.expect("checked").can_update(old.expect("checked"))
        })
    }

    /// The rule the two entry points above share, so it exists once.
    fn outcome(has_old: bool, has_new: bool, can_update: impl FnOnce() -> bool) -> SwitchOutcome {
        match (has_old, has_new) {
            (false, false) => SwitchOutcome::Nothing,
            (true, false) | (false, true) => SwitchOutcome::Switched,
            (true, true) => {
                if can_update() {
                    SwitchOutcome::UpdatedInPlace
                } else {
                    SwitchOutcome::Switched
                }
            }
        }
    }

    /// Upstream's `defaultTransitionBuilder`, which keys the fade **by the
    /// child's key**.
    ///
    /// That is what stops the transition itself being reused across a switch:
    /// two children with different keys get different `FadeTransition`s, and
    /// the outgoing one keeps its own opacity animation while the incoming one
    /// starts a fresh one.
    pub fn transition_key(child: &SwitcherChild) -> Option<u64> {
        child.key
    }

    /// Upstream's `defaultLayoutBuilder`: previous children first, then the
    /// current one -- so the arriving child is painted **on top** -- all
    /// centred on each other.
    pub fn paint_order(previous: &[u64], current: Option<u64>) -> Vec<u64> {
        let mut order = previous.to_vec();
        if let Some(current) = current {
            order.push(current);
        }
        order
    }

    /// Upstream reverses the outgoing entry's controller rather than starting
    /// a separate fade-out, which is what makes `reverseDuration` and
    /// `switchOutCurve` apply to it: it is the same animation running
    /// backwards.
    pub fn outgoing_runs_in_reverse() -> bool {
        true
    }

    /// Which curve a child that is being sent away actually leaves on, which
    /// is **not** always `switchOutCurve`.
    ///
    /// The entry's fade is a `CurvedAnimation(curve: switchInCurve,
    /// reverseCurve: switchOutCurve)` over the entry's own controller, and
    /// `CurvedAnimation` will not change curve under a value that is on
    /// screen: its `_curveDirection` is "only reset when we hit the beginning
    /// or the end of the timeline to avoid discontinuities". So a child that
    /// **had fully arrived** before it was replaced leaves on
    /// `switchOutCurve`, and a child **interrupted on its way in** keeps
    /// running `switchInCurve` backwards.
    ///
    /// Worth stating rather than leaving to be discovered: the second case is
    /// the common one in a list that changes twice in quick succession, and a
    /// reader looking for `switchOutCurve` in the output will not find it
    /// there.
    pub fn curve_a_child_leaves_on(&self, had_fully_arrived: bool) -> crate::animation::Curve {
        crate::animation::curve_for_direction(
            if had_fully_arrived {
                // Reaching the end cleared the direction, so the reversal
                // starts a fresh one.
                crate::animation::AnimationStatus::Reverse
            } else {
                // Still latched to the way in.
                crate::animation::AnimationStatus::Forward
            },
            self.switch_in_curve,
            Some(self.switch_out_curve),
        )
    }

    /// The widget: hand it one child at a time and it fades between them.
    ///
    /// Everything above this was policy with no consumer. The reason it stayed
    /// that way one round longer than [`AnimatedCrossFade::widget`] is that a
    /// switcher **cannot** be a function of its arguments: it is given one
    /// child and has to compare it with the one it was given last time, so it
    /// needs somewhere to remember. That is [`AnimatedSwitcherState`], and it
    /// is why this is a [`StatefulComponent`] where the cross-fade is not.
    ///
    /// `duration_micros` is the fade in; the fade out is
    /// `reverse_duration_micros` when set, and the same duration otherwise --
    /// upstream's `reverseDuration ?? duration`. Both curves default to linear
    /// upstream, which is what this does, so nothing is missing for the
    /// default case.
    ///
    /// A child of `None` is upstream's `child: null`: whatever is showing
    /// fades out and nothing replaces it.
    pub fn widget(&self, child: Option<AnyWidget>) -> AnyWidget {
        crate::framework::stateful(Switching {
            switcher: *self,
            child,
            key: None,
        })
    }

    /// [`AnimatedSwitcher::widget`] with a key, for two switchers that would
    /// otherwise sit in the same position and share each other's memory.
    pub fn keyed_widget(&self, key: u64, child: Option<AnyWidget>) -> AnyWidget {
        crate::framework::stateful(Switching {
            switcher: *self,
            child,
            key: Some(key),
        })
    }
}

/// One child on its way out, and where its fade has got to.
///
/// `number` is upstream's `_childNumber`, the counter it wraps each entry's
/// transition in a `KeyedSubtree` with. It is not decoration: without it the
/// children of the stack are matched by position, so an entry dropping off the
/// front would hand its element to the entry behind it and a half-faded child
/// would jump.
struct OutgoingChild {
    number: u64,
    child: AnyWidget,
    /// Where the reversing controller started, which is **not** always 1.
    ///
    /// [`AnimatedSwitcher::outgoing_runs_in_reverse`] is the whole reason:
    /// upstream reverses the entry's own controller, so a child interrupted
    /// halfway in starts its way out from halfway, not from opaque.
    from_opacity: f32,
    /// The frame the reversal began, stamped by the next `advance`.
    started_micros: Option<i64>,
    /// The curve this child leaves on, fixed the moment it was sent away by
    /// [`AnimatedSwitcher::curve_a_child_leaves_on`]. Kept on the entry rather
    /// than worked out per frame because the answer depends on how far the
    /// child had got, which nothing remembers once it has gone.
    curve: crate::animation::Curve,
}

/// What a switcher remembers between builds.
///
/// The current child is **not** in here -- it is on the widget, where the
/// caller put it. Upstream keeps a `_currentEntry` because its child has to
/// carry an `AnimationController` that outlives any one build; here the only
/// thing about the current child that outlives a build is where its fade has
/// got to, which is two numbers.
#[derive(Default)]
pub struct AnimatedSwitcherState {
    outgoing: Vec<OutgoingChild>,
    /// The current child's `_childNumber`.
    current_number: u64,
    next_number: u64,
    /// When the current child's fade in began, or `None` once it is fully in.
    ///
    /// Starts `None`, which is upstream's `_addEntryForNewChild(animate:
    /// false)` in `initState`: the first child a switcher is ever given is
    /// simply there. Nothing fades in from nowhere on the first frame.
    current_started_micros: Option<i64>,
    /// Set when a switch is decided, cleared when the next frame stamps it.
    current_pending: bool,
    /// The frame clock, so `build` can evaluate without being handed the time.
    now_micros: i64,
}

impl AnimatedSwitcherState {
    /// Where the current child's **controller** has got to, before any curve.
    fn arriving_controller_value(&self, duration_micros: i64) -> f32 {
        let Some(started) = self.current_started_micros else {
            // Settled, or never faded in at all.
            return 1.0;
        };
        if duration_micros <= 0 {
            return 1.0;
        }
        (((self.now_micros - started).max(0) as f32) / duration_micros as f32).clamp(0.0, 1.0)
    }

    /// What is actually painted: that value through `switchInCurve`.
    fn arriving_opacity(&self, duration_micros: i64, curve: crate::animation::Curve) -> f32 {
        curve.transform(self.arriving_controller_value(duration_micros))
    }
}

impl OutgoingChild {
    /// Where this child's **controller** has got to at `now_micros`, before
    /// any curve.
    ///
    /// A reversing controller falls at one over its duration per microsecond,
    /// which is why this subtracts rather than interpolating: an entry that
    /// began its exit from 0.4 reaches zero in 40% of the reverse duration,
    /// not in all of it.
    fn controller_value(&self, now_micros: i64, reverse_micros: i64) -> f32 {
        let Some(started) = self.started_micros else {
            return self.from_opacity;
        };
        if reverse_micros <= 0 {
            return 0.0;
        }
        let fallen = (now_micros - started).max(0) as f32 / reverse_micros as f32;
        (self.from_opacity - fallen).clamp(0.0, 1.0)
    }

    /// What is actually painted: the controller's value through this child's
    /// curve.
    fn opacity(&self, now_micros: i64, reverse_micros: i64) -> f32 {
        self.curve
            .transform(self.controller_value(now_micros, reverse_micros))
    }
}

/// The component behind [`AnimatedSwitcher::widget`].
struct Switching {
    switcher: AnimatedSwitcher,
    child: Option<AnyWidget>,
    key: crate::framework::Key,
}

impl Switching {
    fn reverse_micros(&self) -> i64 {
        // Upstream's `reverseDuration ?? duration`.
        self.switcher
            .reverse_duration_micros
            .unwrap_or(self.switcher.duration_micros)
    }
}

impl StatefulComponent for Switching {
    type State = AnimatedSwitcherState;

    fn key(&self) -> crate::framework::Key {
        self.key
    }

    /// Upstream's `didUpdateWidget`, which is where the whole class lives.
    ///
    /// The decision is [`AnimatedSwitcher::decide_widgets`] and nothing else:
    /// a child that *can update* the old one is the same child with new
    /// contents, and nothing animates. That is the trap the ported `decide`
    /// already documented, and this is the code it was documenting.
    ///
    /// Following [`crate::implicit::Animated`], the switch is **decided** here
    /// and **stamped** by the next `advance`, because the frame clock arrives
    /// there and not here.
    fn did_update_widget(&self, old: &Self, state: &mut Self::State) {
        if AnimatedSwitcher::decide_widgets(old.child.as_ref(), self.child.as_ref())
            != SwitchOutcome::Switched
        {
            return;
        }
        if let Some(leaving) = old.child.clone() {
            // The controller's value, not the painted one: upstream reverses
            // the controller, and the curve is read off it afterwards.
            let from_opacity = state.arriving_controller_value(old.switcher.duration_micros);
            state.outgoing.push(OutgoingChild {
                number: state.current_number,
                child: leaving,
                from_opacity,
                started_micros: None,
                curve: old.switcher.curve_a_child_leaves_on(from_opacity >= 1.0),
            });
        }
        state.next_number += 1;
        state.current_number = state.next_number;
        state.current_pending = true;
        state.current_started_micros = None;
    }

    fn advance(&self, state: &mut Self::State, frame_time_micros: i64) -> bool {
        state.now_micros = frame_time_micros;
        for entry in &mut state.outgoing {
            if entry.started_micros.is_none() {
                entry.started_micros = Some(frame_time_micros);
            }
        }
        if state.current_pending {
            state.current_pending = false;
            state.current_started_micros = Some(frame_time_micros);
        }

        let reverse_micros = self.reverse_micros();
        let before = state.outgoing.len();
        // Upstream removes an entry from `_outgoingEntries` when its animation
        // reports **dismissed**, which is the controller's status and not the
        // curved value. The two part company for any curve that reaches zero
        // early, and dropping an entry a curve has merely made invisible would
        // take it out of the tree while its controller was still running.
        let now = state.now_micros;
        state
            .outgoing
            .retain(|entry| entry.controller_value(now, reverse_micros) > 0.0);
        let dropped = state.outgoing.len() != before;

        let mut wants_another = !state.outgoing.is_empty() || dropped;
        if let Some(started) = state.current_started_micros {
            // The frame that lands still has to be drawn, so this asks for one
            // more and settles at the same time; the frame after it is idle.
            wants_another = true;
            if frame_time_micros - started >= self.switcher.duration_micros {
                state.current_started_micros = None;
            }
        }
        wants_another
    }

    /// Upstream's `build`: `layoutBuilder(currentTransition, outgoing)`, which
    /// by default is a `Stack` centred on itself with the previous children
    /// underneath -- see [`AnimatedSwitcher::paint_order`].
    ///
    /// The outgoing children are painted and otherwise left alone: unlike the
    /// cross-fade, a switcher does not take them out of the hit test or the
    /// semantics walk. See the module comment for why that is upstream's
    /// answer and not an omission here.
    fn build(
        &self,
        state: &Self::State,
        _handle: crate::framework::StateHandle<Self::State>,
        _context: &mut crate::framework::BuildContext,
    ) -> AnyWidget {
        let reverse_micros = self.reverse_micros();
        let mut entries: Vec<(u64, AnyWidget, f32)> = state
            .outgoing
            .iter()
            .map(|entry| {
                (
                    entry.number,
                    entry.child.clone(),
                    entry.opacity(state.now_micros, reverse_micros),
                )
            })
            .collect();
        if let Some(child) = &self.child {
            entries.push((
                state.current_number,
                child.clone(),
                state
                    .arriving_opacity(self.switcher.duration_micros, self.switcher.switch_in_curve),
            ));
        }

        let previous: Vec<u64> = state.outgoing.iter().map(|entry| entry.number).collect();
        let order = AnimatedSwitcher::paint_order(
            &previous,
            self.child.as_ref().map(|_| state.current_number),
        );
        let mut children = Vec::with_capacity(order.len());
        for number in order {
            let (_, child, opacity) = entries
                .iter()
                .find(|(candidate, _, _)| *candidate == number)
                .expect("every number in the paint order is an entry");
            let opacity = *opacity;
            // Upstream's `KeyedSubtree.wrap(builder(child, animation),
            // _childNumber)`, in one call: the fade is the wrapper and the
            // entry's number is the wrapper's key, so an entry keeps its
            // element while the ones in front of it come and go -- and the
            // child's own key, which the caller may be relying on, is left
            // alone underneath.
            children.push(crate::framework::keyed_single(
                number,
                child.clone(),
                move |child| crate::render::RenderOpacity::new(opacity, child),
            ));
        }

        crate::framework::many(children, move |rendered| {
            let mut stack = crate::render::RenderStack::new()
                // Upstream's `defaultLayoutBuilder` centres them on each
                // other, so a shorter child leaving and a taller one arriving
                // stay on the same middle instead of both hanging from the top.
                .with_alignment(crate::render::Alignment::CENTER);
            for child in rendered {
                stack = stack.push(child);
            }
            stack
        })
    }
}

/// Upstream `FadeInImage`: a placeholder that gives way to the real image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeInImage {
    /// Upstream's default: 300ms out.
    pub fade_out_micros: i64,
    /// Upstream's default: 700ms in.
    pub fade_in_micros: i64,
}

impl Default for FadeInImage {
    fn default() -> FadeInImage {
        FadeInImage::new()
    }
}

/// Which of the two images is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FadeInPhase {
    /// The placeholder, while the real image loads.
    #[default]
    Placeholder,
    /// The placeholder fading out.
    FadingOut,
    /// The real image fading in.
    FadingIn,
    /// The real image, settled.
    Complete,
}

impl FadeInImage {
    pub const DEFAULT_FADE_OUT_MICROS: i64 = 300_000;
    pub const DEFAULT_FADE_IN_MICROS: i64 = 700_000;

    pub fn new() -> FadeInImage {
        FadeInImage {
            fade_out_micros: Self::DEFAULT_FADE_OUT_MICROS,
            fade_in_micros: Self::DEFAULT_FADE_IN_MICROS,
        }
    }

    /// The two durations are **deliberately unequal**, and the ratio is the
    /// point: the placeholder leaves in 300ms and the image arrives over
    /// 700ms.
    ///
    /// A symmetric cross-fade would show both at half strength through the
    /// middle, which on a photograph over a grey box reads as a smear. Letting
    /// the placeholder go first and the image come in slowly means the reader
    /// sees the image resolve rather than two pictures overlapping.
    pub fn fade_out_is_quicker(&self) -> bool {
        self.fade_out_micros < self.fade_in_micros
    }

    /// Which phase a given moment is in, measured from the real image
    /// arriving.
    pub fn phase_at(&self, loaded: bool, micros_since_loaded: i64) -> FadeInPhase {
        if !loaded {
            return FadeInPhase::Placeholder;
        }
        if micros_since_loaded < self.fade_out_micros {
            return FadeInPhase::FadingOut;
        }
        if micros_since_loaded < self.fade_out_micros + self.fade_in_micros {
            return FadeInPhase::FadingIn;
        }
        FadeInPhase::Complete
    }

    /// Upstream's `FadeInImage.memoryNetwork` and `.assetNetwork` exist for
    /// one reason worth stating: the **placeholder must not itself be a
    /// network image**, or the widget would be waiting on two downloads to
    /// show the reader anything.
    pub fn placeholder_must_be_local() -> bool {
        true
    }
}

/// Upstream `Icon`: a glyph from an icon font.
///
/// It is a font glyph rather than a picture, which is why it takes a `size`
/// and a `color` and no `fit`: it scales like text because it *is* text, and
/// a font renders at any size without blurring.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Icon {
    /// `None` means "inherit from the ambient `IconTheme`", which is how a
    /// whole toolbar's icons change colour by one line above them. Every field
    /// below is the same three-step chain: the icon, the theme, the fallback.
    pub size: Option<f32>,
    pub color: Option<crate::engine::Color>,
    pub fill: Option<f32>,
    pub weight: Option<f32>,
    pub grade: Option<f32>,
    pub optical_size: Option<f32>,
    /// Upstream's `applyTextScaling` -- see
    /// [`crate::component_themes::ResolvedIcon`] for why it defaults to off.
    pub apply_text_scaling: Option<bool>,
    pub shadows: Option<Vec<crate::painting::BoxShadow>>,
    /// Upstream's `semanticLabel`. Absent by default, and that is right: most
    /// icons sit next to a label that already says what they are, and
    /// announcing both would say it twice.
    pub has_semantic_label: bool,
}

impl Icon {
    /// Upstream's default when no `IconTheme` supplies one.
    pub const DEFAULT_SIZE: f32 = 24.0;

    pub fn new() -> Icon {
        Icon::default()
    }

    /// The size against a theme size handed in, for a caller with no context.
    ///
    /// The fallback here is 24 and not `kDefaultFontSize`, because a caller
    /// passing a theme size explicitly has a theme -- see
    /// [`crate::component_themes::ResolvedIcon`], where the distinction lives.
    pub fn resolved_size(&self, theme_size: Option<f32>) -> f32 {
        self.size.or(theme_size).unwrap_or(Self::DEFAULT_SIZE)
    }

    /// Everything this icon is drawn with, read off the ambient `IconTheme`.
    pub fn resolved(
        &self,
        context: &mut crate::framework::BuildContext,
    ) -> crate::component_themes::ResolvedIcon {
        crate::component_themes::ResolvedIcon::of(context, self)
    }

    /// Upstream asserts `fill` is between 0 and 1 and `weight` is above zero
    /// -- variable-font axes with real ranges, not free numbers.
    pub fn axes_are_valid(&self) -> bool {
        self.fill.is_none_or(|fill| (0.0..=1.0).contains(&fill))
            && self.weight.is_none_or(|weight| weight > 0.0)
    }
}

/// Upstream `ImageIcon`: the same shape, from an image instead of a font.
///
/// It exists for icons a font cannot carry -- a multicoloured logo, an avatar
/// -- and it takes the `IconTheme`'s size and colour so it lines up with the
/// font icons beside it. The colour is applied as a **blend**, which is why an
/// `ImageIcon` of a photograph comes out tinted rather than replaced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageIcon {
    pub icon: Icon,
}

impl ImageIcon {
    pub fn new() -> ImageIcon {
        ImageIcon::default()
    }

    /// It follows the same theme resolution as [`Icon`], which is the whole
    /// point of the class.
    pub fn resolved_size(&self, theme_size: Option<f32>) -> f32 {
        self.icon.resolved_size(theme_size)
    }
}

#[cfg(test)]
mod tests {

    /// The text that reached the canvas, as `(text, alpha)` in paint order,
    /// where the alpha is the **opacity layer** around it and not the text's
    /// own colour.
    ///
    /// Worth spelling out, because reading the paragraph's colour instead --
    /// which is the obvious thing to do, and what this file did for a round --
    /// reports 255 through every fade there has ever been. A paragraph's
    /// colour is its own and an opacity layer never touches it.
    ///
    /// Every fade in this file wraps each child in exactly one opacity layer,
    /// so the alpha pushed immediately before a paragraph is that paragraph's.
    /// A paragraph with no layer in front of it is fully opaque, because
    /// `RenderOpacity` skips the layer entirely at one -- and skips the whole
    /// child at zero, which is why a child that has finished leaving is
    /// absent from this list rather than present at nought.
    fn text_alphas(drawn: Vec<crate::engine_test_stubs::Drawn>) -> Vec<(String, u32)> {
        let mut alphas = Vec::new();
        let mut pending: Option<u32> = None;
        for call in drawn {
            match call {
                crate::engine_test_stubs::Drawn::OpacityLayer { alpha } => {
                    pending = Some(alpha as u32);
                }
                crate::engine_test_stubs::Drawn::Paragraph { text, .. } => {
                    alphas.push((text, pending.take().unwrap_or(255)));
                }
                _ => {}
            }
        }
        alphas
    }

    /// What the two children were drawn at, as `(label, alpha)`.
    fn faded(cross: AnimatedCrossFade, progress: f32) -> Vec<(String, u32)> {
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(cross.widget(
            progress,
            crate::framework::leaf(|| crate::widgets::Text::new("first".to_string())),
            crate::framework::leaf(|| crate::widgets::Text::new("second".to_string())),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        let mut layers = crate::engine::LayerTree::new(400, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(400.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        text_alphas(crate::engine_test_stubs::drawn())
    }

    #[test]
    fn one_child_fades_in_while_the_other_fades_out() {
        // Everything this needed had been ported and had no consumer: the
        // layer order, both treatments, the stretch rule. The policy was
        // complete and nothing in the crate could cross-fade anything.
        let cross = AnimatedCrossFade::new(CrossFadeState::ShowSecond, 200_000);

        // At either end only one of them is drawn at all: a fully transparent
        // layer is not painted, which is the opacity's own doing and worth
        // knowing -- the alphas cannot be compared where one of the two never
        // reaches the canvas.
        let start: Vec<String> = faded(cross, 0.0)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(start, vec!["first".to_string()], "at rest, before the fade");

        let finish: Vec<String> = faded(cross, 1.0)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(finish, vec!["second".to_string()], "at rest, after it");

        // And halfway both are on the canvas -- which is what makes it a
        // cross-fade rather than a swap.
        let middle = faded(cross, 0.5);
        let leaving = middle
            .iter()
            .find(|(text, _)| text == "first")
            .expect("the leaving child");
        let arriving = middle
            .iter()
            .find(|(text, _)| text == "second")
            .expect("the arriving child");
        // Halfway means halfway: both layers carry about half an alpha.
        // Asserting merely that both are on the canvas would pass at any
        // opacity either of them happened to be given.
        assert!(
            (leaving.1 as i64 - 128).abs() < 24 && (arriving.1 as i64 - 128).abs() < 24,
            "{middle:?}"
        );
    }

    #[test]
    fn the_child_that_is_leaving_is_stretched_to_the_one_arriving() {
        // Upstream's `defaultLayoutBuilder` positions the bottom child with
        // `left/top/right` set and the top child with none of them, so the
        // outgoing child takes the incoming one's width while the incoming one
        // sizes itself. That is what makes the box grow *towards* the new
        // content instead of jumping to it -- and it can only be seen when the
        // two are different sizes.
        const LEAVING: crate::engine::Color = crate::engine::Color(0xffff0000);
        let cross = AnimatedCrossFade::new(CrossFadeState::ShowSecond, 200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(cross.widget(
            0.5,
            crate::framework::leaf(|| {
                crate::widgets::Container::new()
                    .with_size(50.0, 20.0)
                    .with_color(LEAVING)
            }),
            crate::framework::leaf(|| crate::widgets::Container::new().with_size(200.0, 20.0)),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        let mut layers = crate::engine::LayerTree::new(400, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(400.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let (left, right) = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect {
                    left, right, argb, ..
                } if argb == LEAVING.0 => Some((left, right)),
                _ => None,
            })
            .expect("the leaving child was not painted");
        assert_eq!(
            (left, right),
            (0.0, 200.0),
            "the leaving child kept its own width"
        );
    }

    #[test]
    fn the_child_that_is_leaving_leaves_the_readers_way_first() {
        // Upstream's own comment: "always exclude the semantics of the widget
        // that's fading out". Without it a reader meets both versions of the
        // same thing at once, which is worse the more alike they are.
        crate::semantics::set_enabled(true);
        let cross = AnimatedCrossFade::new(CrossFadeState::ShowSecond, 200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(cross.widget(
            0.5,
            crate::framework::leaf(|| crate::widgets::Text::new("first".to_string())),
            crate::framework::leaf(|| crate::widgets::Text::new("second".to_string())),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let said: Vec<String> =
            crate::semantics::flush(crate::render::Size::new(200.0, 200.0), &root)
                .unwrap_or_default()
                .iter()
                .filter(|node| !node.properties.label.is_empty())
                .map(|node| node.properties.label.clone())
                .collect();
        crate::semantics::set_enabled(false);
        assert!(said.iter().any(|words| words == "second"), "{said:?}");
        assert!(
            !said.iter().any(|words| words == "first"),
            "both versions were offered at once: {said:?}"
        );
    }

    #[test]
    fn a_finger_never_lands_on_the_child_that_is_leaving() {
        // The other half of the same asymmetry: the outgoing child ignores
        // pointers however solid it still looks.
        const LEAVING: u64 = 4071;
        let cross = AnimatedCrossFade::new(CrossFadeState::ShowSecond, 200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(cross.widget(
            0.5,
            crate::framework::leaf(|| {
                crate::widgets::Pointer::new(
                    LEAVING,
                    crate::widgets::Container::new().with_size(200.0, 200.0),
                )
            }),
            crate::framework::leaf(|| crate::widgets::Container::new().with_size(200.0, 200.0)),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        let mut result = crate::render::HitTestResult::default();
        crate::render::RenderBox::hit_test(
            &root,
            crate::render::Offset::new(100.0, 100.0),
            &mut result,
        );
        assert!(
            !result.path.iter().any(|entry| entry.target == LEAVING),
            "the outgoing child answered a finger"
        );
    }

    use super::*;

    // -- The cross-fade ------------------------------------------------------

    #[test]
    fn the_controller_runs_forward_towards_the_second_child() {
        // Which is why the two are not symmetrical in the code even though
        // they are in the API: one of them is where zero is.
        let fade = AnimatedCrossFade::default();
        assert_eq!(
            fade.layers(true).top,
            CrossFadeState::ShowSecond,
            "forward means the second is arriving"
        );
        assert_eq!(fade.layers(false).top, CrossFadeState::ShowFirst);
        assert_eq!(fade.layers(true).bottom, CrossFadeState::ShowFirst);
    }

    #[test]
    fn the_child_fading_out_is_never_read_out_alongside_the_one_arriving() {
        // Upstream: "always exclude the semantics of the widget that's fading
        // out".
        let fade = AnimatedCrossFade::default();
        assert!(fade.bottom_treatment(true).excludes_semantics);
        assert!(fade.bottom_treatment(false).excludes_semantics);
        assert!(!fade.top_treatment().excludes_semantics);
    }

    #[test]
    fn a_button_halfway_gone_cannot_be_tapped() {
        let fade = AnimatedCrossFade::default();
        assert!(fade.bottom_treatment(true).ignores_pointer);
        assert!(!fade.top_treatment().ignores_pointer);
    }

    #[test]
    fn a_settled_cross_fade_stops_paying_for_the_hidden_subtrees_animations() {
        // The bottom child's tickers run only while the fade is running.
        let fade = AnimatedCrossFade::default();
        assert!(fade.bottom_treatment(true).tickers_enabled);
        assert!(!fade.bottom_treatment(false).tickers_enabled);
        assert!(
            fade.top_treatment().tickers_enabled,
            "where the top child's always are"
        );
    }

    #[test]
    fn focus_is_the_one_part_of_the_bottom_child_the_caller_controls() {
        let mut fade = AnimatedCrossFade::default();
        assert!(fade.exclude_bottom_focus, "excluded by default");
        assert!(fade.bottom_treatment(true).excludes_focus);

        fade.exclude_bottom_focus = false;
        assert!(!fade.bottom_treatment(true).excludes_focus);
        assert!(
            fade.bottom_treatment(true).excludes_semantics,
            "but semantics stay excluded regardless"
        );
    }

    #[test]
    fn a_cross_fade_always_animates_its_size() {
        // Fading two differently-sized children into each other without it
        // makes the surrounding layout jump on the first frame.
        assert!(AnimatedCrossFade::default().animates_size());
    }

    // -- The switcher --------------------------------------------------------

    #[test]
    fn two_texts_with_no_keys_do_not_animate_at_all() {
        // The trap that surprises everyone once: they can update each other,
        // so the text simply changes.
        let outcome = AnimatedSwitcher::decide(
            Some(SwitcherChild::new("Text")),
            Some(SwitcherChild::new("Text")),
        );
        assert_eq!(outcome, SwitchOutcome::UpdatedInPlace);
    }

    #[test]
    fn a_key_is_what_makes_the_switch_happen() {
        // Which is why nearly every example in the wild has one.
        let outcome = AnimatedSwitcher::decide(
            Some(SwitcherChild::keyed("Text", 1)),
            Some(SwitcherChild::keyed("Text", 2)),
        );
        assert_eq!(outcome, SwitchOutcome::Switched);
    }

    #[test]
    fn a_different_widget_type_switches_without_a_key() {
        let outcome = AnimatedSwitcher::decide(
            Some(SwitcherChild::new("Text")),
            Some(SwitcherChild::new("Icon")),
        );
        assert_eq!(outcome, SwitchOutcome::Switched);
    }

    #[test]
    fn appearing_and_disappearing_both_count_as_a_switch() {
        assert_eq!(
            AnimatedSwitcher::decide(None, Some(SwitcherChild::new("Text"))),
            SwitchOutcome::Switched
        );
        assert_eq!(
            AnimatedSwitcher::decide(Some(SwitcherChild::new("Text")), None),
            SwitchOutcome::Switched
        );
        assert_eq!(AnimatedSwitcher::decide(None, None), SwitchOutcome::Nothing);
    }

    #[test]
    fn the_arriving_child_is_painted_on_top_of_the_ones_leaving() {
        assert_eq!(
            AnimatedSwitcher::paint_order(&[1, 2], Some(3)),
            vec![1, 2, 3]
        );
        assert_eq!(AnimatedSwitcher::paint_order(&[1], None), vec![1]);
    }

    #[test]
    fn the_transition_is_keyed_by_the_child_so_it_is_not_reused_across_a_switch() {
        // The outgoing child keeps its own opacity animation while the
        // incoming one starts a fresh one.
        assert_eq!(
            AnimatedSwitcher::transition_key(&SwitcherChild::keyed("Text", 7)),
            Some(7)
        );
        assert_eq!(
            AnimatedSwitcher::transition_key(&SwitcherChild::new("Text")),
            None
        );
    }

    #[test]
    fn the_outgoing_child_runs_the_same_animation_backwards() {
        // Which is what makes reverseDuration and switchOutCurve apply to it.
        assert!(AnimatedSwitcher::outgoing_runs_in_reverse());
    }

    // -- The switcher's widget ----------------------------------------------

    /// A `Text` child.
    ///
    /// Every label goes through the *same* closure, so two of these have the
    /// same type and can update each other unless they are given different
    /// keys -- which is exactly the distinction the switcher turns on, and
    /// exactly the trap upstream's documentation warns about.
    fn switcher_child(label: &str, key: Option<u64>) -> AnyWidget {
        let label = label.to_string();
        let child = crate::framework::leaf(move || crate::widgets::Text::new(label.clone()));
        match key {
            Some(key) => crate::framework::keyed_subtree(key, child),
            None => child,
        }
    }

    /// What reached the canvas, as `(text, alpha)` **in paint order**.
    ///
    /// A fully transparent layer is never painted at all, so a child missing
    /// from this list is a child that has finished leaving.
    fn switcher_painted(tree: &mut crate::framework::ElementTree) -> Vec<(String, u32)> {
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 200.0),
        );
        let mut layers = crate::engine::LayerTree::new(400, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(400.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        text_alphas(crate::engine_test_stubs::drawn())
    }

    #[test]
    fn the_first_child_a_switcher_is_ever_given_is_simply_there() {
        // Upstream's `initState` calls `_addEntryForNewChild(animate: false)`
        // and sets the controller straight to one. Nothing fades in from
        // nowhere on the frame a switcher is mounted.
        let switcher = AnimatedSwitcher::new(200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        assert!(
            !tree.advance_frame(1_000_000),
            "a switcher showing its first child has nothing to animate"
        );
        tree.rebuild_dirty();
        assert_eq!(
            switcher_painted(&mut tree),
            vec![("one".to_string(), 255)],
            "the first child should be fully there at once"
        );
    }

    #[test]
    fn a_different_child_fades_in_over_the_one_it_replaces() {
        let switcher = AnimatedSwitcher::new(200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        // A different key, so the new child cannot update the old one.
        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        assert!(tree.advance_frame(2_000_000), "a switch should want frames");
        tree.rebuild_dirty();

        tree.advance_frame(2_100_000);
        tree.rebuild_dirty();
        let painted = switcher_painted(&mut tree);
        assert_eq!(
            painted
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"],
            "the child leaving is painted underneath the one arriving"
        );
        for (text, alpha) in &painted {
            assert!(
                (*alpha as i64 - 128).abs() < 24,
                "halfway through, {text} should be about half painted, not {alpha}"
            );
        }
    }

    #[test]
    fn the_same_child_with_new_contents_does_not_animate_at_all() {
        // The trap the class is famous for, and the one `decide` documents:
        // two children of the same type with no keys can update each other, so
        // the text simply changes and nothing fades.
        let switcher = AnimatedSwitcher::new(200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", None))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        tree.rebuild(switcher.widget(Some(switcher_child("two", None))));
        assert!(
            !tree.advance_frame(2_000_000),
            "an unkeyed child replacing an unkeyed child of the same type is \
             not a switch, so nothing should be animating"
        );
        tree.rebuild_dirty();
        assert_eq!(
            switcher_painted(&mut tree),
            vec![("two".to_string(), 255)],
            "the new contents should just be there, with nothing left behind"
        );
    }

    #[test]
    fn the_child_that_has_finished_leaving_stops_being_built() {
        let switcher = AnimatedSwitcher::new(200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        tree.advance_frame(2_000_000);
        tree.rebuild_dirty();

        tree.advance_frame(2_200_000);
        tree.rebuild_dirty();
        assert_eq!(
            switcher_painted(&mut tree),
            vec![("two".to_string(), 255)],
            "once the reverse duration is up the old entry is gone, not \
             sitting invisible in the stack for ever"
        );
        assert!(
            !tree.advance_frame(2_400_000),
            "and a settled switcher stops asking for frames"
        );
    }

    #[test]
    fn a_child_interrupted_halfway_in_leaves_from_halfway() {
        // `outgoing_runs_in_reverse` in the only place it can be seen:
        // upstream reverses the entry's own controller, so a child caught at
        // 0.5 falls to nothing in *half* the reverse duration rather than
        // starting again from opaque.
        let switcher = AnimatedSwitcher::new(200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        tree.advance_frame(2_000_000);
        tree.rebuild_dirty();

        // "two" is halfway in when it is interrupted.
        tree.advance_frame(2_100_000);
        tree.rebuild_dirty();
        tree.rebuild(switcher.widget(Some(switcher_child("three", Some(3)))));
        tree.advance_frame(2_101_000);
        tree.rebuild_dirty();

        // A hundred milliseconds later "two" has fallen the 0.5 it had, and
        // "one" -- which left from opaque at 2_000_000 -- has run out too.
        tree.advance_frame(2_210_000);
        tree.rebuild_dirty();
        assert_eq!(
            switcher_painted(&mut tree)
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            vec!["three"],
            "a child that only ever reached half opacity should not take a \
             whole reverse duration to leave"
        );
    }

    #[test]
    fn a_child_taken_away_fades_out_with_nothing_behind_it() {
        // Upstream's `child: null`: whatever is showing leaves and nothing
        // replaces it.
        let switcher = AnimatedSwitcher::new(200_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        tree.rebuild(switcher.widget(None));
        assert!(
            tree.advance_frame(2_000_000),
            "a child being taken away is a switch too"
        );
        tree.rebuild_dirty();
        tree.advance_frame(2_100_000);
        tree.rebuild_dirty();
        let painted = switcher_painted(&mut tree);
        assert_eq!(painted.len(), 1, "the child on its way out is still drawn");
        assert!(
            (painted[0].1 as i64 - 128).abs() < 24,
            "and drawn half gone, not {}",
            painted[0].1
        );

        tree.advance_frame(2_200_000);
        tree.rebuild_dirty();
        assert!(
            switcher_painted(&mut tree).is_empty(),
            "and then there is nothing left"
        );
    }

    #[test]
    fn a_child_that_had_fully_arrived_leaves_on_the_curve_for_leaving() {
        // Its controller reached the end, which cleared `_curveDirection`, so
        // the reversal that follows starts a fresh direction and picks up
        // `switchOutCurve`.
        let mut switcher = AnimatedSwitcher::new(200_000);
        switcher.switch_in_curve = crate::animation::Curve::EASE_IN;
        switcher.switch_out_curve = crate::animation::Curve::EASE_OUT;
        assert_eq!(
            switcher.curve_a_child_leaves_on(true),
            crate::animation::Curve::EASE_OUT
        );
    }

    #[test]
    fn a_child_interrupted_on_its_way_in_keeps_the_curve_it_was_already_on() {
        // `CurvedAnimation` will not change curve under a value that is on
        // screen: "the curve direction is only reset when we hit the beginning
        // or the end of the timeline to avoid discontinuities". So the way out
        // is the way in, run backwards -- and somebody looking for
        // `switchOutCurve` in that output will not find it.
        let mut switcher = AnimatedSwitcher::new(200_000);
        switcher.switch_in_curve = crate::animation::Curve::EASE_IN;
        switcher.switch_out_curve = crate::animation::Curve::EASE_OUT;
        assert_eq!(
            switcher.curve_a_child_leaves_on(false),
            crate::animation::Curve::EASE_IN
        );
    }

    #[test]
    fn the_arriving_child_is_painted_through_the_curve_for_arriving() {
        // Linear would put it at half; `EASE_IN` is `t^2`-ish and is well
        // under that a third of the way through, which is the whole point of
        // asking for a curve.
        let mut switcher = AnimatedSwitcher::new(300_000);
        switcher.switch_in_curve = crate::animation::Curve::EASE_IN;
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();
        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        tree.advance_frame(2_000_000);
        tree.rebuild_dirty();
        tree.advance_frame(2_100_000);
        tree.rebuild_dirty();

        let painted = switcher_painted(&mut tree);
        let arriving = painted
            .iter()
            .find(|(text, _)| text == "two")
            .expect("the arriving child");
        let expected = crate::animation::Curve::EASE_IN.transform(1.0 / 3.0);
        assert!(
            (arriving.1 as f32 - expected * 255.0).abs() < 6.0,
            "a third of the way in on an ease-in should be about {}, not {}",
            expected * 255.0,
            arriving.1
        );
        assert!(
            arriving.1 < 60,
            "and nowhere near the 85 a linear fade would give: {}",
            arriving.1
        );
    }

    #[test]
    fn an_entry_a_curve_has_taken_below_zero_is_kept_until_its_controller_lands() {
        // Upstream removes an entry when its **animation** reports dismissed,
        // which is the controller's status and not the curved value. An
        // elastic reverse curve dips below zero on the way out, so the child
        // disappears and comes back; dropping it the first time it went
        // invisible would mean it could never come back.
        let mut switcher = AnimatedSwitcher::new(200_000);
        switcher.switch_out_curve = crate::animation::Curve::ElasticIn(0.4);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();
        // "one" had fully arrived, so it leaves on `switchOutCurve`.
        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        tree.advance_frame(2_000_000);
        tree.rebuild_dirty();

        // A fifth of the way out the curve is at about -0.25, which the
        // opacity clamps away entirely.
        tree.advance_frame(2_040_000);
        tree.rebuild_dirty();
        assert!(
            switcher_painted(&mut tree)
                .iter()
                .all(|(text, _)| text != "one"),
            "the curve has taken it below nothing, so it is not painted"
        );

        // And a little later the curve comes back up and so does the child --
        // which it could not do if it had been thrown away above.
        tree.advance_frame(2_070_000);
        tree.rebuild_dirty();
        assert!(
            switcher_painted(&mut tree)
                .iter()
                .any(|(text, _)| text == "one"),
            "an entry is kept until its controller lands, not until its curve              first reaches nothing"
        );

        tree.advance_frame(2_200_000);
        tree.rebuild_dirty();
        assert!(
            switcher_painted(&mut tree)
                .iter()
                .all(|(text, _)| text != "one"),
            "and once the controller lands it is gone for good"
        );
    }

    #[test]
    fn the_child_leaving_is_reversed_from_its_controller_and_not_from_what_was_drawn() {
        // The two are the same number only while the curve is linear, which
        // is why this needs one that is not. Upstream reverses the
        // *controller*; a child caught at controller 0.5 has half a reverse
        // duration left however faint an ease-in had made it look.
        let mut switcher = AnimatedSwitcher::new(200_000);
        switcher.switch_in_curve = crate::animation::Curve::EASE_IN;
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        tree.advance_frame(2_000_000);
        tree.rebuild_dirty();

        // "two" is at controller 0.5 -- but drawn at about a third of that,
        // because an ease-in is slow to start.
        tree.advance_frame(2_100_000);
        tree.rebuild_dirty();
        tree.rebuild(switcher.widget(Some(switcher_child("three", Some(3)))));
        tree.advance_frame(2_101_000);
        tree.rebuild_dirty();

        // Reversed from 0.5 it still has 100ms to run. Reversed from what was
        // painted it would have run out around 2_163_000.
        tree.advance_frame(2_180_000);
        tree.rebuild_dirty();
        assert!(
            switcher_painted(&mut tree)
                .iter()
                .any(|(text, _)| text == "two"),
            "reversing from the painted value would have finished it early"
        );

        tree.advance_frame(2_210_000);
        tree.rebuild_dirty();
        assert!(
            switcher_painted(&mut tree)
                .iter()
                .all(|(text, _)| text != "two"),
            "and half a reverse duration is all it has"
        );
    }

    #[test]
    fn the_fade_out_can_be_given_a_length_of_its_own() {
        // Upstream's `reverseDuration ?? duration`.
        let mut switcher = AnimatedSwitcher::new(200_000);
        switcher.reverse_duration_micros = Some(400_000);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(switcher.widget(Some(switcher_child("one", Some(1)))));
        tree.advance_frame(1_000_000);
        tree.rebuild_dirty();

        tree.rebuild(switcher.widget(Some(switcher_child("two", Some(2)))));
        tree.advance_frame(2_000_000);
        tree.rebuild_dirty();

        // The arriving child has landed; the leaving one is only halfway out.
        tree.advance_frame(2_200_000);
        tree.rebuild_dirty();
        let painted = switcher_painted(&mut tree);
        assert_eq!(painted.len(), 2, "the old child is still on its way out");
        assert_eq!(painted[0].0, "one");
        assert!(
            (painted[0].1 as i64 - 128).abs() < 24,
            "half of the longer reverse duration should be half gone, not {}",
            painted[0].1
        );
        assert_eq!(painted[1].1, 255, "while the new one has fully arrived");
    }

    // -- The fading image ----------------------------------------------------

    #[test]
    fn the_placeholder_leaves_faster_than_the_image_arrives() {
        // A symmetric cross-fade would show both at half strength through the
        // middle, which on a photograph over a grey box reads as a smear.
        let image = FadeInImage::new();
        assert_eq!(image.fade_out_micros, 300_000);
        assert_eq!(image.fade_in_micros, 700_000);
        assert!(image.fade_out_is_quicker());
    }

    #[test]
    fn nothing_happens_until_the_real_image_has_arrived() {
        let image = FadeInImage::new();
        assert_eq!(image.phase_at(false, 0), FadeInPhase::Placeholder);
        assert_eq!(
            image.phase_at(false, 10_000_000),
            FadeInPhase::Placeholder,
            "however long it takes"
        );
    }

    #[test]
    fn the_two_fades_run_one_after_the_other_rather_than_together() {
        let image = FadeInImage::new();
        assert_eq!(image.phase_at(true, 0), FadeInPhase::FadingOut);
        assert_eq!(image.phase_at(true, 299_999), FadeInPhase::FadingOut);
        assert_eq!(image.phase_at(true, 300_000), FadeInPhase::FadingIn);
        assert_eq!(image.phase_at(true, 999_999), FadeInPhase::FadingIn);
        assert_eq!(image.phase_at(true, 1_000_000), FadeInPhase::Complete);
    }

    #[test]
    fn the_placeholder_is_never_itself_a_download() {
        // Or the widget would be waiting on two downloads before it could show
        // the reader anything.
        assert!(FadeInImage::placeholder_must_be_local());
    }

    // -- The icons -----------------------------------------------------------

    #[test]
    fn an_icon_takes_its_size_from_the_theme_when_it_was_given_none() {
        // Which is how a whole toolbar's icons change by one line above them.
        let inherits = Icon::new();
        assert_eq!(inherits.resolved_size(Some(18.0)), 18.0);
        assert_eq!(inherits.resolved_size(None), 24.0, "the framework default");

        let mut explicit = Icon::new();
        explicit.size = Some(32.0);
        assert_eq!(explicit.resolved_size(Some(18.0)), 32.0);
    }

    #[test]
    fn an_image_icon_follows_the_same_theme_resolution() {
        // Which is the whole point of the class: it lines up with the font
        // icons beside it.
        let image_icon = ImageIcon::new();
        assert_eq!(image_icon.resolved_size(Some(18.0)), 18.0);
        assert_eq!(image_icon.resolved_size(None), 24.0);
    }

    #[test]
    fn the_variable_font_axes_have_real_ranges_rather_than_free_numbers() {
        let mut icon = Icon::new();
        assert!(icon.axes_are_valid());

        icon.fill = Some(0.5);
        icon.weight = Some(400.0);
        assert!(icon.axes_are_valid());

        icon.fill = Some(1.5);
        assert!(!icon.axes_are_valid());

        icon.fill = Some(0.5);
        icon.weight = Some(0.0);
        assert!(!icon.axes_are_valid());
    }

    #[test]
    fn an_icon_has_no_semantic_label_by_default() {
        // Most sit next to a label that already says what they are, and
        // announcing both would say it twice.
        assert!(!Icon::new().has_semantic_label);
    }
}

#[cfg(test)]
mod icon_theme_tests {
    use super::*;
    use crate::component_themes::{IconTheme, IconThemeData, ResolvedIcon};
    use crate::engine::Color;
    use crate::framework::{AnyWidget, BuildContext, Component, ElementTree, component};

    struct Reader {
        icon: Icon,
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedIcon>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(self.icon.resolved(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve(icon: Icon, data: IconThemeData) -> ResolvedIcon {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(IconTheme::new(
            data,
            component(Reader {
                icon,
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// No theme installed at all, which is the case the two defaults differ in.
    fn resolve_bare(icon: Icon) -> ResolvedIcon {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader {
            icon,
            seen: std::rc::Rc::clone(&seen),
        }));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn a_theme_says_twenty_four_and_no_theme_at_all_says_fourteen() {
        // Not an oversight. An icon with nothing around it to belong to is a
        // glyph in a line of type, and fourteen is what a glyph is; twenty-four
        // is the Material icon size, which is a thing a *theme* knows.
        let mut data = IconThemeData::new();
        data.size = Some(ResolvedIcon::THEME_SIZE);
        assert_eq!(resolve(Icon::new(), data).size, 24.0);
        assert_eq!(
            resolve_bare(Icon::new()).size,
            ResolvedIcon::DEFAULT_FONT_SIZE
        );
        assert_eq!(ResolvedIcon::DEFAULT_FONT_SIZE, 14.0);
    }

    #[test]
    fn the_icons_own_size_beats_the_themes() {
        let mut data = IconThemeData::new();
        data.size = Some(24.0);
        let mut icon = Icon::new();
        icon.size = Some(48.0);
        assert_eq!(resolve(icon, data).size, 48.0);
    }

    #[test]
    fn an_icon_does_not_grow_with_the_text_unless_it_is_told_to() {
        // An icon in a sentence should follow the reader's text size; an icon
        // that is a button should not, because the button around it is a fixed
        // target and a growing glyph would burst it.
        let mut data = IconThemeData::new();
        data.size = Some(20.0);
        assert!(!resolve(Icon::new(), data.clone()).apply_text_scaling);
        assert_eq!(resolve(Icon::new(), data.clone()).size, 20.0);

        // Under a real scale, because at the default of 1.0 a scaled size and
        // an unscaled one are the same number and the test would pass either
        // way.
        crate::media_query::with_text_scale(2.0, || {
            let mut icon = Icon::new();
            icon.apply_text_scaling = Some(true);
            let scaled = resolve(icon, data.clone());
            assert!(scaled.apply_text_scaling);
            assert_eq!(
                scaled.size, 40.0,
                "the tentative twenty, through the scaler"
            );

            assert_eq!(
                resolve(Icon::new(), data.clone()).size,
                20.0,
                "and an icon that did not ask is left alone"
            );
        });

        // And the theme can ask for it on everything below it.
        data.apply_text_scaling = Some(true);
        assert!(resolve(Icon::new(), data).apply_text_scaling);
    }

    #[test]
    fn the_opacity_applies_to_whichever_colour_came_out() {
        // Which is why it is not a colour of its own: it dims the icon's own
        // colour and the theme's alike.
        let mut data = IconThemeData::new();
        data.color = Some(Color::argb(0xFF, 1, 2, 3));
        let data = data.with_opacity(0.5);
        assert_eq!(resolve(Icon::new(), data.clone()).color.alpha(), 128);

        let mut icon = Icon::new();
        icon.color = Some(Color::argb(0xFF, 9, 9, 9));
        let mine = resolve(icon, data);
        assert_eq!(mine.color.red(), 9, "my colour");
        assert_eq!(mine.color.alpha(), 128, "and the theme's opacity over it");
    }

    #[test]
    fn an_icon_with_no_colour_anywhere_is_black() {
        assert_eq!(
            resolve_bare(Icon::new()).color,
            Color::argb(0xFF, 0, 0, 0),
            "upstream's IconThemeData.fallback colour"
        );
    }

    #[test]
    fn the_variable_font_axes_fall_back_to_upstreams_fallback_values() {
        let bare = resolve_bare(Icon::new());
        assert_eq!(bare.fill, 0.0);
        assert_eq!(bare.weight, 400.0);
        assert_eq!(bare.grade, 0.0);
        assert_eq!(bare.optical_size, 48.0);
    }

    #[test]
    fn every_axis_prefers_the_icons_own_value_over_the_themes() {
        // Each axis is `icon.x.or(theme.x)`, and with only one side set the
        // direction cannot be seen. Set on both, on every axis at once.
        let mut data = IconThemeData::new();
        data.size = Some(2.0);
        data.fill = Some(0.2);
        data.weight = Some(200.0);
        data.grade = Some(20.0);
        data.optical_size = Some(22.0);
        data.color = Some(Color::argb(0xFF, 2, 2, 2));

        // The two sides must *disagree* on every field, not merely both be
        // set: a flag that is `Some(true)` on both sides makes the swap
        // invisible, which is how `apply_text_scaling` stayed untested here.
        data.apply_text_scaling = Some(false);

        let mut icon = Icon::new();
        icon.size = Some(1.0);
        icon.fill = Some(0.1);
        icon.weight = Some(100.0);
        icon.grade = Some(10.0);
        icon.optical_size = Some(11.0);
        icon.color = Some(Color::argb(0xFF, 1, 1, 1));
        icon.apply_text_scaling = Some(true);

        let resolved = resolve(icon, data);
        assert!(
            resolved.apply_text_scaling,
            "the icon's own, over the theme's"
        );
        assert_eq!(resolved.size, 1.0);
        assert_eq!(resolved.fill, 0.1);
        assert_eq!(resolved.weight, 100.0);
        assert_eq!(resolved.grade, 10.0);
        assert_eq!(resolved.optical_size, 11.0);
        assert_eq!(resolved.color, Color::argb(0xFF, 1, 1, 1));
    }

    #[test]
    fn each_axis_is_its_own_three_step_chain() {
        let mut data = IconThemeData::new();
        data.weight = Some(700.0);
        data.grade = Some(200.0);
        let mut icon = Icon::new();
        icon.weight = Some(300.0);
        let resolved = resolve(icon, data);
        assert_eq!(resolved.weight, 300.0, "the icon's");
        assert_eq!(resolved.grade, 200.0, "the theme's");
        assert_eq!(resolved.fill, 0.0, "and the fallback for what neither set");
    }

    #[test]
    fn the_axes_have_ranges_and_a_number_outside_them_is_refused() {
        // Variable-font axes with real ranges, not free numbers.
        let mut fill = Icon::new();
        fill.fill = Some(1.5);
        assert!(!fill.axes_are_valid());

        let mut weight = Icon::new();
        weight.weight = Some(0.0);
        assert!(!weight.axes_are_valid());

        let mut fine = Icon::new();
        fine.fill = Some(1.0);
        fine.weight = Some(1.0);
        assert!(fine.axes_are_valid());
    }
}
