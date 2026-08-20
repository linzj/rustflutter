//! The semantics markers from `widgets/basic.dart`: `SliverSemantics`,
//! `MergeSemantics`, `BlockSemantics`, `ExcludeSemantics` and
//! `IndexedSemantics`.
//!
//! Four of them are three-line wrappers over a render object, and the reason
//! there are four rather than one flag is that each removes something
//! different. The interesting pair is `BlockSemantics` and `ExcludeSemantics`:
//! they sound alike and look in opposite directions. **Exclude looks inward,
//! at its own descendants. Block looks outward and backward, at everything
//! painted before it in the same container.** An open drawer wants the second
//! one -- the things it needs to hide are not below it, they are behind it.

use crate::render::RenderBox;
use crate::semantics::{RenderSemantics, SemanticsProperties};

/// Upstream `SliverSemantics`: the sliver variant of `Semantics`.
///
/// Upstream needs a separate class because a sliver's render object has a
/// different base than a box's. Here the annotation is the same render object
/// either way, so the difference is only in what it wraps.
pub struct SliverSemantics;

impl SliverSemantics {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        id: i32,
        properties: SemanticsProperties,
        sliver: impl RenderBox + 'static,
    ) -> RenderSemantics {
        RenderSemantics::new(id, properties, sliver)
    }
}

/// Upstream `MergeSemantics`.
///
/// Everything below it becomes one node. Upstream's documentation is careful
/// about what that costs: labels are joined with newlines, and if more than one
/// node in the merged subtree can handle a gesture, **the first in tree order
/// takes the callbacks** -- the others are not merged so much as dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeSemantics;

/// Upstream `BlockSemantics`.
///
/// Drops the semantics of everything painted **before** it in the same semantic
/// container. Not its own subtree -- the things it hides are its siblings, and
/// specifically the ones underneath it on screen. An alert or an open drawer is
/// the case: those are still partly visible, and a screen reader that could
/// still reach them would let the reader act on a page that is not in front of
/// them any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockSemantics {
    /// Defaults to true. It is a field rather than an absence so a widget can
    /// stop blocking without being removed from the tree.
    pub blocking: bool,
}

impl BlockSemantics {
    pub fn new() -> BlockSemantics {
        BlockSemantics { blocking: true }
    }

    pub fn with_blocking(blocking: bool) -> BlockSemantics {
        BlockSemantics { blocking }
    }
}

impl Default for BlockSemantics {
    fn default() -> Self {
        BlockSemantics::new()
    }
}

/// Upstream `ExcludeSemantics`.
///
/// Drops all the semantics of **its own descendants**, and itself with them.
/// The opposite direction of travel from [`BlockSemantics`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcludeSemantics {
    /// Defaults to true.
    pub excluding: bool,
}

impl ExcludeSemantics {
    pub fn new() -> ExcludeSemantics {
        ExcludeSemantics { excluding: true }
    }

    pub fn with_excluding(excluding: bool) -> ExcludeSemantics {
        ExcludeSemantics { excluding }
    }
}

impl Default for ExcludeSemantics {
    fn default() -> Self {
        ExcludeSemantics::new()
    }
}

/// Upstream `IndexedSemantics`.
///
/// Annotates the first child semantics node with an index, which is what
/// TalkBack and VoiceOver read out as "item 3 of 12". A list gives these out
/// automatically, and the reason to set them by hand is that the automatic ones
/// count everything: upstream's example is a list with spacers between the
/// items, where the announcement claims four things are visible when two are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexedSemantics {
    pub index: i32,
}

impl IndexedSemantics {
    pub fn new(index: i32) -> IndexedSemantics {
        IndexedSemantics { index }
    }
}

/// Which marker, if any, an entry in a semantic container carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SemanticsMarker {
    #[default]
    None,
    Merge(MergeSemantics),
    Block(BlockSemantics),
    Exclude(ExcludeSemantics),
    Indexed(IndexedSemantics),
}

/// One annotated subtree within a semantic container, as the walk meets it.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsEntry {
    pub id: u64,
    pub marker: SemanticsMarker,
    /// The semantics nodes below this one, in tree order.
    pub descendants: Vec<u64>,
}

impl SemanticsEntry {
    pub fn new(id: u64) -> SemanticsEntry {
        SemanticsEntry {
            id,
            marker: SemanticsMarker::None,
            descendants: Vec::new(),
        }
    }

    pub fn with_marker(mut self, marker: SemanticsMarker) -> Self {
        self.marker = marker;
        self
    }

    pub fn with_descendants(mut self, descendants: &[u64]) -> Self {
        self.descendants = descendants.to_vec();
        self
    }
}

/// What survives a semantic container's walk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticsOutcome {
    /// The nodes a screen reader can reach, in paint order.
    pub reachable: Vec<u64>,
    /// The entries whose descendants were folded into them.
    pub merged: Vec<u64>,
    /// Explicit indices, by node.
    pub indices: Vec<(u64, i32)>,
}

/// Applies the markers over one semantic container's entries, given in **paint
/// order** -- first painted first, which is back to front.
///
/// The order is what makes `BlockSemantics` expressible at all: "before me" has
/// no meaning in a tree, only on a screen.
pub fn resolve_semantics(entries: &[SemanticsEntry]) -> SemanticsOutcome {
    let mut outcome = SemanticsOutcome::default();
    for entry in entries {
        match entry.marker {
            SemanticsMarker::Block(block) if block.blocking => {
                // Everything painted earlier goes, and the indices with them.
                outcome.reachable.clear();
                outcome.merged.clear();
                outcome.indices.clear();
                outcome.reachable.push(entry.id);
                outcome.reachable.extend(entry.descendants.iter().copied());
            }
            SemanticsMarker::Exclude(exclude) if exclude.excluding => {
                // Itself and everything below it. Nothing else is touched.
            }
            SemanticsMarker::Merge(_) => {
                outcome.reachable.push(entry.id);
                outcome.merged.push(entry.id);
            }
            SemanticsMarker::Indexed(indexed) => {
                outcome.reachable.push(entry.id);
                outcome.reachable.extend(entry.descendants.iter().copied());
                outcome.indices.push((entry.id, indexed.index));
            }
            _ => {
                outcome.reachable.push(entry.id);
                outcome.reachable.extend(entry.descendants.iter().copied());
            }
        }
    }
    outcome
}

// -- Gestures as the semantics tree sees them ---------------------------------

/// One synthesised gesture callback, in the order a recogniser receives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynthesisedCallback {
    TapDown,
    TapUp,
    Tap,
    LongPressStart,
    LongPress,
    LongPressEnd,
    DragDown,
    DragStart,
    DragUpdate,
    DragEnd,
}

/// Upstream `SemanticsGestureDelegate`.
///
/// The bridge between a gesture detector and the semantics tree, and its whole
/// difficulty is that **a semantic action has no position**. A screen reader
/// says "activate this"; it does not say where. So the delegate invents a
/// place -- the centre of the widget -- and synthesises the whole gesture from
/// it.
///
/// Not just the last callback, either. A tap becomes `onTapDown`, `onTapUp`,
/// `onTap` in order, because a recogniser's callbacks each carry meaning: a
/// button that highlights on down and fires on up would otherwise never
/// highlight for a screen reader user. The point device kind is `unknown`,
/// which is the honest answer.
pub trait SemanticsGestureDelegate {
    /// Which semantic actions this detector advertises. Upstream assigns a
    /// handler per recogniser present, so a detector with no
    /// `TapGestureRecognizer` advertises no tap: **the semantics tree offers
    /// exactly what the widget can actually do.**
    fn assign_semantics(&self, recognizers: &[GestureRecognizerKind]) -> Vec<SemanticsGestureSlot>;
}

/// The recognisers the default delegate looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureRecognizerKind {
    Tap,
    LongPress,
    HorizontalDrag,
    VerticalDrag,
    Pan,
}

/// Which handler slot on the render object gets filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticsGestureSlot {
    OnTap,
    OnLongPress,
    OnHorizontalDragUpdate,
    OnVerticalDragUpdate,
}

/// Upstream's `_DefaultSemanticsGestureDelegate`.
///
/// Upstream prefixes it with an unusual note: *"For readers who come here to
/// learn how to write custom semantics delegates: this is not a proper sample
/// code."* It reaches into the detector's private state, which a real delegate
/// cannot, and it does so to preserve behaviour that predates the interface.
/// A normal delegate stores callbacks as properties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultSemanticsGestureDelegate;

impl DefaultSemanticsGestureDelegate {
    /// The callbacks a semantic tap fires, in order.
    pub fn tap_sequence() -> [SynthesisedCallback; 3] {
        [
            SynthesisedCallback::TapDown,
            SynthesisedCallback::TapUp,
            SynthesisedCallback::Tap,
        ]
    }

    /// A semantic long press: the same idea, one stage longer.
    pub fn long_press_sequence() -> [SynthesisedCallback; 3] {
        [
            SynthesisedCallback::LongPressStart,
            SynthesisedCallback::LongPress,
            SynthesisedCallback::LongPressEnd,
        ]
    }

    /// A semantic drag synthesises a **whole gesture in one call**: down,
    /// start, update, end. The screen reader gives a delta and nothing else, so
    /// the beginning and the end have to be made up around it.
    pub fn drag_sequence() -> [SynthesisedCallback; 4] {
        [
            SynthesisedCallback::DragDown,
            SynthesisedCallback::DragStart,
            SynthesisedCallback::DragUpdate,
            SynthesisedCallback::DragEnd,
        ]
    }

    /// The velocity a synthesised drag ends with. Zero, because a screen
    /// reader's swipe has no speed -- and an invented velocity would send the
    /// list flying.
    pub const SYNTHESISED_END_VELOCITY: f32 = 0.0;

    /// Where a positionless action is taken to have happened.
    pub fn synthesised_local_position(size: (f32, f32)) -> (f32, f32) {
        (size.0 / 2.0, size.1 / 2.0)
    }

    /// A render object that is not a box has no size to find a centre in, so
    /// upstream falls back to the zero rectangle.
    pub fn synthesised_local_position_for_non_box() -> (f32, f32) {
        (0.0, 0.0)
    }
}

impl SemanticsGestureDelegate for DefaultSemanticsGestureDelegate {
    fn assign_semantics(&self, recognizers: &[GestureRecognizerKind]) -> Vec<SemanticsGestureSlot> {
        let has = |kind| recognizers.contains(&kind);
        let mut slots = Vec::new();
        if has(GestureRecognizerKind::Tap) {
            slots.push(SemanticsGestureSlot::OnTap);
        }
        if has(GestureRecognizerKind::LongPress) {
            slots.push(SemanticsGestureSlot::OnLongPress);
        }
        // A pan fills both axes, because a pan answers a drag in either
        // direction -- and when both a pan and an axis recogniser are present,
        // upstream calls both handlers rather than choosing.
        if has(GestureRecognizerKind::HorizontalDrag) || has(GestureRecognizerKind::Pan) {
            slots.push(SemanticsGestureSlot::OnHorizontalDragUpdate);
        }
        if has(GestureRecognizerKind::VerticalDrag) || has(GestureRecognizerKind::Pan) {
            slots.push(SemanticsGestureSlot::OnVerticalDragUpdate);
        }
        slots
    }
}

/// Upstream `SliverEnsureSemantics`.
///
/// Two lines: a proxy sliver whose render object overrides `ensureSemantics` to
/// true, keeping its child in the semantics tree even when it has scrolled out
/// of the viewport **and** out of the cache extent. A screen reader can then
/// reach a header nobody can see.
///
/// The documentation carries a warning that is really an admission, and it is
/// the interesting part: **this only works with slivers that know their extent
/// in advance.** A lazy `SliverList` underestimates the scroll extent, and
/// assistive technology navigating by that extent will fail to scroll to the
/// very content this widget just made reachable. Upstream's advice is to reach
/// for `SliverFixedExtentList`, `SliverVariedExtentList` or
/// `SliverPrototypeExtentList` instead.
///
/// **Making something reachable is not the same as making it findable.**
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SliverEnsureSemantics;

impl SliverEnsureSemantics {
    pub fn new() -> SliverEnsureSemantics {
        SliverEnsureSemantics
    }

    /// The one override.
    pub fn ensure_semantics() -> bool {
        true
    }

    /// Whether a sliver's scroll extent is known before its children are laid
    /// out, which is what this widget needs of whatever it wraps.
    pub fn extent_is_known_in_advance(sliver: &str) -> bool {
        matches!(
            sliver,
            "SliverFixedExtentList" | "SliverVariedExtentList" | "SliverPrototypeExtentList"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, descendants: &[u64]) -> SemanticsEntry {
        SemanticsEntry::new(id).with_descendants(descendants)
    }

    #[test]
    fn block_looks_backward_and_exclude_looks_inward() {
        // They sound alike and remove opposite things. This is the whole reason
        // there are two classes.
        let blocked = resolve_semantics(&[
            entry(1, &[10, 11]),
            entry(2, &[20]).with_marker(SemanticsMarker::Block(BlockSemantics::new())),
            entry(3, &[30]),
        ]);
        assert_eq!(
            blocked.reachable,
            [2, 20, 3, 30],
            "everything painted earlier is gone, including the blocker's siblings"
        );

        let excluded = resolve_semantics(&[
            entry(1, &[10, 11]),
            entry(2, &[20]).with_marker(SemanticsMarker::Exclude(ExcludeSemantics::new())),
            entry(3, &[30]),
        ]);
        assert_eq!(
            excluded.reachable,
            [1, 10, 11, 3, 30],
            "only its own subtree is gone; the siblings are untouched"
        );
    }

    #[test]
    fn an_open_drawer_hides_the_page_behind_it_and_keeps_its_own_contents() {
        // The page is still partly visible, and a reader who could still reach
        // it would be acting on something that is no longer in front of them.
        let page = entry(1, &[10, 11, 12]);
        let drawer = entry(2, &[20, 21]).with_marker(SemanticsMarker::Block(BlockSemantics::new()));
        let outcome = resolve_semantics(&[page, drawer]);
        assert_eq!(outcome.reachable, [2, 20, 21]);
    }

    #[test]
    fn something_painted_after_a_block_is_not_affected_by_it() {
        // "Before" is a fact about the screen, not about the tree.
        let outcome = resolve_semantics(&[
            entry(1, &[]).with_marker(SemanticsMarker::Block(BlockSemantics::new())),
            entry(2, &[]),
            entry(3, &[]),
        ]);
        assert_eq!(outcome.reachable, [1, 2, 3]);
    }

    #[test]
    fn a_marker_can_stop_doing_its_job_without_leaving_the_tree() {
        // Which is why blocking and excluding are fields rather than the
        // widget's presence.
        let not_blocking = resolve_semantics(&[
            entry(1, &[]),
            entry(2, &[]).with_marker(SemanticsMarker::Block(BlockSemantics::with_blocking(false))),
        ]);
        assert_eq!(not_blocking.reachable, [1, 2]);

        let not_excluding = resolve_semantics(&[entry(1, &[10]).with_marker(
            SemanticsMarker::Exclude(ExcludeSemantics::with_excluding(false)),
        )]);
        assert_eq!(not_excluding.reachable, [1, 10]);
    }

    #[test]
    fn merging_folds_the_descendants_into_the_one_node() {
        // Their labels join with newlines, and only the first node able to
        // handle a gesture keeps its callbacks.
        let outcome = resolve_semantics(&[
            entry(1, &[10, 11, 12]).with_marker(SemanticsMarker::Merge(MergeSemantics)),
            entry(2, &[20]),
        ]);
        assert_eq!(outcome.reachable, [1, 2, 20]);
        assert_eq!(outcome.merged, [1]);
    }

    #[test]
    fn an_index_names_the_node_it_is_on() {
        // A list with spacers would otherwise announce four items where there
        // are two.
        let outcome = resolve_semantics(&[
            entry(1, &[]).with_marker(SemanticsMarker::Indexed(IndexedSemantics::new(0))),
            entry(2, &[]),
            entry(3, &[]).with_marker(SemanticsMarker::Indexed(IndexedSemantics::new(1))),
            entry(4, &[]),
        ]);
        assert_eq!(outcome.indices, [(1, 0), (3, 1)]);
        assert_eq!(outcome.reachable.len(), 4, "the spacers are still there");
    }

    #[test]
    fn a_block_takes_the_indices_of_what_it_blocked_with_it() {
        // The nodes are gone, so an index naming one of them names nothing.
        let outcome = resolve_semantics(&[
            entry(1, &[]).with_marker(SemanticsMarker::Indexed(IndexedSemantics::new(0))),
            entry(2, &[]).with_marker(SemanticsMarker::Block(BlockSemantics::new())),
            entry(3, &[]).with_marker(SemanticsMarker::Indexed(IndexedSemantics::new(1))),
        ]);
        assert_eq!(outcome.indices, [(3, 1)]);
        assert_eq!(outcome.reachable, [2, 3]);
    }

    #[test]
    fn every_marker_defaults_to_doing_its_job() {
        assert!(BlockSemantics::default().blocking);
        assert!(ExcludeSemantics::default().excluding);
        assert_eq!(SemanticsMarker::default(), SemanticsMarker::None);
    }

    #[test]
    fn an_empty_container_resolves_to_nothing_rather_than_failing() {
        assert_eq!(resolve_semantics(&[]), SemanticsOutcome::default());
    }
    // -- The gesture delegate -------------------------------------------------

    #[test]
    fn a_semantic_tap_synthesises_the_whole_sequence_not_just_the_last_call() {
        // A button that highlights on down and fires on up would otherwise
        // never highlight for a screen reader user.
        assert_eq!(
            DefaultSemanticsGestureDelegate::tap_sequence(),
            [
                SynthesisedCallback::TapDown,
                SynthesisedCallback::TapUp,
                SynthesisedCallback::Tap
            ]
        );
        assert_eq!(
            DefaultSemanticsGestureDelegate::drag_sequence().len(),
            4,
            "down, start, update, end -- a whole gesture in one call"
        );
    }

    #[test]
    fn a_positionless_action_is_taken_to_have_happened_in_the_middle() {
        assert_eq!(
            DefaultSemanticsGestureDelegate::synthesised_local_position((80.0, 40.0)),
            (40.0, 20.0)
        );
        assert_eq!(
            DefaultSemanticsGestureDelegate::synthesised_local_position_for_non_box(),
            (0.0, 0.0),
            "and something with no size has no centre to find"
        );
    }

    #[test]
    fn a_synthesised_drag_ends_at_a_standstill() {
        // A screen reader's swipe has no speed, and an invented velocity would
        // send the list flying.
        assert_eq!(
            DefaultSemanticsGestureDelegate::SYNTHESISED_END_VELOCITY,
            0.0
        );
    }

    #[test]
    fn the_semantics_tree_offers_exactly_what_the_widget_can_actually_do() {
        let delegate = DefaultSemanticsGestureDelegate;
        assert!(delegate.assign_semantics(&[]).is_empty());
        assert_eq!(
            delegate.assign_semantics(&[GestureRecognizerKind::Tap]),
            [SemanticsGestureSlot::OnTap]
        );
        assert_eq!(
            delegate
                .assign_semantics(&[GestureRecognizerKind::Tap, GestureRecognizerKind::LongPress]),
            [
                SemanticsGestureSlot::OnTap,
                SemanticsGestureSlot::OnLongPress
            ]
        );
    }

    #[test]
    fn a_pan_answers_a_drag_in_either_direction() {
        let delegate = DefaultSemanticsGestureDelegate;
        assert_eq!(
            delegate.assign_semantics(&[GestureRecognizerKind::Pan]),
            [
                SemanticsGestureSlot::OnHorizontalDragUpdate,
                SemanticsGestureSlot::OnVerticalDragUpdate
            ]
        );
        assert_eq!(
            delegate.assign_semantics(&[GestureRecognizerKind::HorizontalDrag]),
            [SemanticsGestureSlot::OnHorizontalDragUpdate],
            "while an axis recogniser answers only its own"
        );
    }

    // -- SliverEnsureSemantics --------------------------------------------------

    #[test]
    fn the_whole_class_is_one_override() {
        assert!(SliverEnsureSemantics::ensure_semantics());
    }

    #[test]
    fn making_something_reachable_is_not_the_same_as_making_it_findable() {
        // A lazy list underestimates the scroll extent, and assistive
        // technology navigating by that extent fails to scroll to the very
        // content this widget just made reachable.
        for sliver in [
            "SliverFixedExtentList",
            "SliverVariedExtentList",
            "SliverPrototypeExtentList",
        ] {
            assert!(
                SliverEnsureSemantics::extent_is_known_in_advance(sliver),
                "{sliver}"
            );
        }
        assert!(!SliverEnsureSemantics::extent_is_known_in_advance(
            "SliverList"
        ));
    }
}
