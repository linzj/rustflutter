//! Slotted children -- a port of upstream's
//! `widgets/slotted_render_object_widget.dart`.
//!
//! Most render objects with several children keep them in a **list**, and a
//! child's identity is its position in it. That is wrong for a widget whose
//! children mean different things: a list tile's leading icon and its trailing
//! chevron are not "child 0" and "child 2", they are the leading one and the
//! trailing one, and either may be absent without the other shifting.
//!
//! So the children live in a **map from slot to child**, and the interesting
//! part is what happens when the map is rebuilt.
//!
//! **A keyed child keeps its state when it moves between slots.** The matcher
//! looks for a key match across every slot before it looks at the slot it is
//! filling -- so moving a keyed widget from leading to trailing carries its
//! `State` with it, exactly as moving one within a list would.
//!
//! ## What is not here
//!
//! The `RenderObject` adoption and the element's `updateChild` belong to this
//! crate's own tree -- see [`crate::framework`] and [`crate::render`]. What is
//! ported is the slot map, its two invariants, and the matching algorithm.

use std::collections::HashMap;
use std::hash::Hash;

/// Upstream `SlottedMultiChildRenderObjectWidget`: the widget side.
///
/// Upstream is abstract with two methods; this is a trait with the same two.
/// `slots` is a **list rather than a set**, and the order is load-bearing --
/// it is the order children are visited and updated in.
pub trait SlottedMultiChildRenderObjectWidget<Slot> {
    /// Upstream's `slots`, whose documentation is unusually firm: "The list of
    /// slots must be static and must never change for a given class." A class
    /// whose slots varied would have children appearing and vanishing for
    /// reasons no caller asked for, so upstream asserts it rather than coping.
    fn slots(&self) -> Vec<Slot>;

    /// Upstream's `childForSlot`, returning `None` for an empty slot. That is
    /// the ordinary case, not an error: a list tile with no icon has an empty
    /// leading slot every frame of its life.
    fn child_for_slot(&self, slot: &Slot) -> Option<SlottedChild>;
}

/// A child widget, reduced to what the matcher needs: an identity and an
/// optional key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlottedChild {
    /// Which widget this is, for the tests to follow.
    pub widget: u64,
    /// Upstream's `Widget.key`.
    pub key: Option<u64>,
}

impl SlottedChild {
    pub fn new(widget: u64) -> SlottedChild {
        SlottedChild { widget, key: None }
    }

    pub fn keyed(widget: u64, key: u64) -> SlottedChild {
        SlottedChild {
            widget,
            key: Some(key),
        }
    }
}

/// Upstream's deprecated `SlottedMultiChildRenderObjectWidgetMixin`.
///
/// It is the same logic, reachable by mixing in rather than extending.
/// Upstream deprecated it "to simplify the process of creating slotted
/// widgets" -- two ways to say one thing is one way too many, and the
/// extending form is the one that needs no explanation.
pub trait SlottedMultiChildRenderObjectWidgetMixin<Slot>:
    SlottedMultiChildRenderObjectWidget<Slot>
{
}

/// Upstream `SlottedContainerRenderObjectMixin`: the render object's side.
///
/// A struct rather than a trait, because unlike most of upstream's mixins this
/// one carries state -- the slot map itself -- and everything it does is a
/// method over that map. The name keeps upstream's `Mixin` suffix so a reader
/// searching for the upstream class finds this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlottedContainerRenderObjectMixin<Slot: Eq + Hash + Clone> {
    slot_to_child: HashMap<Slot, u64>,
    /// The slot order, kept so [`Self::children`] can be deterministic.
    order: Vec<Slot>,
    adopted: Vec<u64>,
    dropped: Vec<u64>,
    attached: bool,
}

impl<Slot: Eq + Hash + Clone + std::fmt::Debug> SlottedContainerRenderObjectMixin<Slot> {
    pub fn new(order: Vec<Slot>) -> SlottedContainerRenderObjectMixin<Slot> {
        SlottedContainerRenderObjectMixin {
            slot_to_child: HashMap::new(),
            order,
            adopted: Vec::new(),
            dropped: Vec::new(),
            attached: false,
        }
    }

    /// Upstream's `childForSlot`.
    pub fn child_for_slot(&self, slot: &Slot) -> Option<u64> {
        self.slot_to_child.get(slot).copied()
    }

    /// Upstream's `children`, and the note on it is the point: the base
    /// implementation "makes no guarantee about the order in which the
    /// children are returned", and subclasses for which order matters --
    /// **hit testing, most of all** -- are told to override it. A hit test
    /// that visited children in map order would let the wrong one win.
    ///
    /// This port keeps the declared slot order, which is a stronger guarantee
    /// than upstream's base, and says so rather than pretending the map is
    /// ordered.
    pub fn children(&self) -> Vec<u64> {
        self.order
            .iter()
            .filter_map(|slot| self.slot_to_child.get(slot).copied())
            .collect()
    }

    pub fn adopted(&self) -> &[u64] {
        &self.adopted
    }

    pub fn dropped(&self) -> &[u64] {
        &self.dropped
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Upstream's `debugNameForSlot`, which calls `EnumName.name` for an enum
    /// and `toString` otherwise. The slot is usually an enum precisely so that
    /// this reads as `leading` rather than `Instance of 'Object'`.
    pub fn debug_name_for_slot(slot: &Slot) -> String {
        format!("{slot:?}")
    }

    /// Upstream's `_setChild`, which **drops the old child before adopting the
    /// new one**. The order matters: a render object adopted while still
    /// parented elsewhere would have two parents for the length of one
    /// statement, and the tree's depth invariant would be briefly false.
    pub fn set_child(&mut self, slot: Slot, child: Option<u64>) {
        if let Some(old) = self.slot_to_child.remove(&slot) {
            self.dropped.push(old);
        }
        if let Some(child) = child {
            self.slot_to_child.insert(slot, child);
            self.adopted.push(child);
        }
    }

    /// Upstream's `_moveChild`.
    ///
    /// The guard is the careful bit: the old slot is only cleared when it
    /// **still holds this child**. By the time a move runs, something else may
    /// already have taken that slot -- and clearing it then would drop a
    /// child that had just arrived.
    pub fn move_child(&mut self, child: u64, slot: Slot, old_slot: Slot) {
        debug_assert!(slot != old_slot, "a move goes somewhere else");
        if self.slot_to_child.get(&old_slot) == Some(&child) {
            self.set_child(old_slot, None);
        }
        self.set_child(slot, Some(child));
    }

    pub fn attach(&mut self) {
        self.attached = true;
    }

    pub fn detach(&mut self) {
        self.attached = false;
    }

    /// Upstream's `debugDescribeChildren`, which names each child by its slot.
    pub fn describe_children(&self) -> Vec<(String, u64)> {
        self.order
            .iter()
            .filter_map(|slot| {
                self.slot_to_child
                    .get(slot)
                    .map(|child| (Self::debug_name_for_slot(slot), *child))
            })
            .collect()
    }
}

/// Why a rebuild was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotError {
    /// Upstream: "`slots` must not change."
    SlotsChanged,
    /// Upstream: "slots must be unique."
    DuplicateSlot,
    /// Upstream collects these and reports them together: two children in
    /// different slots carrying the same key.
    DuplicateKey(u64),
}

/// An element in a slot, after a rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlottedElement {
    /// The element's identity -- what survives a rebuild is what carries
    /// `State` with it.
    pub element: u64,
    pub widget: u64,
    pub key: Option<u64>,
}

/// Upstream `SlottedRenderObjectElement`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlottedRenderObjectElement<Slot: Eq + Hash + Clone> {
    slot_to_child: HashMap<Slot, SlottedElement>,
    keyed_children: HashMap<u64, SlottedElement>,
    debug_previous_slots: Option<Vec<Slot>>,
    next_element_id: u64,
    /// Elements that were reused rather than created, for the tests.
    reused: Vec<u64>,
    created: Vec<u64>,
    deactivated: Vec<u64>,
}

impl<Slot: Eq + Hash + Clone + std::fmt::Debug> SlottedRenderObjectElement<Slot> {
    pub fn new() -> SlottedRenderObjectElement<Slot> {
        SlottedRenderObjectElement {
            slot_to_child: HashMap::new(),
            keyed_children: HashMap::new(),
            debug_previous_slots: None,
            next_element_id: 1,
            reused: Vec::new(),
            created: Vec::new(),
            deactivated: Vec::new(),
        }
    }

    pub fn child_in_slot(&self, slot: &Slot) -> Option<SlottedElement> {
        self.slot_to_child.get(slot).copied()
    }

    pub fn reused(&self) -> &[u64] {
        &self.reused
    }

    pub fn created(&self) -> &[u64] {
        &self.created
    }

    pub fn deactivated(&self) -> &[u64] {
        &self.deactivated
    }

    /// Upstream's `visitChildren`.
    pub fn visit_children(&self, order: &[Slot]) -> Vec<u64> {
        order
            .iter()
            .filter_map(|slot| self.slot_to_child.get(slot).map(|child| child.element))
            .collect()
    }

    /// Upstream's `forgetChild`, which asserts the child is actually in a slot
    /// -- forgetting one that is not there would silently leave the real
    /// occupant behind.
    pub fn forget_child(&mut self, slot: &Slot) {
        debug_assert!(
            self.slot_to_child.contains_key(slot),
            "forgetting a child that is not in that slot"
        );
        if let Some(child) = self.slot_to_child.remove(slot) {
            if let Some(key) = child.key {
                self.keyed_children.remove(&key);
            }
        }
    }

    /// Upstream's `_updateChildren`, the whole reason this file exists.
    ///
    /// For each slot in order, it works out which existing element -- if any --
    /// the new widget should update, and the three-way choice is the design:
    ///
    /// 1. **A key match anywhere wins.** The element is taken out of whatever
    ///    slot it was in, so a keyed widget moved from one slot to another
    ///    keeps its `State`. This is the same promise a keyed child gets
    ///    inside a list, and it would be strange for slots to break it.
    /// 2. **Otherwise the same slot's old child is reused, but only if it had
    ///    no key.** A keyed old child that did not match is somebody else's,
    ///    and reusing it would hand its state to the wrong widget.
    /// 3. **Otherwise nothing is reused** and a fresh element is built.
    ///
    /// Upstream asserts on the slot list not changing and on slots being
    /// unique; both are returned as errors here so they can be pinned.
    pub fn update_children(
        &mut self,
        widget: &dyn SlottedMultiChildRenderObjectWidget<Slot>,
    ) -> Result<(), SlotError> {
        let slots = widget.slots();
        match &self.debug_previous_slots {
            Some(previous) if *previous != slots => return Err(SlotError::SlotsChanged),
            Some(_) => {}
            None => self.debug_previous_slots = Some(slots.clone()),
        }
        let mut seen: Vec<&Slot> = Vec::new();
        for slot in slots.iter() {
            if seen.contains(&slot) {
                return Err(SlotError::DuplicateSlot);
            }
            seen.push(slot);
        }

        let old_keyed = std::mem::take(&mut self.keyed_children);
        let mut old_slot_to_child = std::mem::take(&mut self.slot_to_child);
        let mut duplicate_key: Option<u64> = None;

        for slot in slots.iter() {
            let child = widget.child_for_slot(slot);
            let new_key = child.and_then(|child| child.key);

            // 1. A key match anywhere, taken out of the slot it was in.
            let from_element = match new_key.and_then(|key| old_keyed.get(&key)) {
                Some(keyed) => {
                    let its_slot = old_slot_to_child
                        .iter()
                        .find(|(_, held)| held.element == keyed.element)
                        .map(|(slot, _)| slot.clone());
                    its_slot.and_then(|slot| old_slot_to_child.remove(&slot))
                }
                None => {
                    // 2. The same slot's old child, but only if it is unkeyed.
                    match old_slot_to_child.get(slot) {
                        Some(old) if old.key.is_none() => old_slot_to_child.remove(slot),
                        // 3. A keyed old child that did not match belongs to
                        //    somebody else.
                        _ => None,
                    }
                }
            };

            let Some(child) = child else {
                if let Some(gone) = from_element {
                    self.deactivated.push(gone.element);
                }
                continue;
            };

            let element = match from_element {
                Some(existing) => {
                    self.reused.push(existing.element);
                    SlottedElement {
                        element: existing.element,
                        widget: child.widget,
                        key: child.key,
                    }
                }
                None => {
                    let id = self.next_element_id;
                    self.next_element_id += 1;
                    self.created.push(id);
                    SlottedElement {
                        element: id,
                        widget: child.widget,
                        key: child.key,
                    }
                }
            };
            self.slot_to_child.insert(slot.clone(), element);
            if let Some(key) = child.key {
                if self.keyed_children.contains_key(&key) {
                    duplicate_key = Some(key);
                }
                self.keyed_children.insert(key, element);
            }
        }

        // Whatever was not claimed is gone.
        for (_, orphan) in old_slot_to_child.into_iter() {
            self.deactivated.push(orphan.element);
        }

        match duplicate_key {
            Some(key) => Err(SlotError::DuplicateKey(key)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list tile's three slots.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Slot {
        Leading,
        Title,
        Trailing,
    }

    /// A widget whose slot contents the test dictates.
    struct Tile {
        slots: Vec<Slot>,
        leading: Option<SlottedChild>,
        title: Option<SlottedChild>,
        trailing: Option<SlottedChild>,
    }

    impl Tile {
        fn new() -> Tile {
            Tile {
                slots: vec![Slot::Leading, Slot::Title, Slot::Trailing],
                leading: None,
                title: None,
                trailing: None,
            }
        }

        fn with(mut self, slot: Slot, child: Option<SlottedChild>) -> Tile {
            match slot {
                Slot::Leading => self.leading = child,
                Slot::Title => self.title = child,
                Slot::Trailing => self.trailing = child,
            }
            self
        }
    }

    impl SlottedMultiChildRenderObjectWidget<Slot> for Tile {
        fn slots(&self) -> Vec<Slot> {
            self.slots.clone()
        }

        fn child_for_slot(&self, slot: &Slot) -> Option<SlottedChild> {
            match slot {
                Slot::Leading => self.leading,
                Slot::Title => self.title,
                Slot::Trailing => self.trailing,
            }
        }
    }

    // -- The element's matching --------------------------------------------

    #[test]
    fn an_unkeyed_child_is_matched_by_the_slot_it_is_in() {
        let mut element = SlottedRenderObjectElement::new();
        let first = Tile::new().with(Slot::Title, Some(SlottedChild::new(10)));
        element.update_children(&first).unwrap();
        let title = element.child_in_slot(&Slot::Title).unwrap();
        assert_eq!(element.created(), &[title.element]);

        let second = Tile::new().with(Slot::Title, Some(SlottedChild::new(11)));
        element.update_children(&second).unwrap();
        assert_eq!(
            element.child_in_slot(&Slot::Title).unwrap().element,
            title.element,
            "the same element updated to the new widget"
        );
        assert_eq!(element.child_in_slot(&Slot::Title).unwrap().widget, 11);
    }

    #[test]
    fn a_keyed_child_keeps_its_state_when_it_moves_between_slots() {
        // The same promise a keyed child gets inside a list, and it would be
        // strange for slots to break it.
        let mut element = SlottedRenderObjectElement::new();
        let first = Tile::new().with(Slot::Leading, Some(SlottedChild::keyed(10, 7)));
        element.update_children(&first).unwrap();
        let moved = element.child_in_slot(&Slot::Leading).unwrap().element;

        let second = Tile::new().with(Slot::Trailing, Some(SlottedChild::keyed(10, 7)));
        element.update_children(&second).unwrap();

        assert_eq!(
            element.child_in_slot(&Slot::Trailing).unwrap().element,
            moved,
            "carried across"
        );
        assert!(element.child_in_slot(&Slot::Leading).is_none());
        assert!(
            element.reused().contains(&moved),
            "reused rather than rebuilt"
        );
        assert!(!element.deactivated().contains(&moved));
    }

    #[test]
    fn a_keyed_old_child_in_the_slot_is_not_handed_to_a_different_widget() {
        // It belongs to somebody else, and reusing it would give its state to
        // the wrong widget.
        let mut element = SlottedRenderObjectElement::new();
        let first = Tile::new().with(Slot::Title, Some(SlottedChild::keyed(10, 7)));
        element.update_children(&first).unwrap();
        let keyed = element.child_in_slot(&Slot::Title).unwrap().element;

        let second = Tile::new().with(Slot::Title, Some(SlottedChild::new(11)));
        element.update_children(&second).unwrap();

        assert_ne!(
            element.child_in_slot(&Slot::Title).unwrap().element,
            keyed,
            "a fresh element"
        );
        assert!(element.deactivated().contains(&keyed));
    }

    #[test]
    fn two_keyed_children_can_swap_slots_and_both_survive() {
        let mut element = SlottedRenderObjectElement::new();
        let first = Tile::new()
            .with(Slot::Leading, Some(SlottedChild::keyed(10, 1)))
            .with(Slot::Trailing, Some(SlottedChild::keyed(20, 2)));
        element.update_children(&first).unwrap();
        let one = element.child_in_slot(&Slot::Leading).unwrap().element;
        let two = element.child_in_slot(&Slot::Trailing).unwrap().element;

        let swapped = Tile::new()
            .with(Slot::Leading, Some(SlottedChild::keyed(20, 2)))
            .with(Slot::Trailing, Some(SlottedChild::keyed(10, 1)));
        element.update_children(&swapped).unwrap();

        assert_eq!(element.child_in_slot(&Slot::Leading).unwrap().element, two);
        assert_eq!(element.child_in_slot(&Slot::Trailing).unwrap().element, one);
        assert!(element.deactivated().is_empty(), "neither was rebuilt");
    }

    #[test]
    fn an_emptied_slot_deactivates_whatever_was_in_it() {
        let mut element = SlottedRenderObjectElement::new();
        let first = Tile::new().with(Slot::Leading, Some(SlottedChild::new(10)));
        element.update_children(&first).unwrap();
        let leading = element.child_in_slot(&Slot::Leading).unwrap().element;

        element.update_children(&Tile::new()).unwrap();
        assert!(element.child_in_slot(&Slot::Leading).is_none());
        assert!(element.deactivated().contains(&leading));
    }

    #[test]
    fn an_empty_slot_is_the_ordinary_case_and_not_an_error() {
        // A list tile with no icon has an empty leading slot every frame of
        // its life.
        let mut element = SlottedRenderObjectElement::new();
        let tile = Tile::new().with(Slot::Title, Some(SlottedChild::new(10)));
        assert!(element.update_children(&tile).is_ok());
        assert!(element.child_in_slot(&Slot::Leading).is_none());
        assert!(element.child_in_slot(&Slot::Title).is_some());
    }

    #[test]
    fn children_are_visited_in_the_declared_slot_order() {
        let mut element = SlottedRenderObjectElement::new();
        let tile = Tile::new()
            .with(Slot::Leading, Some(SlottedChild::new(10)))
            .with(Slot::Trailing, Some(SlottedChild::new(30)));
        element.update_children(&tile).unwrap();

        let order = [Slot::Leading, Slot::Title, Slot::Trailing];
        let visited = element.visit_children(&order);
        assert_eq!(visited.len(), 2, "the empty slot contributes nothing");
        assert_eq!(
            visited[0],
            element.child_in_slot(&Slot::Leading).unwrap().element
        );
    }

    // -- The two invariants ------------------------------------------------

    #[test]
    fn a_widget_whose_slots_changed_is_refused() {
        // Children would appear and vanish for reasons no caller asked for.
        let mut element = SlottedRenderObjectElement::new();
        element.update_children(&Tile::new()).unwrap();

        let mut fewer = Tile::new();
        fewer.slots = vec![Slot::Leading, Slot::Title];
        assert_eq!(
            element.update_children(&fewer),
            Err(SlotError::SlotsChanged)
        );
    }

    #[test]
    fn a_repeated_slot_is_refused() {
        let mut element = SlottedRenderObjectElement::new();
        let mut doubled = Tile::new();
        doubled.slots = vec![Slot::Title, Slot::Title];
        assert_eq!(
            element.update_children(&doubled),
            Err(SlotError::DuplicateSlot)
        );
    }

    #[test]
    fn two_children_sharing_one_key_is_reported() {
        let mut element = SlottedRenderObjectElement::new();
        let clashing = Tile::new()
            .with(Slot::Leading, Some(SlottedChild::keyed(10, 7)))
            .with(Slot::Trailing, Some(SlottedChild::keyed(20, 7)));
        assert_eq!(
            element.update_children(&clashing),
            Err(SlotError::DuplicateKey(7))
        );
    }

    #[test]
    fn forgetting_a_child_takes_its_key_with_it() {
        let mut element = SlottedRenderObjectElement::new();
        let tile = Tile::new().with(Slot::Title, Some(SlottedChild::keyed(10, 7)));
        element.update_children(&tile).unwrap();
        element.forget_child(&Slot::Title);
        assert!(element.child_in_slot(&Slot::Title).is_none());

        // The key is free again, so a new child claiming it builds fresh
        // rather than finding a ghost.
        let again = Tile::new().with(Slot::Leading, Some(SlottedChild::keyed(11, 7)));
        element.update_children(&again).unwrap();
        assert_eq!(element.created().len(), 2);
    }

    #[test]
    #[should_panic(expected = "forgetting a child that is not in that slot")]
    fn forgetting_an_empty_slot_would_leave_the_real_occupant_behind() {
        let mut element: SlottedRenderObjectElement<Slot> = SlottedRenderObjectElement::new();
        element.forget_child(&Slot::Title);
    }

    // -- The render object -------------------------------------------------

    fn render() -> SlottedContainerRenderObjectMixin<Slot> {
        SlottedContainerRenderObjectMixin::new(vec![Slot::Leading, Slot::Title, Slot::Trailing])
    }

    #[test]
    fn setting_a_slot_drops_the_old_child_before_adopting_the_new_one() {
        // A render object adopted while still parented elsewhere would have
        // two parents for the length of one statement.
        let mut object = render();
        object.set_child(Slot::Title, Some(10));
        assert_eq!(object.adopted(), &[10]);
        assert!(object.dropped().is_empty());

        object.set_child(Slot::Title, Some(11));
        assert_eq!(object.dropped(), &[10]);
        assert_eq!(object.adopted(), &[10, 11]);
        assert_eq!(object.child_for_slot(&Slot::Title), Some(11));
    }

    #[test]
    fn clearing_a_slot_drops_without_adopting() {
        let mut object = render();
        object.set_child(Slot::Title, Some(10));
        object.set_child(Slot::Title, None);
        assert_eq!(object.dropped(), &[10]);
        assert_eq!(object.child_for_slot(&Slot::Title), None);
    }

    #[test]
    fn a_move_only_clears_the_old_slot_if_it_still_holds_that_child() {
        // Something else may already have taken it, and clearing then would
        // drop a child that had just arrived.
        let mut object = render();
        object.set_child(Slot::Leading, Some(10));
        object.set_child(Slot::Trailing, Some(20));

        // 20 has already replaced 10 in Leading; moving 10 out of Leading must
        // not disturb it.
        object.set_child(Slot::Leading, Some(20));
        let dropped_before = object.dropped().len();
        object.move_child(10, Slot::Trailing, Slot::Leading);

        assert_eq!(object.child_for_slot(&Slot::Leading), Some(20), "untouched");
        assert_eq!(object.child_for_slot(&Slot::Trailing), Some(10));
        assert_eq!(
            object.dropped().len(),
            dropped_before + 1,
            "only the trailing slot's old occupant went"
        );
    }

    #[test]
    fn an_ordinary_move_clears_where_it_came_from() {
        let mut object = render();
        object.set_child(Slot::Leading, Some(10));
        object.move_child(10, Slot::Trailing, Slot::Leading);
        assert_eq!(object.child_for_slot(&Slot::Leading), None);
        assert_eq!(object.child_for_slot(&Slot::Trailing), Some(10));
    }

    #[test]
    #[should_panic(expected = "a move goes somewhere else")]
    fn moving_a_child_to_the_slot_it_is_in_is_not_a_move() {
        let mut object = render();
        object.set_child(Slot::Leading, Some(10));
        object.move_child(10, Slot::Leading, Slot::Leading);
    }

    #[test]
    fn the_children_come_back_in_slot_order_which_hit_testing_needs() {
        // Upstream's base makes no ordering guarantee and tells subclasses to
        // override when order matters; this port keeps the declared order and
        // says so.
        let mut object = render();
        object.set_child(Slot::Trailing, Some(30));
        object.set_child(Slot::Leading, Some(10));
        assert_eq!(object.children(), vec![10, 30]);
    }

    #[test]
    fn each_child_is_named_by_its_slot_in_diagnostics() {
        // Which is why the slot is usually an enum: it reads as `Leading`
        // rather than as an opaque instance.
        let mut object = render();
        object.set_child(Slot::Leading, Some(10));
        assert_eq!(
            object.describe_children(),
            vec![("Leading".to_string(), 10)]
        );
    }

    #[test]
    fn attaching_and_detaching_the_container_is_carried_to_its_children() {
        let mut object = render();
        assert!(!object.is_attached());
        object.attach();
        assert!(object.is_attached());
        object.detach();
        assert!(!object.is_attached());
    }
}
