//! Lists and grids whose rows animate in and out -- a port of upstream's
//! `widgets/animated_scroll_view.dart`.
//!
//! The whole of the difficulty is **two index spaces that do not agree**, and
//! upstream's own comment says why they exist:
//!
//! > The `insertItem()` and `removeItem()` index parameters are defined as if
//! > the `removeItem()` operation removed the corresponding list/grid entry
//! > immediately. The entry is only actually removed from the
//! > `ListView`/`GridView` when the remove animation finishes.
//!
//! So there are two answers to "which row is this":
//!
//! * an **index**, which is what a caller means -- the list as it will be, with
//!   removed rows already gone;
//! * an **item index**, which is what the sliver sees -- the list as it is,
//!   with rows still shrinking away still occupying space.
//!
//! [`AnimatedItems::index_to_item_index`] and its inverse convert between them,
//! and the asymmetry between the two (`<=` one way, `<` the other) is the
//! entire reason a caller can say "remove row 3" twice in a row and mean two
//! different rows.
//!
//! ## What is not here
//!
//! The animations themselves are `AnimationController`s upstream; here an item
//! carries the animation's *value* and the caller advances it, which is the
//! shape the rest of this crate uses. The scroll views the four public widgets
//! wrap are the crate's own.

use crate::framework::AnyWidget;
use std::rc::Rc;

/// How long an insertion or removal takes by default. Upstream's `_kDuration`.
pub const ANIMATED_ITEM_DURATION_MICROS: i64 = 300_000;

/// Which way an active item is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveItemKind {
    /// Growing in. Upstream's `_ActiveItem.incoming`.
    Incoming,
    /// Shrinking away, and still occupying a slot until it has finished.
    /// Upstream's `_ActiveItem.outgoing`.
    Outgoing,
}

/// Upstream's `_ActiveItem`: one row that is mid-animation.
///
/// Upstream's third constructor, `_ActiveItem.index`, builds one with no
/// animation at all -- it exists only to be the needle in a binary search over
/// a list sorted by index, which is what `compareTo` sorts by.
#[derive(Clone)]
pub struct ActiveItem {
    pub kind: ActiveItemKind,
    /// The **item** index -- the sliver's numbering, not the caller's.
    pub item_index: usize,
    /// How far along its animation this row is, from 0 to 1.
    pub value: f32,
    pub duration_micros: i64,
    /// Upstream's `removedItemBuilder`, which an outgoing row is built from
    /// because the caller's builder no longer knows about it.
    pub removed_item_builder: Option<Rc<dyn Fn(f32) -> AnyWidget>>,
}

impl std::fmt::Debug for ActiveItem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActiveItem")
            .field("kind", &self.kind)
            .field("item_index", &self.item_index)
            .field("value", &self.value)
            .finish()
    }
}

impl ActiveItem {
    /// Upstream's `_ActiveItem.incoming`.
    pub fn incoming(item_index: usize, duration_micros: i64) -> ActiveItem {
        ActiveItem {
            kind: ActiveItemKind::Incoming,
            item_index,
            value: 0.0,
            duration_micros,
            removed_item_builder: None,
        }
    }

    /// Upstream's `_ActiveItem.outgoing`.
    ///
    /// It starts at one, not zero: the row is already fully there, and the
    /// animation runs *backwards* from where it is.
    pub fn outgoing(
        item_index: usize,
        duration_micros: i64,
        value: f32,
        removed_item_builder: Rc<dyn Fn(f32) -> AnyWidget>,
    ) -> ActiveItem {
        ActiveItem {
            kind: ActiveItemKind::Outgoing,
            item_index,
            value,
            duration_micros,
            removed_item_builder: Some(removed_item_builder),
        }
    }
}

/// Upstream's `_SliverAnimatedMultiBoxAdaptorState`: which rows exist, which
/// are arriving, and which are still leaving.
///
/// Shared by all four public states, because upstream shares it too -- the
/// list and the grid differ only in the sliver they build.
#[derive(Debug, Default)]
pub struct AnimatedItems {
    /// Sorted by item index, as upstream keeps them.
    incoming: Vec<ActiveItem>,
    outgoing: Vec<ActiveItem>,
    items_count: usize,
}

impl AnimatedItems {
    pub fn new(initial_item_count: usize) -> AnimatedItems {
        AnimatedItems {
            incoming: Vec::new(),
            outgoing: Vec::new(),
            items_count: initial_item_count,
        }
    }

    /// How many slots the sliver is being asked for, rows still leaving
    /// included. Upstream's `_itemsCount`.
    pub fn items_count(&self) -> usize {
        self.items_count
    }

    /// How many rows a caller would say there are: the slots less the ones
    /// still shrinking away. Upstream computes this inline in `removeAllItems`.
    pub fn visible_item_count(&self) -> usize {
        self.items_count.saturating_sub(self.outgoing.len())
    }

    pub fn incoming(&self) -> &[ActiveItem] {
        &self.incoming
    }

    pub fn outgoing(&self) -> &[ActiveItem] {
        &self.outgoing
    }

    fn active_at(items: &[ActiveItem], item_index: usize) -> Option<usize> {
        items.iter().position(|item| item.item_index == item_index)
    }

    /// Upstream's `_indexToItemIndex`: a caller's index to the sliver's.
    ///
    /// Walks the outgoing rows in order and takes a slot for each one **at or
    /// before** the running answer. The `<=` is what makes an insertion land
    /// after a row that is still leaving from the same place, rather than in
    /// front of it.
    ///
    /// The `break` is not an optimisation: the list is sorted, so the first
    /// outgoing row past the answer means every later one is too, and counting
    /// them would move the answer past rows that are not in its way.
    pub fn index_to_item_index(&self, index: usize) -> usize {
        let mut item_index = index;
        for item in &self.outgoing {
            if item.item_index <= item_index {
                item_index += 1;
            } else {
                break;
            }
        }
        item_index
    }

    /// Upstream's `_itemIndexToIndex`: the sliver's numbering back to the
    /// caller's.
    ///
    /// **Strictly less than**, where the forward direction used `<=`. The two
    /// are not each other's mirror by accident: this one is only ever asked
    /// about a row that is *not* leaving -- upstream asserts exactly that --
    /// so an outgoing row at the same index cannot arise, while in the forward
    /// direction it is the ordinary case.
    pub fn item_index_to_index(&self, item_index: usize) -> usize {
        debug_assert!(
            Self::active_at(&self.outgoing, item_index).is_none(),
            "a row that is leaving has no caller-facing index"
        );
        let mut index = item_index;
        for item in &self.outgoing {
            if item.item_index < item_index {
                index -= 1;
            } else {
                break;
            }
        }
        index
    }

    /// Upstream's `insertItem`.
    ///
    /// Every active row at or after the new slot shifts up by one -- **both**
    /// lists, because a row still leaving occupies a slot just as much as one
    /// arriving does.
    pub fn insert_item(&mut self, index: usize, duration_micros: i64) {
        let item_index = self.index_to_item_index(index);
        debug_assert!(item_index <= self.items_count);
        for item in self.incoming.iter_mut().chain(self.outgoing.iter_mut()) {
            if item.item_index >= item_index {
                item.item_index += 1;
            }
        }
        self.incoming
            .push(ActiveItem::incoming(item_index, duration_micros));
        self.incoming.sort_by_key(|item| item.item_index);
        self.items_count += 1;
    }

    /// Upstream's `insertAllItems`, which is `insertItem` in a loop **going
    /// up**: each insertion shifts the later ones along, so `index + i` is the
    /// same place each time.
    pub fn insert_all_items(&mut self, index: usize, length: usize, duration_micros: i64) {
        for offset in 0..length {
            self.insert_item(index + offset, duration_micros);
        }
    }

    /// Upstream's `removeItem`.
    ///
    /// A row removed while it is still arriving **keeps its own controller**
    /// rather than getting a fresh one: it reverses from wherever it had got
    /// to, so a row cancelled halfway shrinks from half size instead of
    /// jumping to full size first.
    pub fn remove_item(
        &mut self,
        index: usize,
        duration_micros: i64,
        removed_item_builder: Rc<dyn Fn(f32) -> AnyWidget>,
    ) {
        let item_index = self.index_to_item_index(index);
        debug_assert!(item_index < self.items_count);
        debug_assert!(Self::active_at(&self.outgoing, item_index).is_none());

        let value = match Self::active_at(&self.incoming, item_index) {
            Some(at) => self.incoming.remove(at).value,
            None => 1.0,
        };
        self.outgoing.push(ActiveItem::outgoing(
            item_index,
            duration_micros,
            value,
            removed_item_builder,
        ));
        self.outgoing.sort_by_key(|item| item.item_index);
    }

    /// Upstream's `removeAllItems`, which counts **downwards** from the last
    /// visible row.
    ///
    /// Going upwards would not work: each removal is expressed in the caller's
    /// index space, and removing row 0 renumbers everything after it.
    pub fn remove_all_items(
        &mut self,
        duration_micros: i64,
        removed_item_builder: Rc<dyn Fn(f32) -> AnyWidget>,
    ) {
        for index in (0..self.visible_item_count()).rev() {
            self.remove_item(index, duration_micros, removed_item_builder.clone());
        }
    }

    /// Builds a leaving row from the builder it was removed with -- upstream's
    /// `outgoingItem.removedItemBuilder!(context, controller.view)`.
    pub fn build_leaving(&self, item_index: usize) -> Option<AnyWidget> {
        let at = Self::active_at(&self.outgoing, item_index)?;
        let item = &self.outgoing[at];
        item.removed_item_builder
            .as_ref()
            .map(|build| build(item.value))
    }

    /// The end of an insertion's animation: upstream's `controller.forward()`
    /// continuation, which drops the item and leaves the count alone -- the
    /// count went up when the insertion started.
    pub fn finish_incoming(&mut self, item_index: usize) -> bool {
        match Self::active_at(&self.incoming, item_index) {
            Some(at) => {
                self.incoming.remove(at);
                true
            }
            None => false,
        }
    }

    /// The end of a removal's animation: upstream's `controller.reverse()`
    /// continuation.
    ///
    /// **Now** the slot goes away, and every active row after it comes down by
    /// one. This is the moment the two index spaces converge again for that
    /// row.
    pub fn finish_outgoing(&mut self, item_index: usize) -> bool {
        let Some(at) = Self::active_at(&self.outgoing, item_index) else {
            return false;
        };
        self.outgoing.remove(at);
        for item in self.incoming.iter_mut().chain(self.outgoing.iter_mut()) {
            if item.item_index > item_index {
                item.item_index -= 1;
            }
        }
        self.items_count -= 1;
        true
    }

    /// Moves every active animation forward, and returns whether any is still
    /// running. Incoming rows grow towards one, outgoing rows shrink towards
    /// zero, and the ones that arrive are retired.
    pub fn advance(&mut self, elapsed_micros: i64) -> bool {
        let mut finished_incoming = Vec::new();
        for item in self.incoming.iter_mut() {
            item.value =
                (item.value + elapsed_micros as f32 / item.duration_micros as f32).min(1.0);
            if item.value >= 1.0 {
                finished_incoming.push(item.item_index);
            }
        }
        let mut finished_outgoing = Vec::new();
        for item in self.outgoing.iter_mut() {
            item.value =
                (item.value - elapsed_micros as f32 / item.duration_micros as f32).max(0.0);
            if item.value <= 0.0 {
                finished_outgoing.push(item.item_index);
            }
        }
        for item_index in finished_incoming {
            self.finish_incoming(item_index);
        }
        // Descending, so that each removal's renumbering does not move a slot
        // that is still to be retired.
        finished_outgoing.sort_unstable();
        for item_index in finished_outgoing.into_iter().rev() {
            self.finish_outgoing(item_index);
        }
        !self.incoming.is_empty() || !self.outgoing.is_empty()
    }

    /// What the sliver should build at a slot, upstream's `_itemBuilder`.
    ///
    /// A leaving row is built from the *removal* builder, because the caller's
    /// own builder no longer has anything at that index. Everything else is
    /// built from the caller's builder at its caller-facing index, with an
    /// animation that is [`kAlwaysCompleteAnimation`](SlotAnimation::Settled)
    /// when nothing is happening to it.
    pub fn slot(&self, item_index: usize) -> Slot {
        if let Some(at) = Self::active_at(&self.outgoing, item_index) {
            return Slot::Leaving {
                value: self.outgoing[at].value,
            };
        }
        let value = Self::active_at(&self.incoming, item_index)
            .map(|at| self.incoming[at].value)
            .unwrap_or(1.0);
        Slot::Present {
            index: self.item_index_to_index(item_index),
            value,
        }
    }
}

/// What a slot of the sliver holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    /// A row the caller still knows about, at its caller-facing index.
    Present { index: usize, value: f32 },
    /// A row that has been removed and is still shrinking away.
    Leaving { value: f32 },
}

/// The animation a settled row is handed: upstream's
/// `kAlwaysCompleteAnimation`, which is a real object rather than a null so
/// that a builder never has to test for one.
pub struct SlotAnimation;

impl SlotAnimation {
    pub const SETTLED: f32 = 1.0;
}

/// Upstream `SliverAnimatedList`: a `SliverList` whose rows animate in and out.
pub struct SliverAnimatedList {
    pub initial_item_count: usize,
}

/// Upstream `SliverAnimatedListState`.
#[derive(Debug, Default)]
pub struct SliverAnimatedListState {
    pub items: AnimatedItems,
}

/// Upstream `SliverAnimatedGrid`: the same for a grid.
///
/// The only difference from the list is the sliver it builds; the bookkeeping
/// in [`AnimatedItems`] is shared, as it is upstream.
pub struct SliverAnimatedGrid {
    pub initial_item_count: usize,
}

/// Upstream `SliverAnimatedGridState`.
#[derive(Debug, Default)]
pub struct SliverAnimatedGridState {
    pub items: AnimatedItems,
}

/// Upstream `AnimatedList`: a scroll view around a [`SliverAnimatedList`].
pub struct AnimatedList {
    pub initial_item_count: usize,
}

/// Upstream `AnimatedListState`, which does nothing but forward to the
/// sliver's state -- upstream's does the same.
#[derive(Debug, Default)]
pub struct AnimatedListState {
    pub items: AnimatedItems,
}

/// Upstream `AnimatedGrid`: a scroll view around a [`SliverAnimatedGrid`].
pub struct AnimatedGrid {
    pub initial_item_count: usize,
}

/// Upstream `AnimatedGridState`.
#[derive(Debug, Default)]
pub struct AnimatedGridState {
    pub items: AnimatedItems,
}

// The four pairs differ only in name -- upstream's do too, above a shared
// `_SliverAnimatedMultiBoxAdaptorState` -- so their methods are written once.
// Note that the *types* above are spelled out rather than generated: a name
// that exists only as a macro argument is invisible to `tools/coverage.py`,
// which reads declarations and is right not to look inside macro calls. A port
// that hides its class names from the ruler is a port the ruler cannot check.
macro_rules! animated_scroll_view {
    ($widget:ident, $state:ident) => {
        impl $widget {
            pub fn new(initial_item_count: usize) -> $widget {
                $widget { initial_item_count }
            }

            /// Upstream's `createState`.
            pub fn create_state(&self) -> $state {
                $state {
                    items: AnimatedItems::new(self.initial_item_count),
                }
            }
        }

        impl $state {
            /// Upstream's `insertItem`.
            pub fn insert_item(&mut self, index: usize) {
                self.items.insert_item(index, ANIMATED_ITEM_DURATION_MICROS);
            }

            /// Upstream's `insertAllItems`.
            pub fn insert_all_items(&mut self, index: usize, length: usize) {
                self.items
                    .insert_all_items(index, length, ANIMATED_ITEM_DURATION_MICROS);
            }

            /// Upstream's `removeItem`.
            pub fn remove_item(
                &mut self,
                index: usize,
                removed_item_builder: Rc<dyn Fn(f32) -> AnyWidget>,
            ) {
                self.items
                    .remove_item(index, ANIMATED_ITEM_DURATION_MICROS, removed_item_builder);
            }

            /// Upstream's `removeAllItems`.
            pub fn remove_all_items(&mut self, removed_item_builder: Rc<dyn Fn(f32) -> AnyWidget>) {
                self.items
                    .remove_all_items(ANIMATED_ITEM_DURATION_MICROS, removed_item_builder);
            }
        }
    };
}

animated_scroll_view!(SliverAnimatedList, SliverAnimatedListState);
animated_scroll_view!(SliverAnimatedGrid, SliverAnimatedGridState);
animated_scroll_view!(AnimatedList, AnimatedListState);
animated_scroll_view!(AnimatedGrid, AnimatedGridState);

#[cfg(test)]
mod tests {
    use super::*;

    fn list(count: usize) -> AnimatedItems {
        AnimatedItems::new(count)
    }

    /// A removal builder, which upstream requires and which is what a leaving
    /// row is built from once the caller's own builder has forgotten it.
    fn gone() -> Rc<dyn Fn(f32) -> AnyWidget> {
        Rc::new(|_value| crate::framework::leaf(|| crate::widgets::Empty))
    }

    #[test]
    fn a_row_that_is_still_leaving_still_takes_up_a_slot() {
        // The whole reason there are two index spaces. Upstream's comment: the
        // index parameters are defined as if the removal had happened at once,
        // but the entry is only really gone when its animation finishes.
        let mut items = list(5);
        items.remove_item(2, ANIMATED_ITEM_DURATION_MICROS, gone());
        assert_eq!(items.items_count(), 5, "the sliver still has five slots");
        assert_eq!(items.visible_item_count(), 4, "the caller sees four");
    }

    #[test]
    fn the_callers_index_and_the_slivers_diverge_the_moment_a_row_leaves() {
        let mut items = list(5);
        items.remove_item(2, ANIMATED_ITEM_DURATION_MICROS, gone());
        // Rows before the departing one are unaffected.
        assert_eq!(items.index_to_item_index(0), 0);
        assert_eq!(items.index_to_item_index(1), 1);
        // From the departing row onwards, the caller's index is one behind.
        assert_eq!(items.index_to_item_index(2), 3);
        assert_eq!(items.index_to_item_index(3), 4);
        // And back the other way, for rows that are not leaving.
        assert_eq!(items.item_index_to_index(0), 0);
        assert_eq!(items.item_index_to_index(3), 2);
        assert_eq!(items.item_index_to_index(4), 3);
    }

    #[test]
    fn removing_row_three_twice_removes_two_different_rows() {
        // Which is the point of the caller-facing index space: a caller
        // clearing rows one at a time should not have to know how many are
        // still animating away.
        let mut items = list(6);
        items.remove_item(3, ANIMATED_ITEM_DURATION_MICROS, gone());
        items.remove_item(3, ANIMATED_ITEM_DURATION_MICROS, gone());
        let leaving: Vec<usize> = items
            .outgoing()
            .iter()
            .map(|item| item.item_index)
            .collect();
        assert_eq!(leaving, vec![3, 4], "two different slots");
        assert_eq!(items.visible_item_count(), 4);
    }

    #[test]
    fn the_two_conversions_use_different_comparisons_and_that_is_deliberate() {
        // Forward uses `<=` and back uses `<`. The reverse direction is only
        // ever asked about a row that is *not* leaving -- upstream asserts it
        // -- so an outgoing row at the same index cannot arise there, while
        // going forward it is the ordinary case.
        let mut items = list(5);
        items.remove_item(2, ANIMATED_ITEM_DURATION_MICROS, gone());
        // An insertion at the caller's index 2 lands *after* the row leaving
        // from that same place, not in front of it.
        assert_eq!(items.index_to_item_index(2), 3);
        // The inverse is not asked about slot 2 at all; slot 3 comes back as 2.
        assert_eq!(items.item_index_to_index(3), 2);
    }

    #[test]
    fn an_insertion_shifts_everything_that_is_animating_including_what_is_leaving() {
        // A row on its way out occupies a slot exactly as much as one on its
        // way in, so both lists have to move.
        let mut items = list(5);
        items.remove_item(4, ANIMATED_ITEM_DURATION_MICROS, gone());
        items.insert_item(0, ANIMATED_ITEM_DURATION_MICROS);
        assert_eq!(
            items.outgoing()[0].item_index,
            5,
            "the leaving row moved along with everything else"
        );
        assert_eq!(items.incoming()[0].item_index, 0);
        assert_eq!(items.items_count(), 6);
    }

    #[test]
    fn a_row_removed_while_still_arriving_reverses_from_where_it_had_got_to() {
        // Rather than being handed a fresh animation at full size, which would
        // make a cancelled insertion visibly pop out to full size before
        // shrinking.
        let mut items = list(3);
        items.insert_item(1, ANIMATED_ITEM_DURATION_MICROS);
        items.advance(ANIMATED_ITEM_DURATION_MICROS / 2);
        let halfway = items.incoming()[0].value;
        assert!((halfway - 0.5).abs() < 1e-3, "halfway in: {halfway}");

        items.remove_item(1, ANIMATED_ITEM_DURATION_MICROS, gone());
        assert!(items.incoming().is_empty(), "it stopped arriving");
        assert!(
            (items.outgoing()[0].value - halfway).abs() < 1e-6,
            "and left from where it was, not from the top"
        );
    }

    #[test]
    fn a_slot_still_leaving_is_built_by_the_removal_builder() {
        // The caller's own builder no longer has anything at that index, so
        // there would be nothing to build it from.
        let mut items = list(4);
        items.remove_item(1, ANIMATED_ITEM_DURATION_MICROS, gone());
        assert_eq!(items.slot(1), Slot::Leaving { value: 1.0 });
        assert!(
            items.build_leaving(1).is_some(),
            "and it is built from the builder it was removed with"
        );
        assert!(items.build_leaving(0).is_none(), "row 0 is not leaving");
        assert_eq!(
            items.slot(0),
            Slot::Present {
                index: 0,
                value: 1.0
            }
        );
        assert_eq!(
            items.slot(2),
            Slot::Present {
                index: 1,
                value: 1.0
            },
            "the slot after it is the caller's row 1"
        );
    }

    #[test]
    fn a_settled_row_gets_a_finished_animation_rather_than_none_at_all() {
        // Upstream hands kAlwaysCompleteAnimation, which is a real object
        // rather than a null, so a builder never has to test for one.
        let items = list(3);
        assert_eq!(
            items.slot(1),
            Slot::Present {
                index: 1,
                value: SlotAnimation::SETTLED
            }
        );
    }

    #[test]
    fn the_slot_goes_away_only_when_the_animation_finishes() {
        let mut items = list(4);
        items.remove_item(1, ANIMATED_ITEM_DURATION_MICROS, gone());
        assert_eq!(items.items_count(), 4);
        assert!(items.advance(ANIMATED_ITEM_DURATION_MICROS / 2));
        assert_eq!(items.items_count(), 4, "still shrinking");
        assert!(!items.advance(ANIMATED_ITEM_DURATION_MICROS));
        assert_eq!(items.items_count(), 3, "now the slot is gone");
        assert_eq!(items.visible_item_count(), 3);
        // And the two index spaces agree again.
        assert_eq!(items.index_to_item_index(1), 1);
    }

    #[test]
    fn finishing_a_removal_renumbers_everything_after_it() {
        let mut items = list(6);
        items.remove_item(1, ANIMATED_ITEM_DURATION_MICROS, gone());
        items.insert_item(3, ANIMATED_ITEM_DURATION_MICROS);
        let arriving = items.incoming()[0].item_index;
        assert_eq!(arriving, 4, "past the leaving row");
        items.finish_outgoing(1);
        assert_eq!(
            items.incoming()[0].item_index,
            3,
            "and it came down when the slot went"
        );
    }

    #[test]
    fn inserting_a_run_walks_upwards_and_removing_them_all_walks_down() {
        // Insertions go up because each one shifts the later ones along, so
        // index + i is the same place each time. Removals go down because each
        // is expressed in the caller's index space, and removing row 0 would
        // renumber everything after it.
        let mut items = list(2);
        items.insert_all_items(1, 3, ANIMATED_ITEM_DURATION_MICROS);
        let arriving: Vec<usize> = items
            .incoming()
            .iter()
            .map(|item| item.item_index)
            .collect();
        assert_eq!(arriving, vec![1, 2, 3]);
        assert_eq!(items.items_count(), 5);

        items.advance(ANIMATED_ITEM_DURATION_MICROS);
        items.remove_all_items(ANIMATED_ITEM_DURATION_MICROS, gone());
        let leaving: Vec<usize> = items
            .outgoing()
            .iter()
            .map(|item| item.item_index)
            .collect();
        assert_eq!(leaving, vec![0, 1, 2, 3, 4], "every one of them, once");
        assert_eq!(items.visible_item_count(), 0);

        items.advance(ANIMATED_ITEM_DURATION_MICROS);
        assert_eq!(items.items_count(), 0);
    }

    #[test]
    fn removing_them_all_does_not_touch_rows_that_are_already_leaving() {
        // removeAllItems counts from the *visible* count, so a row already on
        // its way out is not removed a second time -- which upstream's own
        // assertion in removeItem would catch.
        let mut items = list(4);
        items.remove_item(0, ANIMATED_ITEM_DURATION_MICROS, gone());
        items.remove_all_items(ANIMATED_ITEM_DURATION_MICROS, gone());
        assert_eq!(items.outgoing().len(), 4, "one plus the other three");
        assert_eq!(items.visible_item_count(), 0);
    }

    #[test]
    fn an_insertion_animates_in_and_then_stops_being_active() {
        let mut items = list(2);
        items.insert_item(2, ANIMATED_ITEM_DURATION_MICROS);
        assert_eq!(items.items_count(), 3, "the count went up at once");
        assert!(items.advance(ANIMATED_ITEM_DURATION_MICROS / 3));
        assert_eq!(items.incoming().len(), 1);
        assert!(!items.advance(ANIMATED_ITEM_DURATION_MICROS));
        assert!(items.incoming().is_empty());
        assert_eq!(items.items_count(), 3, "and stayed up");
    }

    #[test]
    fn the_four_widgets_are_the_same_bookkeeping_with_four_names() {
        // Which is upstream's arrangement exactly: the list and the grid
        // differ in the sliver they build, and the two non-sliver widgets
        // wrap the two slivers.
        let mut sliver_list = SliverAnimatedList::new(3).create_state();
        sliver_list.insert_item(0);
        assert_eq!(sliver_list.items.items_count(), 4);

        let mut sliver_grid = SliverAnimatedGrid::new(3).create_state();
        sliver_grid.remove_item(0, gone());
        assert_eq!(sliver_grid.items.visible_item_count(), 2);

        let mut animated_list = AnimatedList::new(2).create_state();
        animated_list.insert_all_items(0, 2);
        assert_eq!(animated_list.items.items_count(), 4);

        let mut animated_grid = AnimatedGrid::new(2).create_state();
        animated_grid.remove_all_items(gone());
        assert_eq!(animated_grid.items.visible_item_count(), 0);
        assert_eq!(animated_grid.items.items_count(), 2, "not gone yet");
    }

    #[test]
    fn the_default_duration_is_upstreams() {
        assert_eq!(ANIMATED_ITEM_DURATION_MICROS, 300_000);
    }
}
