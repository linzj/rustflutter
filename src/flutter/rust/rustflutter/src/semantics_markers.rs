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
}
